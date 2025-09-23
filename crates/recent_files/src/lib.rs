use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{
    App, AsyncApp, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    Subscription, Task, WeakEntity, Window,
};
use ordered_float::OrderedFloat;
use parking_lot::Mutex;
use picker::{Picker, PickerDelegate, highlighted_match_with_paths::HighlightedMatch};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use ui::{ListItem, prelude::*};
use util::paths::PathExt;
use workspace::{
    self, ModalView, PathList, SerializedWorkspaceLocation, WORKSPACE_DB, Workspace, WorkspaceId,
    with_active_or_new_workspace,
};
use zed_actions::OpenRecentFile;

/// Match strings with order-insensitive word matching.
/// Splits the query into words and ensures all words match somewhere in the candidate,
/// regardless of order.
async fn match_strings_order_insensitive<T>(
    candidates: &[T],
    query: &str,
    smart_case: bool,
    max_results: usize,
    cancel_flag: &std::sync::atomic::AtomicBool,
) -> Vec<StringMatch>
where
    T: std::borrow::Borrow<StringMatchCandidate> + Sync,
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

    // Split query into words and remove empty ones
    let words: Vec<&str> = if query.trim().contains(' ') {
        query.split_whitespace().collect()
    } else {
        // For single words, treat the whole query as one word
        vec![query.trim()]
    };

    if words.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    for candidate in candidates {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let candidate_borrowed = candidate.borrow();
        let candidate_string = &candidate_borrowed.string;
        let candidate_lower = candidate_string.to_lowercase();

        // Check if all words are present in the candidate (case-insensitive)
        let mut all_words_match = true;
        let mut total_score = 0.0;
        let mut all_positions = Vec::new();

        for word in &words {
            let word_lower = if smart_case {
                word.to_string()
            } else {
                word.to_lowercase()
            };

            let found_match = if smart_case {
                candidate_string.contains(word)
            } else {
                candidate_lower.contains(&word_lower)
            };

            if found_match {
                if let Some(byte_pos) = if smart_case {
                    candidate_string.find(word)
                } else {
                    candidate_lower.find(&word_lower)
                } {
                    // Calculate a simple score based on position and word length
                    let word_score = 1.0 / (byte_pos as f64 + 1.0)
                        * (word.len() as f64 / candidate_string.len() as f64);
                    total_score += word_score;

                    if let Some(original_byte_pos) = if smart_case {
                        candidate_string.find(word)
                    } else {
                        candidate_string.to_lowercase().find(&word_lower)
                    } {
                        let word_byte_len = word.as_bytes().len();
                        for i in 0..word_byte_len {
                            let pos = original_byte_pos + i;
                            if pos < candidate_string.len()
                                && candidate_string.is_char_boundary(pos)
                            {
                                all_positions.push(pos);
                            }
                        }
                    }
                }
            } else {
                all_words_match = false;
                break;
            }
        }

        if all_words_match {
            all_positions.sort_unstable();
            all_positions.dedup();

            results.push(StringMatch {
                candidate_id: candidate_borrowed.id,
                score: total_score / words.len() as f64, // Average score across words
                positions: all_positions,
                string: candidate_string.clone(),
            });
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(max_results);
    results
}

static RECENT_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn add_recent_file(path: PathBuf) {
    let mut recent_files = RECENT_FILES.lock();
    recent_files.retain(|p| p != &path);
    recent_files.insert(0, path.clone());
    recent_files.truncate(3000);

    // Save to database asynchronously
    smol::spawn(async move {
        if let Err(e) = WORKSPACE_DB.save_recent_file(&path).await {
            log::error!("Failed to save recent file to database: {:?}", e);
        }
    })
    .detach();
}

/// Expand tilde (~) in path to the user's home directory
fn expand_tilde(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        if path_str.starts_with("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                let mut home_path = PathBuf::from(home);
                home_path.push(&path_str[2..]);
                return home_path;
            }
        }
    }
    path.to_path_buf()
}

