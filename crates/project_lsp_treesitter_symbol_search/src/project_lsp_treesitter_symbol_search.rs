mod providers;
mod symbols;

use std::sync::Arc;

use editor::{Bias, Editor, SelectionEffects, scroll::Autoscroll};
use fuzzy::StringMatch;
use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, IntoElement,
    ParentElement, Render, SharedString, Styled, Task, WeakEntity, Window, actions, rems,
};
use language::ToPoint;
use picker::{Picker, PickerDelegate};
use providers::SearchResult;
use symbols::SymbolProvider;
use ui::{Divider, HighlightedLabel, IconName, ListItem, ListItemSpacing, prelude::*};
use util::ResultExt;
use workspace::{ModalView, Workspace};

actions!(
    project_lsp_treesitter_symbol_search,
    [
        /// Toggle the Search Everywhere modal.
        Toggle,
    ]
);

pub fn init(cx: &mut App) {
    cx.observe_new(ProjectSymbolSearch::register).detach();
}

impl ModalView for ProjectSymbolSearch {}

pub struct ProjectSymbolSearch {
    picker: Entity<Picker<ProjectSymbolSearchDelegate>>,
}

impl ProjectSymbolSearch {
    fn register(
        workspace: &mut Workspace,
        _window: Option<&mut Window>,
        _: &mut Context<Workspace>,
    ) {
        workspace.register_action(|workspace, _: &Toggle, window, cx| {
            Self::toggle(workspace, window, cx);
        });
    }

    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        let Some(previous_focus_handle) = window.focused(cx) else {
            return;
        };

        let project = workspace.project().clone();
        let weak_workspace = cx.entity().downgrade();

        workspace.toggle_modal(window, cx, move |window, cx| {
            ProjectSymbolSearch::new(weak_workspace, project, previous_focus_handle, window, cx)
        });
    }

    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<project::Project>,
        previous_focus_handle: FocusHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak_symbol_search = cx.entity().downgrade();

        let delegate = ProjectSymbolSearchDelegate::new(
            weak_symbol_search,
            workspace,
            project,
            previous_focus_handle,
            window,
            cx,
        );

        // Start indexing symbols every time the modal is opened to ensure results are up to date.
        delegate.symbol_provider.start_indexing(cx);

        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));

        Self { picker }
    }
}

impl EventEmitter<DismissEvent> for ProjectSymbolSearch {}

impl Focusable for ProjectSymbolSearch {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for ProjectSymbolSearch {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("ProjectSymbolSearch")
            .w(rems(34.))
            .child(self.picker.clone())
    }
}

pub struct ProjectSymbolSearchDelegate {
    symbol_search: WeakEntity<ProjectSymbolSearch>,
    workspace: WeakEntity<Workspace>,
    project: Entity<project::Project>,
    previous_focus_handle: FocusHandle,
    matches: Vec<SearchResultMatch>,
    selected_ix: usize,
    is_loading: bool,
    symbol_provider: SymbolProvider,
}

struct SearchResultMatch {
    result: SearchResult,
    string_match: StringMatch,
}

impl ProjectSymbolSearchDelegate {
    fn new(
        symbol_search: WeakEntity<ProjectSymbolSearch>,
        workspace: WeakEntity<Workspace>,
        project: Entity<project::Project>,
        previous_focus_handle: FocusHandle,
        _window: &mut Window,
        _cx: &mut Context<ProjectSymbolSearch>,
    ) -> Self {
        let symbol_provider = SymbolProvider::new(project.clone());

        Self {
            symbol_search,
            workspace,
            project,
            previous_focus_handle,
            matches: Vec::new(),
            selected_ix: 0,
            is_loading: false,
            symbol_provider,
        }
    }
}

impl PickerDelegate for ProjectSymbolSearchDelegate {
    type ListItem = ListItem;

