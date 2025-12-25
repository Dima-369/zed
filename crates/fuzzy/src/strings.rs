use crate::{CharBag, matcher};
use gpui::BackgroundExecutor;
use nucleo::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use std::{
    borrow::Borrow,
    cmp::{self, Ordering},
    iter,
    ops::Range,
    sync::atomic::{self, AtomicBool},
};

#[derive(Clone, Debug)]
pub struct StringMatchCandidate {
    pub id: usize,
    pub string: String,
    pub char_bag: CharBag,
}

impl StringMatchCandidate {
    pub fn new(id: usize, string: &str) -> Self {
        Self {
            id,
            string: string.into(),
            char_bag: string.into(),
        }
    }
}


#[derive(Clone, Debug)]
pub struct StringMatch {
    pub candidate_id: usize,
    pub score: f64,
    pub positions: Vec<usize>,
    pub string: String,
}

impl StringMatch {
    pub fn ranges(&self) -> impl '_ + Iterator<Item = Range<usize>> {
        let mut positions = self.positions.iter().peekable();
        iter::from_fn(move || {
            if let Some(start) = positions.next().copied() {
                let Some(char_len) = self.char_len_at_index(start) else {
                    log::error!(
                        "Invariant violation: Index {start} out of range or not on a utf-8 boundary in string {:?}",
                        self.string
                    );
                    return None;
                };
                let mut end = start + char_len;
                while let Some(next_start) = positions.peek() {
                    if end == **next_start {
                        let Some(char_len) = self.char_len_at_index(end) else {
                            log::error!(
                                "Invariant violation: Index {end} out of range or not on a utf-8 boundary in string {:?}",
                                self.string
                            );
                            return None;
                        };
                        end += char_len;
                        positions.next();
                    } else {
                        break;
                    }
                }

                return Some(start..end);
            }
            None
        })
    }

    /// Gets the byte length of the utf-8 character at a byte offset. If the index is out of range
    /// or not on a utf-8 boundary then None is returned.
    fn char_len_at_index(&self, ix: usize) -> Option<usize> {
        self.string
            .get(ix..)
            .and_then(|slice| slice.chars().next().map(|char| char.len_utf8()))
    }
}

impl PartialEq for StringMatch {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for StringMatch {}

impl PartialOrd for StringMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StringMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .partial_cmp(&other.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| self.candidate_id.cmp(&other.candidate_id))
    }
}

pub async fn match_strings<T>(
    candidates: &[T],
    query: &str,
    smart_case: bool,
    penalize_length: bool,
    max_results: usize,
    cancel_flag: &AtomicBool,
    executor: BackgroundExecutor,
) -> Vec<StringMatch>
where
    T: Borrow<StringMatchCandidate> + Sync,
{
    if candidates.is_empty() || max_results == 0 {
        return Default::default();
    }

    if query.is_empty() {
        return candidates
            .iter()
            .map(|candidate| StringMatch {
                candidate_id: candidate.borrow().id,
                score: 0.,
                positions: Default::default(),
                string: candidate.borrow().string.clone(),
            })
            .collect();
    }

    let config = nucleo::Config::DEFAULT;
    // Note: penalize_length is not used in nucleo implementation
    let _ = penalize_length; // Suppress unused variable warning
    let mut matchers = matcher::get_matchers(executor.num_cpus().min(candidates.len()), config);

    // Check if the user is typing a negation
    let pattern_string = if let Some(stripped) = query.strip_prefix('!') {
        // User typed "!", we want negated substring: "!'text"
        format!("!'{}", stripped)
    } else {
        // Normal substring matching: "'text"
        format!("'{}", query)
    };

    let pattern = Pattern::new(
        &pattern_string,
        if smart_case {
            CaseMatching::Smart
        } else {
            CaseMatching::Ignore
        },
        Normalization::Smart,
        AtomKind::Substring,
    );

    let segment_size = candidates.len().div_ceil(matchers.len());
    let mut segment_results = (0..matchers.len())
        .map(|_| Vec::<StringMatch>::with_capacity(max_results.min(candidates.len())))
        .collect::<Vec<_>>();

    executor
        .scoped(|scope| {
            for (segment_idx, (results, matcher)) in segment_results
                .iter_mut()
                .zip(matchers.iter_mut())
                .enumerate()
            {
                let cancel_flag = &cancel_flag;
                let pattern = pattern.clone();
                scope.spawn(async move {
                    let segment_start = cmp::min(segment_idx * segment_size, candidates.len());
                    let segment_end = cmp::min(segment_start + segment_size, candidates.len());

                    for candidate in &candidates[segment_start..segment_end] {
                        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
                            break;
                        }
                        let candidate = candidate.borrow();
                        let mut indices = Vec::new();
                        let mut buf = Vec::new();
                        if let Some(score) = pattern.indices(
                            nucleo::Utf32Str::new(&candidate.string, &mut buf),
                            matcher,
                            &mut indices,
                        ) {
                            // Convert char indices to byte indices
                            let positions: Vec<_> = candidate
                                .string
                                .char_indices()
                                .enumerate()
                                .filter_map(|(char_offset, (byte_offset, _))| {
                                    indices
                                        .contains(&(char_offset as u32))
                                        .then_some(byte_offset)
                                })
                                .collect();

                            results.push(StringMatch {
                                candidate_id: candidate.id,
                                score: score as f64,
                                positions,
                                string: candidate.string.clone(),
                            });
                        }
                    }
                });
            }
        })
        .await;

    if cancel_flag.load(atomic::Ordering::Acquire) {
        matcher::return_matchers(matchers);
        return Vec::new();
    }

    matcher::return_matchers(matchers);

    let mut results = segment_results.concat();
    util::truncate_to_bottom_n_sorted_by(&mut results, max_results, &|a, b| b.cmp(a));
    results
}