/// Find the most recent workspace that contains the given file path.
/// Returns the workspace info if found, None otherwise.
/// The workspaces are already ordered by recency (most recent first).
async fn find_workspace_for_file(
    file_path: &Path,
) -> Option<(WorkspaceId, SerializedWorkspaceLocation, PathList)> {
    let recent_workspaces = WORKSPACE_DB.recent_workspaces_on_disk().await.ok()?;

    // Expand tilde in the file path
    let expanded_file_path = expand_tilde(file_path);
    log::debug!(
        "Looking for workspace containing file: {:?} (expanded from {:?})",
        expanded_file_path,
        file_path
    );

    // Iterate through workspaces in order of recency (most recent first)
    for (workspace_id, location, paths) in recent_workspaces {
        // Only consider local workspaces for now
        if !matches!(location, SerializedWorkspaceLocation::Local) {
            continue;
        }

        log::debug!(
            "Checking workspace {:?} with paths: {:?}",
            workspace_id,
            paths.paths()
        );

        // Check if any of the workspace paths contain the file
        for workspace_path in paths.paths() {
            // Expand tilde in workspace path as well
            let expanded_workspace_path = expand_tilde(workspace_path);

            // Try to canonicalize both paths for robust comparison
            let canonical_workspace = expanded_workspace_path
                .canonicalize()
                .unwrap_or_else(|_| expanded_workspace_path.clone());
            let canonical_file = expanded_file_path
                .canonicalize()
                .unwrap_or_else(|_| expanded_file_path.clone());

            log::debug!(
                "Comparing file {:?} with workspace {:?}",
                canonical_file,
                canonical_workspace
            );

            if canonical_file.starts_with(&canonical_workspace) {
                log::debug!(
                    "Found matching workspace! Opening workspace {:?}",
                    workspace_id
                );
                // Return the first (most recent) workspace that contains this file
                return Some((workspace_id, location, paths));
            }
        }
    }

    log::debug!(
        "No workspace found containing file: {:?}",
        expanded_file_path
    );
    None
}

pub fn init(cx: &mut App) {
    // Load recent files from database on startup
    cx.spawn(|_cx: &mut AsyncApp| async move {
        match WORKSPACE_DB.get_recent_files(3000).await {
            Ok(files) => {
                let mut recent_files = RECENT_FILES.lock();
                recent_files.clear();
                recent_files.extend(files);
            }
            Err(e) => {
                log::error!("Failed to load recent files from database: {:?}", e);
            }
        }
    })
    .detach();

    cx.on_action(|open_recent_file: &OpenRecentFile, cx| {
        let create_new_window = open_recent_file.create_new_window;
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            let Some(recent_files) = workspace.active_modal::<RecentFiles>(cx) else {
                RecentFiles::open(workspace, create_new_window, window, cx);
                return;
            };

            recent_files.update(cx, |recent_files, cx| {
                recent_files
                    .picker
                    .update(cx, |picker, cx| picker.cycle_selection(window, cx))
            });
        });
    });

    cx.observe_new(|_workspace: &mut Workspace, window, cx| {
        let Some(window) = window else { return };
        cx.subscribe_in(
            &cx.entity(),
            window,
            |workspace, _, event, _, cx| match event {
                workspace::Event::ItemAdded { item } => {
                    if let Some(project_path) = item.project_path(cx) {
                        if let Some(abs_path) = workspace
                            .project()
                            .read(cx)
                            .absolute_path(&project_path, cx)
                        {
                            add_recent_file(abs_path);
                        }
                    }
                }
                workspace::Event::ActiveItemChanged => {
                    if let Some(active_item) = workspace.active_item(cx) {
                        if let Some(project_path) = active_item.project_path(cx) {
                            if let Some(abs_path) = workspace
                                .project()
                                .read(cx)
                                .absolute_path(&project_path, cx)
                            {
                                add_recent_file(abs_path);
                            }
                        }
                    }
                }
                _ => {}
            },
        )
        .detach();
    })
    .detach();

    // Start periodic save task
    let executor = cx.background_executor().clone();
    cx.spawn(|_cx: &mut AsyncApp| async move {
        loop {
            // Wait for 5 seconds
            executor.timer(std::time::Duration::from_secs(5)).await;

            // Get current recent files
            let recent_files = {
                let recent_files = RECENT_FILES.lock();
                recent_files.clone()
            };

            // Save all recent files to database
            if let Err(e) = WORKSPACE_DB.clear_recent_files().await {
                log::error!("Failed to clear recent files from database: {:?}", e);
                continue;
            }

            for path in recent_files {
                if let Err(e) = WORKSPACE_DB.save_recent_file(&path).await {
                    log::error!(
                        "Failed to save recent file to database: {:?}, path: {:?}",
                        e,
                        path
                    );
                }
            }
        }
    })
    .detach();
}

