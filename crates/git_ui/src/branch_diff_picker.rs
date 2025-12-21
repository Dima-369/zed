use anyhow::Result;
use buffer_diff::{BufferDiff, BufferDiffSnapshot};
use editor::{Editor, EditorEvent, MultiBuffer};
use fuzzy::StringMatchCandidate;
use git::repository::{Branch, RepoPath};
use gpui::{
    AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    IntoElement, ParentElement, Render, SharedString, Styled, Subscription, Task, WeakEntity,
    Window,
};
use language::{Buffer, Capability};
use picker::{Picker, PickerDelegate};
use project::{git_store::Repository, Project};
use std::sync::Arc;
use ui::{ListItem, ListItemSpacing, prelude::*};
use util::ResultExt;
use workspace::{ItemHandle, ModalView, Workspace};

pub struct BranchDiffPicker {
    pub picker: Entity<Picker<BranchDiffPickerDelegate>>,
    _subscription: Subscription,
}

impl BranchDiffPicker {
    pub fn new(
        branches: Vec<Branch>,
        repo_path: RepoPath,
        buffer: Entity<Buffer>,
        repo: WeakEntity<Repository>,
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let delegate = BranchDiffPickerDelegate::new(
            branches, repo_path, buffer, repo, workspace, project, cx,
        );
        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));

        let _subscription = cx.subscribe(&picker, |_, _, _, cx| {
            cx.emit(DismissEvent);
        });

        Self {
            picker,
            _subscription,
        }
    }
}

impl ModalView for BranchDiffPicker {}
impl EventEmitter<DismissEvent> for BranchDiffPicker {}

impl Focusable for BranchDiffPicker {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for BranchDiffPicker {
    fn render(&mut self, _: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("BranchDiffPicker")
            .w(rems(34.))
            .child(self.picker.clone())
    }
}

pub struct BranchDiffPickerDelegate {
    all_branches: Vec<Branch>,
    matches: Vec<Branch>,
    repo_path: RepoPath,
    buffer: Entity<Buffer>,
    repo: WeakEntity<Repository>,
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    selected_index: usize,
}

impl BranchDiffPickerDelegate {
    fn new(
        branches: Vec<Branch>,
        repo_path: RepoPath,
        buffer: Entity<Buffer>,
        repo: WeakEntity<Repository>,
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        _cx: &mut Context<BranchDiffPicker>,
    ) -> Self {
        // Filter out branches that are likely not useful for diffing (like the current one? 
        // keeping all for now allows diffing against upstream/origin easily).
        Self {
            all_branches: branches.clone(),
            matches: branches,
            repo_path,
            buffer,
            repo,
            workspace,
            project,
            selected_index: 0,
        }
    }

    fn open_diff_view(
        &self,
        branch_name: &str,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) {
        let branch_name = SharedString::from(branch_name.to_string());
        let repo = self.repo.clone();
        let buffer = self.buffer.clone();
        let workspace = self.workspace.clone();
        let project = self.project.clone();
        let path = self.repo_path.clone();

        cx.spawn_in(window, async move |_, cx| {
            let old_text = repo
                .update(cx, |repo, _| {
                    repo.show_file(branch_name.to_string(), path.clone())
                })?
                .await??;

            let new_buffer_snapshot = buffer.read_with(cx, |buffer, _| buffer.snapshot())?;

            let old_buffer = cx.new(|cx| {
                let mut buffer = Buffer::local(old_text.unwrap_or_default(), cx);
                buffer.set_capability(Capability::ReadOnly, cx);
                if let Some(language) = new_buffer_snapshot.language() {
                    buffer.set_language(Some(language.clone()), cx);
                }
                buffer
            })?;

            let old_buffer_snapshot = old_buffer.read_with(cx, |buffer, _| buffer.snapshot())?;

            let diff_snapshot = cx
                .update(|_window, cx| {
                    BufferDiffSnapshot::new_with_base_buffer(
                        new_buffer_snapshot.text.clone(),
                        Some(old_buffer_snapshot.text().into()),
                        old_buffer_snapshot.clone(),
                        cx,
                    )
                })?
                .await;

            let buffer_diff = cx.new(|cx| {
                let mut diff = BufferDiff::new(&new_buffer_snapshot.text, cx);
                diff.set_snapshot(diff_snapshot, &new_buffer_snapshot.text, cx);
                diff
            })?;

            workspace.update_in(cx, |workspace, window, cx| {
                let diff_view = cx.new(|cx| {
                    BranchFileDiffView::new(
                        old_buffer,
                        buffer.clone(),
                        buffer_diff,
                        branch_name.clone(),
                        project.clone(),
                        window,
                        cx,
                    )
                });

                let pane = workspace.active_pane();
                pane.update(cx, |pane, cx| {
                    pane.add_item(Box::new(diff_view), true, true, None, window, cx);
                });
            })?;

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);

        cx.emit(DismissEvent);
    }
}

impl PickerDelegate for BranchDiffPickerDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Select branch to diff against…".into()
    }

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        let all_branches = self.all_branches.clone();

