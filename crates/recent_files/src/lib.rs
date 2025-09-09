use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    Subscription, Task, WeakEntity, Window,
};
use ordered_float::OrderedFloat;
use parking_lot::Mutex;
use picker::{
    highlighted_match_with_paths::{HighlightedMatch},
    Picker, PickerDelegate,
};
use std::path::PathBuf;
use std::sync::Arc;
use ui::{prelude::*, ListItem};
use util::paths::PathExt;
use workspace::{self, with_active_or_new_workspace, ModalView, Workspace};
use zed_actions::OpenRecentFile;

static RECENT_FILES: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

fn add_recent_file(path: PathBuf) {
    let mut recent_files = RECENT_FILES.lock();
    recent_files.retain(|p| p != &path);
    recent_files.insert(0, path);
    recent_files.truncate(3000);
}

pub fn init(cx: &mut App) {
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
        cx.subscribe_in(&cx.entity(), window, |workspace, _, event, _, cx| {
            match event {
                workspace::Event::ItemAdded { item } => {
                    if let Some(project_path) = item.project_path(cx) {
                        if let Some(abs_path) =
                            workspace.project().read(cx).absolute_path(&project_path, cx)
                        {
                            add_recent_file(abs_path);
                        }
                    }
                }
                workspace::Event::ActiveItemChanged => {
                    if let Some(active_item) = workspace.active_item(cx) {
                        if let Some(project_path) = active_item.project_path(cx) {
                            if let Some(abs_path) =
                                workspace.project().read(cx).absolute_path(&project_path, cx)
                            {
                                add_recent_file(abs_path);
                            }
                        }
                    }
                }
                _ => {}
            }
        })
        .detach();
    })
    .detach();
}

struct RecentFiles {
    picker: Entity<Picker<RecentFilesDelegate>>,
    _subscription: Subscription,
}

impl ModalView for RecentFiles {}

impl RecentFiles {
    fn new(
        delegate: RecentFilesDelegate,
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
        cx: &mut Context<Picker<Self>>,
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

        self.matches = smol::block_on(fuzzy::match_strings(
            candidates.as_slice(),
            query,
            smart_case,
            true,
            100,
            &Default::default(),
            cx.background_executor().clone(),
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
                workspace.update(cx, |workspace, cx| {
                    workspace
                        .open_workspace_for_paths(create_new_window, vec![path], window, cx)
                        .detach_and_log_err(cx);
                });
            }
        }
        cx.emit(DismissEvent);
    }

    fn dismissed(&mut self, _window: &mut Window, _: &mut Context<Picker<Self>>) {}

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
            char_count: path.compact().as_os_str().to_string_lossy().chars().count(),
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
