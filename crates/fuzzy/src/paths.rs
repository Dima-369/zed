use gpui::BackgroundExecutor;
use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use std::{
    cmp::{self, Ordering},
    sync::{
        Arc,
        atomic::{self, AtomicBool},
    },
};
use util::{paths::PathStyle, rel_path::RelPath};

use crate::{CharBag, matcher};

#[derive(Clone, Debug)]
pub struct PathMatchCandidate<'a> {
    pub is_dir: bool,
    pub path: &'a RelPath,
    pub char_bag: CharBag,
}

#[derive(Clone, Debug)]
pub struct PathMatch {
    pub score: f64,
    /// Guarenteed to be sorted, chars from the start of the string
    pub positions: Vec<usize>,
    pub worktree_id: usize,
    pub path: Arc<RelPath>,
    pub path_prefix: Arc<RelPath>,
    pub is_dir: bool,
    /// Number of steps removed from a shared parent with the relative path
    /// Used to order closer paths first in the search list
    pub distance_to_relative_ancestor: usize,
}

// This has only one implementation. It's here to invert dependencies so fuzzy
// does not need to depend on project. Though we also use it to make testing easier.
pub trait PathMatchCandidateSet<'a>: Send + Sync {
    type Candidates: Iterator<Item = PathMatchCandidate<'a>>;
    fn id(&self) -> usize;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn root_is_file(&self) -> bool;
    fn prefix(&self) -> Arc<RelPath>;
    fn candidates(&'a self, start: usize) -> Self::Candidates;
    fn path_style(&self) -> PathStyle;
}


impl PartialEq for PathMatch {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for PathMatch {}

impl PartialOrd for PathMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PathMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        println!(
            "{:?}: {}, {:?} {}",
            self.path, self.score, other.path, other.score
        );
        self.score
            .total_cmp(&other.score)
            .reverse()
            .then_with(|| self.worktree_id.cmp(&other.worktree_id))
            .then_with(|| {
                other
                    .distance_to_relative_ancestor
                    .cmp(&self.distance_to_relative_ancestor)
            })
            .then_with(|| {
                self.distance_from_end()
                    .total_cmp(&other.distance_from_end())
            })
            // see shorter_over_lexicographical test for an example of why we want this
            .then_with(|| {
                self.path
                    .as_unix_str()
                    .chars()
                    .count()
                    .cmp(&other.path.as_unix_str().chars().count())
            })
            .then_with(|| self.path.cmp(&other.path))
    }
}

impl PathMatch {
    fn distance_from_end(&self) -> f32 {
        let len = self.path_prefix.as_unix_str().chars().count()
            + 1
            + self.path.as_unix_str().chars().count(); // add one for path separator
        dbg!(&self.path, &self.path_prefix);
        self.positions
            .iter()
            .map(|p| (dbg!(len) - dbg!(p)) as f32 / 1000.0)
            .sum()
    }
}

pub fn match_fixed_path_set(
    candidates: Vec<PathMatchCandidate>,
    worktree_id: usize,
    worktree_root_name: Option<Arc<RelPath>>,
    query: &str,
    smart_case: bool,
    max_results: usize,
    path_style: PathStyle,
) -> Vec<PathMatch> {
    let mut config = nucleo::Config::DEFAULT;
    config.set_match_paths();
    let mut matcher = matcher::get_matcher(config);

    // Check if the user is typing a negation
    let pattern_string = if let Some(stripped) = query.strip_prefix('!') {
        // User typed "!", we want negated substring: "!'text"
        format!("!'{}", stripped)
    } else {
        // Normal substring matching: "'text"
        format!("'{}", query)
    };

    let pattern = Pattern::parse(
        &pattern_string,
        if smart_case {
            CaseMatching::Smart
        } else {
            CaseMatching::Ignore
        },
        Normalization::Smart,
    );

    let mut results = Vec::with_capacity(candidates.len());
    path_match_helper(
        &mut matcher,
        &pattern,
        candidates.into_iter(),
        worktree_id,
        &worktree_root_name
            .clone()
            .unwrap_or(RelPath::empty().into()),
        &None,
        path_style,
        &AtomicBool::new(false),
        &mut results,
    )
    .ok();
    matcher::return_matcher(matcher);
    util::truncate_to_bottom_n_sorted(&mut results, max_results);
    for r in &mut results {
        r.positions.sort();
    }
    results
}

struct Cancelled;