        cx.spawn_in(window, async move |picker, cx| {
            let matches: Vec<Branch> = if query.is_empty() {
                all_branches
            } else {
                let candidates: Vec<StringMatchCandidate> = all_branches
                    .iter()
                    .enumerate()
                    .map(|(ix, branch)| StringMatchCandidate::new(ix, &branch.name()))
                    .collect();

                fuzzy::match_strings(
                    &candidates,
                    &query,
                    true,
                    true,
                    10000,
                    &Default::default(),
                    cx.background_executor().clone(),
                )
                .await
                .into_iter()
                .map(|m| all_branches[m.candidate_id].clone())
                .collect()
            };

            picker
                .update(cx, |picker, _cx| {
                    picker.delegate.matches = matches;
                    if picker.delegate.matches.is_empty() {
                        picker.delegate.selected_index = 0;
                    } else {
                        picker.delegate.selected_index = picker
                            .delegate
                            .selected_index
                            .min(picker.delegate.matches.len().saturating_sub(1));
                    }
                })
                .log_err();
        })
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(branch) = self.matches.get(self.selected_index) {
            self.open_diff_view(&branch.name(), window, cx);
        }
    }

    fn dismissed(&mut self, _: &mut Window, cx: &mut Context<Picker<Self>>) {
        cx.emit(DismissEvent);
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let branch = self.matches.get(ix)?;
        let branch_name: SharedString = branch.name().to_string().into();

        Some(
            ListItem::new(SharedString::from(format!("branch-{ix}")))
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .child(
                    h_flex()
                        .w_full()
                        .gap_2()
                        .child(Icon::new(IconName::GitBranch).size(IconSize::Small).color(Color::Muted))
                        .child(
                            Label::new(branch_name)
                                .color(if selected { Color::Default } else { Color::Muted })
                        )
                ),
        )
    }
}

pub struct BranchFileDiffView {
    editor: Entity<Editor>,
    _old_buffer: Entity<Buffer>,
    new_buffer: Entity<Buffer>,
    branch_name: SharedString,
}

impl BranchFileDiffView {
    pub fn new(
        old_buffer: Entity<Buffer>,
        new_buffer: Entity<Buffer>,
        diff: Entity<BufferDiff>,
        branch_name: SharedString,
        project: Entity<Project>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let multibuffer = cx.new(|cx| {
            let mut multibuffer = MultiBuffer::singleton(new_buffer.clone(), cx);
            multibuffer.add_diff(diff.clone(), cx);
            multibuffer
        });
        let editor = cx.new(|cx| {
            let mut editor =
                Editor::for_multibuffer(multibuffer.clone(), Some(project.clone()), window, cx);
            editor.start_temporary_diff_override();
            editor.disable_diagnostics(cx);
            editor.set_expand_all_diff_hunks(cx);
            editor.set_render_diff_hunk_controls(
                Arc::new(|_, _, _, _, _, _, _, _| gpui::Empty.into_any_element()),
                cx,
            );
            editor
        });

        Self {
            editor,
            _old_buffer: old_buffer,
            new_buffer,
            branch_name,
        }
    }
}

impl EventEmitter<EditorEvent> for BranchFileDiffView {}

impl Focusable for BranchFileDiffView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.editor.focus_handle(cx)
    }
}