struct RecentFiles {
    picker: Entity<Picker<RecentFilesDelegate>>,
    _subscription: Subscription,
}

impl ModalView for RecentFiles {}

impl RecentFiles {
    fn new(delegate: RecentFilesDelegate, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));
        let _subscription = cx.subscribe(&picker, |_, _, _, cx| cx.emit(DismissEvent));
        Self {
            picker,
            _subscription,
        }
    }

    pub fn open(
        workspace: &mut Workspace,
        create_new_window: bool,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let weak = cx.entity().downgrade();
        workspace.toggle_modal(window, cx, |window, cx| {
            let delegate = RecentFilesDelegate::new(weak, create_new_window);
            Self::new(delegate, window, cx)
        })
    }
}

impl EventEmitter<DismissEvent> for RecentFiles {}

impl Focusable for RecentFiles {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for RecentFiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("RecentFiles")
            .w(rems(34.))
            .child(self.picker.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.picker.update(cx, |this, cx| {
                    this.cancel(&Default::default(), window, cx);
                })
            }))
    }
}

struct RecentFilesDelegate {
    workspace: WeakEntity<Workspace>,
    files: Vec<PathBuf>,
    matches: Vec<StringMatch>,
    selected_match_index: usize,
    create_new_window: bool,
}

impl RecentFilesDelegate {
    fn new(workspace: WeakEntity<Workspace>, create_new_window: bool) -> Self {
        Self {
            workspace,
            files: RECENT_FILES.lock().clone(),
            matches: Vec::new(),
            selected_match_index: 0,
            create_new_window,
        }
    }
}

impl EventEmitter<DismissEvent> for RecentFilesDelegate {}

impl PickerDelegate for RecentFilesDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, _: &mut Window, _: &mut App) -> Arc<str> {
        Arc::from("Search recent files...")
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_match_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_match_index = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        _: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> gpui::Task<()> {
        let query = query.trim_start();
        let smart_case = query.chars().any(|c| c.is_uppercase());
        let candidates = self
            .files
            .iter()
            .enumerate()
            .map(|(id, path)| {
                let path_str = path.compact().to_string_lossy().into_owned();
                StringMatchCandidate::new(id, &path_str)
            })
            .collect::<Vec<_>>();

        self.matches = smol::block_on(match_strings_order_insensitive(
            candidates.as_slice(),
            query,
            smart_case,
            100,
            &Default::default(),
        ));
        self.matches.sort_unstable_by_key(|m| m.candidate_id);

        if self.matches.is_empty() {
            self.selected_match_index = 0;
        } else {
            self.selected_match_index = self
                .matches
                .iter()
                .enumerate()
                .max_by_key(|(_, m)| OrderedFloat(m.score))
                .map(|(ix, _)| ix)
                .unwrap_or(0);
        }

        Task::ready(())
    }

    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(hit) = self.matches.get(self.selected_index()) {
            let path = self.files[hit.candidate_id].clone();
            let create_new_window = if self.create_new_window {
                !secondary
            } else {
                secondary
            };

            if let Some(workspace) = self.workspace.upgrade() {
                // Try to find a recent workspace that contains this file
                let workspace_handle = workspace.clone();
                cx.spawn_in(window, async move |_, cx| {
                    if let Some((workspace_id, location, workspace_paths)) =
                        find_workspace_for_file(&path).await
                    {
                        // Found a workspace that contains this file, open that workspace
                        workspace_handle.update_in(cx, |workspace, window, cx| {
                            // Check if we're already in the correct workspace
                            if workspace.database_id() == Some(workspace_id) {
                                // We're already in the right workspace, just open the file
                                workspace
                                    .open_workspace_for_paths(false, vec![path], window, cx)
                                    .detach_and_log_err(cx);
                            } else {
                                // Open the workspace that contains this file
                                match location {
                                    SerializedWorkspaceLocation::Local => {
                                        let paths = workspace_paths.paths().to_vec();
                                        workspace
                                            .open_workspace_for_paths(
                                                create_new_window,
                                                paths,
                                                window,
                                                cx,
                                            )
                                            .detach_and_log_err(cx);
                                    }
                                    SerializedWorkspaceLocation::Remote(_) => {
                                        // For now, fall back to opening the file directly for remote workspaces
                                        workspace
                                            .open_workspace_for_paths(
                                                create_new_window,
                                                vec![path],
                                                window,
                                                cx,
                                            )
                                            .detach_and_log_err(cx);
                                    }
                                }
                            }
                        })
                    } else {
                        // No workspace found, open the file standalone
                        workspace_handle.update_in(cx, |workspace, window, cx| {
                            workspace
                                .open_workspace_for_paths(create_new_window, vec![path], window, cx)
                                .detach_and_log_err(cx);
                        })
                    }
                })
                .detach_and_log_err(cx);
            }
        }
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let hit = self.matches.get(ix)?;
        let path = self.files.get(hit.candidate_id)?;

