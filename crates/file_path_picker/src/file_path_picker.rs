use gpui::{
    App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, Render, Subscription, Task, Window,
};
use picker::{Picker, PickerDelegate};
use project::{Project, ProjectPath};
use std::{collections::HashSet, sync::Arc};
use ui::{prelude::*, ListItem, ListItemSpacing};
use util::paths::PathExt;
use workspace::{ModalView, Workspace};
use zed_actions::workspace::CopyFilePaths;

pub fn init(cx: &mut App) {
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|workspace, _: &CopyFilePaths, window, cx| {
            FilePathPicker::open(workspace, window, cx);
        });
    })
    .detach();
}

#[derive(Debug, Clone)]
pub struct PathEntry {
    pub label: String,
    pub path: String,
    pub description: String,
}

pub struct FilePathPicker {
    picker: Entity<Picker<FilePathPickerDelegate>>,
    _subscription: Subscription,
}

impl ModalView for FilePathPicker {}

impl FilePathPicker {
    fn new(
        delegate: FilePathPickerDelegate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));
        let _subscription = cx.subscribe(&picker, |_, _, _, cx| cx.emit(DismissEvent));
        Self {
            picker,
            _subscription,
        }
    }

    pub fn open(
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let entries = FilePathPickerDelegate::generate_path_entries(workspace, cx);
        workspace.toggle_modal(window, cx, |window, cx| {
            let delegate = FilePathPickerDelegate::new(entries);
            Self::new(delegate, window, cx)
        })
    }
}

impl EventEmitter<DismissEvent> for FilePathPicker {}

impl Focusable for FilePathPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for FilePathPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("FilePathPicker")
            .w(rems(34.))
            .child(self.picker.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.picker.update(cx, |this, cx| {
                    this.cancel(&Default::default(), window, cx);
                })
            }))
    }
}

pub struct FilePathPickerDelegate {
    entries: Vec<PathEntry>,
    selected_index: usize,
}

impl FilePathPickerDelegate {
    fn new(entries: Vec<PathEntry>) -> Self {
        Self {
            entries,
            selected_index: 0,
        }
    }

    fn generate_path_entries(workspace: &Workspace, cx: &mut App) -> Vec<PathEntry> {
        let mut entries = Vec::new();

        if let Some(active_item) = workspace.active_item(cx) {
            if let Some(project_path) = active_item.project_path(cx) {
                let project = workspace.project().clone();

                // Get the absolute path
                if let Some(worktree) = project.read(cx).worktree_for_id(project_path.worktree_id, cx) {
                    let abs_path = worktree.read(cx).abs_path().join(project_path.path.as_std_path());

                    // Collect all potential entries
                    let mut potential_entries = Vec::new();

                    // Entry 1: Filename only
                    if let Some(filename) = abs_path.file_name() {
                        potential_entries.push(PathEntry {
                            label: filename.to_string_lossy().to_string(),
                            path: filename.to_string_lossy().to_string(),
                            description: "Filename only".to_string(),
                        });
                    }

                    // Entry 2: Path from git root
                    if let Some(git_root_relative) = Self::get_git_relative_path(&project, &project_path, cx) {
                        potential_entries.push(PathEntry {
                            label: git_root_relative.clone(),
                            path: git_root_relative.clone(),
                            description: "Path from git root".to_string(),
                        });
                    }

                    // Entry 3: Full path with tilde abbreviation
                    let compact_path = abs_path.compact();
                    potential_entries.push(PathEntry {
                        label: compact_path.to_string_lossy().to_string(),
                        path: compact_path.to_string_lossy().to_string(),
                        description: "Full path (abbreviated)".to_string(),
                    });

                    // Entry 4: Full path without abbreviation
                    potential_entries.push(PathEntry {
                        label: abs_path.to_string_lossy().to_string(),
                        path: abs_path.to_string_lossy().to_string(),
                        description: "Full path".to_string(),
                    });

                    // Remove duplicates, keeping the first occurrence (which prioritizes filename)
                    let mut seen_paths = HashSet::new();
                    for entry in potential_entries {
                        if seen_paths.insert(entry.path.clone()) {
                            entries.push(entry);
                        }
                    }
                }
            }
        }

        if entries.is_empty() {
            entries.push(PathEntry {
                label: "No active file".to_string(),
                path: "".to_string(),
                description: "No file is currently active".to_string(),
            });
        }

        entries
    }

    fn get_git_relative_path(
        project: &Entity<Project>,
        project_path: &ProjectPath,
        cx: &App,
    ) -> Option<String> {
        let git_store = project.read(cx).git_store().read(cx);
        if let Some((repo, _repo_path)) = git_store.repository_and_path_for_project_path(project_path, cx) {
            let repo = repo.read(cx);
            let git_root = &repo.snapshot().work_directory_abs_path;

            // Get the absolute path of the file
            if let Some(worktree) = project.read(cx).worktree_for_id(project_path.worktree_id, cx) {
                let abs_path = worktree.read(cx).abs_path().join(project_path.path.as_std_path());
                if let Ok(relative_path) = abs_path.strip_prefix(git_root) {
                    return Some(relative_path.to_string_lossy().to_string());
                }
            }
        }
        None
    }
}

impl EventEmitter<DismissEvent> for FilePathPickerDelegate {}

impl PickerDelegate for FilePathPickerDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Select a path format to copy...".into()
    }

    fn match_count(&self) -> usize {
        self.entries.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        self.selected_index = ix.min(self.entries.len().saturating_sub(1));
        cx.notify();
    }

    fn update_matches(
        &mut self,
        _query: String,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(entry) = self.entries.get(self.selected_index) {
            cx.write_to_clipboard(ClipboardItem::new_string(entry.path.clone()));
        }
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let entry = self.entries.get(ix)?;
        
        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            v_flex()
                                .child(Label::new(entry.label.clone()).single_line())
                                .child(
                                    Label::new(entry.description.clone())
                                        .size(LabelSize::Small)
                                        .color(Color::Muted)
                                        .single_line()
                                )
                        )
                )
        )
    }
}