impl workspace::Item for BranchFileDiffView {
    type Event = EditorEvent;

    fn tab_icon(&self, _window: &Window, _cx: &App) -> Option<Icon> {
        Some(Icon::new(IconName::Diff).color(Color::Muted))
    }

    fn tab_content(
        &self,
        params: workspace::item::TabContentParams,
        _window: &Window,
        cx: &App,
    ) -> AnyElement {
        Label::new(self.tab_content_text(params.detail.unwrap_or_default(), cx))
            .color(if params.selected {
                Color::Default
            } else {
                Color::Muted
            })
            .into_any_element()
    }

    fn tab_content_text(&self, _detail: usize, cx: &App) -> SharedString {
        let filename = self
            .new_buffer
            .read(cx)
            .file()
            .and_then(|file| {
                Some(
                    file.full_path(cx)
                        .file_name()?
                        .to_string_lossy()
                        .to_string(),
                )
            })
            .unwrap_or_else(|| "untitled".into());

        format!("{filename} @ {}", self.branch_name).into()
    }

    fn tab_tooltip_text(&self, cx: &App) -> Option<SharedString> {
        let path = self
            .new_buffer
            .read(cx)
            .file()
            .map(|file| file.full_path(cx).to_string_lossy().into_owned())
            .unwrap_or_else(|| "untitled".into());

        Some(format!("Diff: {path} vs branch {}", self.branch_name).into())
    }

    fn to_item_events(event: &EditorEvent, f: impl FnMut(workspace::item::ItemEvent)) {
        Editor::to_item_events(event, f)
    }

    fn telemetry_event_text(&self) -> Option<&'static str> {
        Some("Branch Diff View")
    }

    fn deactivated(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.editor
            .update(cx, |editor, cx| editor.deactivated(window, cx));
    }

    fn act_as_type<'a>(
        &'a self,
        type_id: std::any::TypeId,
        self_handle: &'a Entity<Self>,
        _: &'a App,
    ) -> Option<gpui::AnyEntity> {
        if type_id == std::any::TypeId::of::<Self>() {
            Some(self_handle.clone().into())
        } else if type_id == std::any::TypeId::of::<Editor>() {
            Some(self.editor.clone().into())
        } else {
            None
        }
    }

    fn as_searchable(
        &self,
        _: &Entity<Self>,
        _: &App,
    ) -> Option<Box<dyn workspace::searchable::SearchableItemHandle>> {
        Some(Box::new(self.editor.clone()))
    }

    fn for_each_project_item(
        &self,
        cx: &App,
        f: &mut dyn FnMut(gpui::EntityId, &dyn project::ProjectItem),
    ) {
        self.editor.for_each_project_item(cx, f)
    }

    fn set_nav_history(
        &mut self,
        nav_history: workspace::ItemNavHistory,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, _| {
            editor.set_nav_history(Some(nav_history));
        });
    }

    fn navigate(
        &mut self,
        data: Box<dyn std::any::Any>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.editor
            .update(cx, |editor, cx| editor.navigate(data, window, cx))
    }

    fn breadcrumb_location(&self, _: &App) -> workspace::ToolbarItemLocation {
        workspace::ToolbarItemLocation::PrimaryLeft
    }

    fn breadcrumbs(
        &self,
        theme: &theme::Theme,
        cx: &App,
    ) -> Option<Vec<workspace::item::BreadcrumbText>> {
        self.editor.breadcrumbs(theme, cx)
    }

    fn added_to_workspace(
        &mut self,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.editor.update(cx, |editor, cx| {
            editor.added_to_workspace(workspace, window, cx)
        });
    }

    fn can_save(&self, _cx: &App) -> bool {
        false
    }

    fn save(
        &mut self,
        _options: workspace::item::SaveOptions,
        _project: Entity<Project>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn save_as(
        &mut self,
        _project: Entity<Project>,
        _path: project::ProjectPath,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn reload(
        &mut self,
        _project: Entity<Project>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        Task::ready(Ok(()))
    }

    fn is_dirty(&self, _cx: &App) -> bool {
        false
    }

    fn has_conflict(&self, _cx: &App) -> bool {
        false
    }
}

impl Render for BranchFileDiffView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        self.editor.clone()
    }
}