        let path_str = path.compact().to_string_lossy().to_string();
        let highlighted_match = HighlightedMatch {
            text: path_str,
            highlight_positions: hit.positions.clone(),
            color: ui::Color::Default,
        };

        Some(
            ListItem::new(ix)
                .toggle_state(selected)
                .inset(true)
                .child(highlighted_match.render(window, cx)),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    #[test]
    fn test_workspace_path_matching() {
        // Test the core logic of finding the deepest workspace path
        let workspace_paths = vec![
            PathBuf::from("/Users/dima/Developer"),
            PathBuf::from("/Users/dima/Developer/zed"),
            PathBuf::from("/Users/dima/Developer/zed/docs"),
        ];

        let file_path = PathBuf::from("/Users/dima/Developer/zed/docs/src/configuring-zed.md");

        // Find the deepest matching path
        let mut best_match: Option<(PathBuf, usize)> = None;
        for workspace_path in &workspace_paths {
            if file_path.starts_with(workspace_path) {
                let depth = workspace_path.components().count();
                if best_match
                    .as_ref()
                    .map_or(true, |(_, best_depth)| depth > *best_depth)
                {
                    best_match = Some((workspace_path.clone(), depth));
                }
            }
        }

        // Should match the deepest path: /Users/dima/Developer/zed/docs
        assert_eq!(
            best_match.unwrap().0,
            PathBuf::from("/Users/dima/Developer/zed/docs")
        );
    }

    #[test]
    fn test_workspace_path_matching_prefers_most_recent() {
        // Test that we prefer the most recent workspace that contains the file
        // Simulating the order that recent_workspaces_on_disk() would return
        let workspace_paths_in_recency_order = vec![
            PathBuf::from("/Users/dima/Developer/zed"), // Most recent
            PathBuf::from("/Users/dima/Developer/zed/docs"), // Less recent
        ];

        let file_path = PathBuf::from("/Users/dima/Developer/zed/docs/src/configuring-zed.md");

        // Find the first (most recent) matching workspace
        let mut first_match: Option<PathBuf> = None;
        for workspace_path in &workspace_paths_in_recency_order {
            if file_path.starts_with(workspace_path) {
                first_match = Some(workspace_path.clone());
                break; // Take the first match (most recent)
            }
        }

        // Should match the most recent workspace that contains the file: /Users/dima/Developer/zed
        assert_eq!(
            first_match.unwrap(),
            PathBuf::from("/Users/dima/Developer/zed")
        );
    }

    #[test]
    fn test_tilde_expansion() {
        use super::expand_tilde;

        // Test tilde expansion
        let tilde_path = PathBuf::from("~/Developer/zed");
        let expanded = expand_tilde(&tilde_path);

        // Should expand to absolute path
        assert!(expanded.is_absolute());
        assert!(expanded.to_string_lossy().contains("Developer/zed"));
        assert!(!expanded.to_string_lossy().starts_with("~"));

        // Test non-tilde path remains unchanged
        let abs_path = PathBuf::from("/Users/test/project");
        let unchanged = expand_tilde(&abs_path);
        assert_eq!(unchanged, abs_path);
    }

    #[test]
    fn test_workspace_matching_with_tilde_paths() {
        use super::expand_tilde;

        // Simulate workspace paths (could be absolute)
        let workspace_paths = vec![PathBuf::from("/Users/dima/Developer/zed")];

        // Simulate file path with tilde (as it appears in recent files)
        let file_path_with_tilde = PathBuf::from("~/Developer/zed/docs/src/configuring-zed.md");
        let expanded_file_path = expand_tilde(&file_path_with_tilde);

        // Find matching workspace
        let mut match_found = false;
        for workspace_path in &workspace_paths {
            let expanded_workspace = expand_tilde(workspace_path);
            if expanded_file_path.starts_with(&expanded_workspace) {
                match_found = true;
                break;
            }
        }

        assert!(
            match_found,
            "Should find workspace match after tilde expansion"
        );
    }
}