    fn match_count(&self) -> usize {
        self.matches.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_ix
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_ix = ix;
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search project symbols via LSP and Tree-sitter...".into()
    }

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        if self.symbol_provider.is_indexing() {
            let progress = self.symbol_provider.indexing_progress_percent();
            Some(SharedString::from(format!(
                "Indexing project... {}%",
                progress
            )))
        } else if self.is_loading {
            Some("Searching...".into())
        } else {
            Some("No matches".into())
        }
    }

    fn render_editor(
        &self,
        editor: &Entity<Editor>,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Div {
        v_flex()
            .child(
                h_flex()
                    .overflow_hidden()
                    .flex_none()
                    .h_9()
                    .px_2p5()
                    .child(editor.clone()),
            )
            .child(Divider::horizontal())
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        self.is_loading = true;
        cx.notify();

        let symbol_results = self.symbol_provider.search(&query, cx);

        cx.spawn_in(window, async move |picker, cx| {
            let symbol_matches = symbol_results.await;

            let mut all_matches: Vec<SearchResultMatch> = Vec::new();

            for (result, string_match) in symbol_matches {
                all_matches.push(SearchResultMatch {
                    result,
                    string_match,
                });
            }

            all_matches.sort_by(|a, b| {
                b.string_match
                    .score
                    .partial_cmp(&a.string_match.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });

            picker
                .update(cx, |picker, cx| {
                    picker.delegate.matches = all_matches;
                    picker.delegate.selected_ix = 0;
                    picker.delegate.is_loading = false;
                    cx.notify();
                })
                .log_err();
        })
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if self.matches.is_empty() {
            self.dismissed(window, cx);
            return;
        }

        let selected_ix = self.selected_ix;
        if selected_ix >= self.matches.len() {
            self.dismissed(window, cx);
            return;
        }

        let selected_match = &self.matches[selected_ix];

        if let Some(symbol) = &selected_match.result.symbol {
            let symbol = symbol.clone();
            let project = self.project.clone();
            let workspace = self.workspace.clone();
            let symbol_search = self.symbol_search.clone();

            let buffer = project.update(cx, |project, cx| {
                project.open_buffer_for_symbol(&symbol, cx)
            });

            cx.spawn_in(window, async move |_, cx| {
                let buffer = buffer.await?;
                workspace.update_in(cx, |workspace, window, cx| {
                    let position = buffer
                        .read(cx)
                        .clip_point_utf16(symbol.range.start, Bias::Left);

                    let editor = workspace.open_project_item::<Editor>(
                        workspace.active_pane().clone(),
                        buffer,
                        true,
                        true,
                        true,
                        true,
                        window,
                        cx,
                    );

                    editor.update(cx, |editor, cx| {
                        editor.change_selections(
                            SelectionEffects::scroll(Autoscroll::center()),
                            window,
                            cx,
                            |s| s.select_ranges([position..position]),
                        );
                    });
                })?;
                symbol_search
                    .update(cx, |_, cx| cx.emit(DismissEvent))
                    .log_err();
                anyhow::Ok(())
            })
            .detach_and_log_err(cx);
        } else if let Some(doc_symbol) = &selected_match.result.document_symbol {
            let buffer = doc_symbol.buffer.clone();
            let range = doc_symbol.range.clone();
            let workspace = self.workspace.clone();
            let symbol_search = self.symbol_search.clone();

            if let Some(workspace) = workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    // Convert anchor to point using buffer snapshot
                    let point = {
                        let buffer_snapshot = buffer.read(cx).snapshot();
                        range.start.to_point(&buffer_snapshot)
                    };

                    let editor = workspace.open_project_item::<Editor>(
                        workspace.active_pane().clone(),
                        buffer,
                        true,
                        true,
                        true,
                        true,
                        window,
                        cx,
                    );

                    editor.update(cx, |editor, cx| {
                        editor.change_selections(
                            SelectionEffects::scroll(Autoscroll::center()),
                            window,
                            cx,
                            |s| s.select_ranges([point..point]),
                        );
                    });
                });
            }

            symbol_search
                .update(cx, |_, cx| cx.emit(DismissEvent))
                .log_err();
        }
    }

    fn dismissed(&mut self, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        window.focus(&self.previous_focus_handle, cx);
        self.symbol_search
            .update(cx, |_, cx| cx.emit(DismissEvent))
            .log_err();
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let search_match = self.matches.get(ix)?;
        let result = &search_match.result;

        let highlights = search_match.string_match.positions.clone();
        let icon = IconName::Code;

        let (label_element, detail_element) = if result.document_symbol.is_some() {
            // Outline: Matched on `name` (label). Detail is `path` (no match).
            (
                HighlightedLabel::new(result.label.clone(), highlights).into_any_element(),
                result.detail.as_ref().map(|detail| {
                    Label::new(detail.clone())
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .into_any_element()
                }),
            )
        } else {
            // Workspace: Matched on `filter_text` (detail). Label is `text` (no match).
            (
                Label::new(result.label.clone()).into_any_element(),
                result.detail.as_ref().map(|detail| {
                    HighlightedLabel::new(detail.clone(), highlights)
                        .size(LabelSize::Small)
                        .color(Color::Muted)
                        .into_any_element()
                }),
            )
        };

        Some(
            ListItem::new(ix)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .toggle_state(selected)
                .start_slot(Icon::new(icon).color(Color::Muted))
                .child(
                    h_flex()
                        .gap_2()
                        .child(label_element)
                        .children(detail_element),
                ),
        )
    }
}