fn path_match_helper<'a>(
    matcher: &mut nucleo::Matcher,
    pattern: &Pattern,
    candidates: impl Iterator<Item = PathMatchCandidate<'a>>,
    worktree_id: usize,
    path_prefix: &Arc<RelPath>,
    relative_to: &Option<Arc<RelPath>>,
    path_style: PathStyle,
    cancel_flag: &AtomicBool,
    results: &mut Vec<PathMatch>,
) -> std::result::Result<(), Cancelled> {
    let mut candidate_buf = path_prefix.display(path_style).to_string();
    if !path_prefix.is_empty() {
        candidate_buf.push_str(path_style.primary_separator());
    }
    let path_prefix_len = candidate_buf.len();
    for c in candidates {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(Cancelled);
        }
        let mut indices = Vec::new();
        let mut buf = Vec::new();
        candidate_buf.truncate(path_prefix_len);
        candidate_buf.push_str(c.path.as_unix_str());
        // TODO: need to convert indices/positions from char offsets to byte offsets.
        if let Some(score) = pattern.indices(
            nucleo::Utf32Str::new(dbg!(&candidate_buf), &mut buf),
            matcher,
            &mut indices,
        ) {
            // TODO: walk both in order for better perf
            let positions: Vec<_> = candidate_buf
                .char_indices()
                .enumerate()
                .filter_map(|(char_offset, (byte_offset, _))| {
                    indices
                        .contains(&(char_offset as u32))
                        .then_some(byte_offset)
                })
                .collect();

            results.push(PathMatch {
                score: score as f64,
                worktree_id,
                positions,
                is_dir: c.is_dir,
                path: c.path.into(),
                path_prefix: Arc::clone(&path_prefix),
                distance_to_relative_ancestor: relative_to
                    .as_ref()
                    .map_or(usize::MAX, |relative_to| {
                        distance_between_paths(c.path, relative_to.as_ref())
                    }),
            })
        };
    }
    Ok(())
}

/// Query should contain spaces if you want it to be matched out of order
/// for example: 'audio Cargo' matching 'audio/Cargo.toml'
pub async fn match_path_sets<'a, Set: PathMatchCandidateSet<'a>>(
    candidate_sets: &'a [Set],
    query: &str,
    relative_to: &Option<Arc<RelPath>>,
    smart_case: bool,
    max_results: usize,
    cancel_flag: &AtomicBool,
    executor: BackgroundExecutor,
) -> Vec<PathMatch> {
    let path_count: usize = candidate_sets.iter().map(|s| s.len()).sum();
    if path_count == 0 {
        return Vec::new();
    }
    dbg!(relative_to);

    let path_style = candidate_sets[0].path_style();

    let query = if path_style.is_windows() {
        query.replace('\\', "/")
    } else {
        query.to_owned()
    };

    // Check if the user is typing a negation
    let pattern_string = if let Some(stripped) = query.strip_prefix('!') {
        // User typed "!", we want negated substring: "!'text"
        format!("!'{}", stripped)
    } else {
        // Normal substring matching: "'text"
        format!("'{}", query)
    };

    let pattern = Pattern::parse(
        &pattern_string,
        if smart_case {
            CaseMatching::Smart
        } else {
            CaseMatching::Ignore
        },
        Normalization::Smart,
    );

    let num_cpus = executor.num_cpus().min(path_count);
    let segment_size = path_count.div_ceil(num_cpus);
    let mut segment_results = (0..num_cpus)
        .map(|_| Vec::with_capacity(max_results))
        .collect::<Vec<_>>();

    let mut config = nucleo::Config::DEFAULT;
    config.set_match_paths();
    let mut matchers = matcher::get_matchers(num_cpus, config);

    // This runs num_cpu parallel searches. Each search is going through all candidate sets
    // Each parallel search goes through one segment of the every candidate set. The segments are
    // not overlapping.
    executor
        .scoped(|scope| {
            for (segment_idx, (results, matcher)) in segment_results
                .iter_mut()
                .zip(matchers.iter_mut())
                .enumerate()
            {
                let relative_to = relative_to.clone();
                let pattern = pattern.clone();
                scope.spawn(async move {
                    let segment_start = segment_idx * segment_size;
                    let segment_end = segment_start + segment_size;

                    let mut tree_start = 0;
                    for candidate_set in candidate_sets {
                        let tree_end = tree_start + candidate_set.len();

                        if tree_start < segment_end && segment_start < tree_end {
                            let start = cmp::max(tree_start, segment_start) - tree_start;
                            let end = cmp::min(tree_end, segment_end) - tree_start;
                            let candidates = candidate_set.candidates(start).take(end - start);

                            let worktree_id = candidate_set.id();
                            if path_match_helper(
                                matcher,
                                &pattern,
                                candidates,
                                worktree_id,
                                &candidate_set.prefix(),
                                &relative_to,
                                path_style,
                                cancel_flag,
                                results,
                            )
                            .is_err()
                            {
                                break;
                            }
                        }
                        if tree_end >= segment_end {
                            break;
                        }
                        tree_start = tree_end;
                    }
                })
            }
        })
        .await;

    if cancel_flag.load(atomic::Ordering::Acquire) {
        return Vec::new();
    }

    matcher::return_matchers(matchers);

    let mut results = segment_results.concat();
    util::truncate_to_bottom_n_sorted(&mut results, max_results);
    for r in &mut results {
        r.positions.sort();
    }

    results
}

/// Compute the distance from a given path to some other path
/// If there is no shared path, returns usize::MAX
fn distance_between_paths(path: &RelPath, relative_to: &RelPath) -> usize {
    let mut path_components = path.components();
    let mut relative_components = relative_to.components();

    while path_components
        .next()
        .zip(relative_components.next())
        .map(|(path_component, relative_component)| path_component == relative_component)
        .unwrap_or_default()
    {}
    path_components.count() + relative_components.count() + 1
}

#[cfg(test)]
mod tests {
    use util::rel_path::RelPath;

    use super::distance_between_paths;

    #[test]
    fn test_distance_between_paths_empty() {
        distance_between_paths(RelPath::empty(), RelPath::empty());
    }
}
