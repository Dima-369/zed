// crates/search/src/buffer_search_modal.rs

use collections::HashMap;
use editor::{Anchor as MultiBufferAnchor, Editor, EditorEvent, MultiBufferOffset};
use gpui::{
    Action, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    HighlightStyle, Pixels, Render, SharedString, StyledText, Subscription, Task, WeakEntity,
    Window, actions,
};
use workspace::searchable::SearchableItem;
use language::{Anchor, Buffer, HighlightId, ToOffset as _};
use text::Bias;
use picker::{Picker, PickerDelegate};
use project::search::SearchQuery;
use std::{
    ops::Range,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use ui::{
    Button, ButtonStyle, Color, Divider, IconButton, IconButtonShape, KeyBinding,
    Label, ListItem, ListItemSpacing, Tooltip, prelude::*, rems_from_px,
};
use util::{ResultExt, paths::PathMatcher};
use workspace::{ModalView, Workspace};

use crate::{
    SearchOption, SearchOptions, ToggleCaseSensitive, ToggleIncludeIgnored, ToggleRegex,
    ToggleWholeWord,
};

actions!(buffer_search_modal, [ToggleBufferSearch]);

const MIN_WIDTH_FOR_HORIZONTAL_LAYOUT: Pixels = px(950.);
// Requested 50% width split
const LEFT_PANEL_RATIO: f32 = 0.50;
const VERTICAL_RESULTS_RATIO: f32 = 0.40;
const MAX_PREVIEW_BYTES: usize = 200;
const PREVIEW_DEBOUNCE_MS: u64 = 50;

type AnchorRange = Range<Anchor>;

pub fn init(cx: &mut App) {
    cx.observe_new(BufferSearchModal::register).detach();
}

#[derive(Clone)]
struct LineMatchData {
    line: u32,
    line_label: SharedString,
    preview_text: SharedString,
    // Range in the preview text for the specific match this item represents (for list item highlighting)
    list_match_ranges: Arc<Vec<Range<usize>>>,
    active_match_index_in_list: Option<usize>,
    trim_start: usize,
    syntax_highlights: Option<Arc<Vec<(Range<usize>, HighlightId)>>>,
    // The offset of the match this item specifically represents (for sorting/selection)
    primary_match_offset: usize,
}

// Helper to find safe char boundaries for highlighting
fn find_safe_char_boundaries(text: &str, start: usize, end: usize) -> Option<(usize, usize)> {
    let mut safe_start = start.min(text.len());
    while safe_start > 0 && !text.is_char_boundary(safe_start) {
        safe_start -= 1;
    }

    let mut safe_end = end.min(text.len());
    while safe_end > 0 && !text.is_char_boundary(safe_end) {
        safe_end -= 1;
    }

    if safe_start < safe_end {
        Some((safe_start, safe_end))
    } else {
        None
    }
}

// Truncate preview text with ellipsis
fn truncate_preview(text: &str, max_bytes: usize) -> SharedString {
    let trimmed = text.trim();
    if trimmed.len() <= max_bytes {
        return trimmed.to_string().into();
    }

    let mut end = max_bytes;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }

    let mut result = trimmed[..end].to_string();
    result.push('…');
    result.into()
}

#[inline]
fn preview_content_len(preview_text: &str) -> usize {
    preview_text
        .len()
        .saturating_sub(if preview_text.ends_with('…') {
            '…'.len_utf8()
        } else {
            0
        })
}

pub struct BufferSearchDelegate {
    target_editor: Entity<Editor>,
    target_buffer: Entity<Buffer>,
    search_options: SearchOptions,
    items: Vec<LineMatchData>,
    selected_index: usize,
    initial_cursor_offset: usize,
    search_cancelled: Option<Arc<AtomicBool>>,
    buffer_search_modal: WeakEntity<BufferSearchModal>,
    match_count: usize,
    is_searching: bool,
    current_query: String,
    focus_handle: Option<FocusHandle>,
    regex_error: Option<String>,
    all_matches: Arc<Vec<AnchorRange>>,
}

pub struct BufferSearchModal {
    picker: Entity<Picker<BufferSearchDelegate>>,
    preview_editor: Option<Entity<Editor>>,
    target_buffer: Entity<Buffer>,
    _picker_subscription: Subscription,
    _preview_editor_subscription: Option<Subscription>,
    _preview_debounce_task: Option<Task<()>>,
}

impl ModalView for BufferSearchModal {}

impl EventEmitter<DismissEvent> for BufferSearchModal {}

impl Focusable for BufferSearchModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for BufferSearchModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let preview_editor = self.preview_editor.clone();
        let picker = self.picker.clone();

        let viewport_size = window.viewport_size();
        let use_vertical_layout = viewport_size.width < MIN_WIDTH_FOR_HORIZONTAL_LAYOUT;

        let modal_width = (viewport_size.width * 0.9).min(viewport_size.width);
        let modal_height = (viewport_size.height * 0.8).min(viewport_size.height);

        let border_color = cx.theme().colors().border;

        let results_panel = v_flex()
            .flex_shrink_0()
            .min_h_0()
            .overflow_hidden()
            .child(self.picker.clone());

        let preview_panel = v_flex()
            .id("buffer-search-preview")
            .relative()
            .flex_1()
            .overflow_hidden()
            .bg(cx.theme().colors().editor_background)
            .on_click(move |_, window, cx| {
                window.focus(&picker.focus_handle(cx), cx);
            })
            .when_some(preview_editor, |this, editor| this.child(editor))
            .when(self.preview_editor.is_none(), |this| {
                this.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Label::new("Select a result to preview").color(Color::Muted)),
                )
            });

        let content = if use_vertical_layout {
            let results_height = modal_height * VERTICAL_RESULTS_RATIO;
            v_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(
                    results_panel
                        .h(results_height)
                        .w_full()
                        .border_b_1()
                        .border_color(border_color),
                )
                .child(preview_panel.w_full())
                .into_any_element()
        } else {
            let left_panel_width = modal_width * LEFT_PANEL_RATIO;
            h_flex()
                .w_full()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .child(
                    results_panel
                        .w(left_panel_width)
                        .h_full()
                        .border_r_1()
                        .border_color(border_color),
                )
                .child(preview_panel.h_full())
                .into_any_element()
        };

        div()
            .id("buffer-search-modal")
            .relative()
            .h(modal_height)
            .w(modal_width)
            .child(
                v_flex()
                    .elevation_3(cx)
                    .size_full()
                    .overflow_hidden()
                    .border_1()
                    .border_color(border_color)
                    .child(content),
            )
    }
}

enum BufferSearchHighlights {}

impl BufferSearchModal {
    fn register(workspace: &mut Workspace, _window: Option<&mut Window>, _cx: &mut Context<Workspace>) {
        workspace.register_action(|workspace, _: &ToggleBufferSearch, window, cx| {
            let Some(editor) = workspace.active_item(cx).and_then(|item| item.act_as::<Editor>(cx)) else {
                return;
            };

            let selected_text = editor.update(cx, |editor, cx| {
                let snapshot = editor.buffer().read(cx).snapshot(cx);
                let selection = editor.selections.newest_anchor();
                let range = selection.range();
                if range.start.cmp(&range.end, &snapshot).is_ne() {
                    let text_opt = editor
                        .buffer()
                        .read(cx)
                        .as_singleton()
                        .and_then(|buffer| {
                            let buffer = buffer.read(cx);
                            let start = range.start.text_anchor.to_offset(&buffer);
                            let end = range.end.text_anchor.to_offset(&buffer);
                            Some(buffer.text_for_range(start..end).collect::<String>())
                        });
                    text_opt
                } else {
                    let query = editor.query_suggestion(window, cx);
                    if query.is_empty() {
                        None
                    } else {
                        Some(query)
                    }
                }
            });

            let buffer = editor.read(cx).buffer().read(cx).as_singleton();
            let Some(buffer) = buffer else { return };

            // Capture cursor offset for centering search results
            let singleton_buffer_snapshot = buffer.read(cx).snapshot();
            let cursor_offset = editor.read(cx).selections.newest_anchor().head().text_anchor.to_offset(&singleton_buffer_snapshot);

            let weak_workspace = cx.entity().downgrade();
            workspace.toggle_modal(window, cx, |window, cx| {
                BufferSearchModal::new(weak_workspace, editor, buffer, cursor_offset, selected_text, window, cx)
            });
        });

        workspace.register_action(Self::toggle_case_sensitive);
        workspace.register_action(Self::toggle_whole_word);
        workspace.register_action(Self::toggle_regex);
        workspace.register_action(Self::toggle_include_ignored);
    }

    fn toggle_search_option(
        workspace: &mut Workspace,
        option: SearchOptions,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if let Some(modal) = workspace.active_modal::<Self>(cx) {
            modal.update(cx, |modal, cx| {
                modal.picker.update(cx, |picker, cx| {
                    picker.delegate.toggle_search_option(option);
                    let query = picker.delegate.current_query.clone();
                    picker.set_query(query, window, cx);
                });
            });
        }
    }

    fn toggle_case_sensitive(
        workspace: &mut Workspace,
        _: &ToggleCaseSensitive,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::toggle_search_option(workspace, SearchOptions::CASE_SENSITIVE, window, cx);
    }

    fn toggle_whole_word(
        workspace: &mut Workspace,
        _: &ToggleWholeWord,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::toggle_search_option(workspace, SearchOptions::WHOLE_WORD, window, cx);
    }

    fn toggle_regex(
        workspace: &mut Workspace,
        _: &ToggleRegex,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::toggle_search_option(workspace, SearchOptions::REGEX, window, cx);
    }

    fn toggle_include_ignored(
        workspace: &mut Workspace,
        _: &ToggleIncludeIgnored,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        Self::toggle_search_option(workspace, SearchOptions::INCLUDE_IGNORED, window, cx);
    }

    fn new(
        _workspace: WeakEntity<Workspace>,
        target_editor: Entity<Editor>,
        target_buffer: Entity<Buffer>,
        initial_cursor_offset: usize,
        initial_query: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let weak_self = cx.entity().downgrade();

        let delegate = BufferSearchDelegate {
            target_editor,
            target_buffer: target_buffer.clone(),
            search_options: SearchOptions::NONE,
            items: Vec::new(),
            selected_index: 0,
            initial_cursor_offset,
            search_cancelled: None,
            buffer_search_modal: weak_self,
            match_count: 0,
            is_searching: false,
            current_query: initial_query.clone().unwrap_or_default(),
            focus_handle: None,
            regex_error: None,
            all_matches: Arc::new(Vec::new()),
        };

        let picker = cx.new(|cx| {
            let mut picker = Picker::uniform_list(delegate, window, cx)
                .modal(false)
                .max_height(None)
                .show_scrollbar(true);
            picker.delegate.focus_handle = Some(picker.focus_handle(cx));
            if let Some(q) = initial_query {
                picker.set_query(q, window, cx);
            }
            picker
        });

        let picker_subscription = cx.subscribe_in(&picker, window, Self::on_picker_event);

        Self {
            picker,
            preview_editor: None,
            target_buffer,
            _picker_subscription: picker_subscription,
            _preview_editor_subscription: None,
            _preview_debounce_task: None,
        }
    }

    fn on_picker_event(
        &mut self,
        _picker: &Entity<Picker<BufferSearchDelegate>>,
        _event: &DismissEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DismissEvent);
    }

    fn on_preview_editor_event(
        &mut self,
        _editor: &Entity<Editor>,
        event: &EditorEvent,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        if !matches!(event, EditorEvent::Edited { .. }) {
            return;
        }
    }

    fn navigate_and_highlight_matches(
        editor: &mut Editor,
        match_offset: usize,
        active_match_index: usize,
        match_ranges: &[AnchorRange],
        window: &mut Window,
        cx: &mut Context<Editor>,
    ) {
        let buffer_snapshot = editor.buffer().read(cx).snapshot(cx);
        let point = buffer_snapshot.offset_to_point(MultiBufferOffset(match_offset));
        editor.go_to_singleton_buffer_point(point, window, cx);

        let multi_buffer = editor.buffer().read(cx);
        if let Some(excerpt_id) = multi_buffer.excerpt_ids().first().copied() {
            let multi_buffer_ranges: Vec<_> = match_ranges
                .iter()
                .map(|range| MultiBufferAnchor::range_in_buffer(excerpt_id, range.clone()))
                .collect();
            editor.highlight_background::<BufferSearchHighlights>(
                &multi_buffer_ranges,
                move |index, theme| {
                    if index == &active_match_index {
                        theme.colors().search_active_match_background
                    } else {
                        theme.colors().search_match_background
                    }
                },
                cx,
            );
        }
    }

    fn schedule_preview_update(
        &mut self,
        data: Option<(usize, usize, Arc<Vec<AnchorRange>>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._preview_debounce_task = Some(cx.spawn_in(window, async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(PREVIEW_DEBOUNCE_MS))
                .await;

            this.update_in(cx, |this, window, cx| {
                this.update_preview(data, window, cx);
            })
            .log_err();
        }));
    }

    fn update_preview(
        &mut self,
        data: Option<(usize, usize, Arc<Vec<AnchorRange>>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((match_offset, active_index, match_ranges)) = data else {
            self.preview_editor = None;
            self._preview_editor_subscription = None;
            cx.notify();
            return;
        };

        if let Some(editor) = &self.preview_editor {
            editor.update(cx, |editor, cx| {
                Self::navigate_and_highlight_matches(
                    editor,
                    match_offset,
                    active_index,
                    &match_ranges,
                    window,
                    cx,
                );
            });
            cx.notify();
            return;
        }

        let buffer = self.target_buffer.clone();

        let editor = cx.new(|cx| {
            let mut editor = Editor::for_buffer(buffer.clone(), None, window, cx);
            editor.set_show_gutter(true, cx);
            editor
        });

        editor.update(cx, |editor, cx| {
            Self::navigate_and_highlight_matches(
                editor,
                match_offset,
                active_index,
                &match_ranges,
                window,
                cx,
            );
        });

        self._preview_editor_subscription =
            Some(cx.subscribe_in(&editor, window, Self::on_preview_editor_event));
        self.preview_editor = Some(editor);
        cx.notify();
    }
}

fn build_search_query(query: &str, search_options: SearchOptions) -> Result<SearchQuery, String> {
    if search_options.contains(SearchOptions::REGEX) {
        SearchQuery::regex(
            query,
            search_options.contains(SearchOptions::WHOLE_WORD),
            search_options.contains(SearchOptions::CASE_SENSITIVE),
            search_options.contains(SearchOptions::INCLUDE_IGNORED),
            false,
            PathMatcher::default(),
            PathMatcher::default(),
            false,
            None,
        )
    } else {
        SearchQuery::text(
            query,
            search_options.contains(SearchOptions::WHOLE_WORD),
            search_options.contains(SearchOptions::CASE_SENSITIVE),
            search_options.contains(SearchOptions::INCLUDE_IGNORED),
            PathMatcher::default(),
            PathMatcher::default(),
            false,
            None,
        )
    }
    .map_err(|e| e.to_string())
}

impl BufferSearchDelegate {
    fn toggle_search_option(&mut self, option: SearchOptions) {
        self.search_options.toggle(option);
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        cx: &App,
    ) -> ListItem {
        let item = &self.items[ix];

        let preview_text = &item.preview_text;
        let line_label = &item.line_label;
        let list_match_ranges = &item.list_match_ranges;
        let syntax_highlights = &item.syntax_highlights;

        let preview_str: &str = preview_text.as_ref();

        let is_valid_range = |range: &Range<usize>| -> bool {
            range.start < range.end
                && range.end <= preview_str.len()
                && preview_str.is_char_boundary(range.start)
                && preview_str.is_char_boundary(range.end)
        };

        let syntax_theme = cx.theme().syntax();
        let mut highlights: Vec<(Range<usize>, HighlightStyle)> = syntax_highlights
            .as_ref()
            .map(|sh| {
                sh.iter()
                    .filter_map(|(range, id)| {
                        if !is_valid_range(range) {
                            return None;
                        }
                        id.style(&syntax_theme).map(|style| (range.clone(), style))
                    })
                    .collect()
            })
            .unwrap_or_default();

        for (i, range) in list_match_ranges.iter().enumerate() {
            if !is_valid_range(range) {
                continue;
            }
            let is_active = item.active_match_index_in_list == Some(i);
            let color = if is_active {
                cx.theme().colors().search_active_match_background
            } else {
                cx.theme().colors().search_match_background
            };
            let match_style = HighlightStyle {
                font_weight: Some(gpui::FontWeight::BOLD),
                background_color: Some(color),
                ..Default::default()
            };
            highlights.push((range.clone(), match_style));
        }

        ListItem::new(ix)
            .inset(true)
            .spacing(ListItemSpacing::Sparse)
            .toggle_state(selected)
            .child(
                h_flex()
                    .w_full()
                    .gap_2()
                    .pl(px(8.))
                    .justify_between()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .text_ui_sm(cx)
                            .child(StyledText::new(preview_text).with_highlights(highlights)),
                    )
                    .child(
                        Label::new(line_label.clone())
                            .size(ui::LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
    }
}

impl PickerDelegate for BufferSearchDelegate {
    type ListItem = ListItem;

    fn match_count(&self) -> usize {
        self.items.len()
    }

    fn selected_index(&self) -> usize {
        self.selected_index
    }

    fn set_selected_index(
        &mut self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn selected_index_changed(
        &self,
        ix: usize,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Box<dyn Fn(&mut Window, &mut App) + 'static>> {
        let buffer_search_modal = self.buffer_search_modal.clone();

        let preview_data = if let Some(item) = self.items.get(ix) {
            Some((
                item.primary_match_offset,
                ix,
                self.all_matches.clone(),
            ))
        } else {
            None
        };

        Some(Box::new(move |window, cx| {
            let preview_data_clone = preview_data.clone();
            if let Some(modal) = buffer_search_modal.upgrade() {
                modal.update(cx, |modal, cx| {
                    modal.schedule_preview_update(preview_data_clone, window, cx);
                });
            }
        }))
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search in buffer...".into()
    }

    fn render_editor(
        &self,
        editor: &Entity<Editor>,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Div {
        let search_options = self.search_options;
        let focus_handle = self.focus_handle.clone();

        let render_option_button_fn = |option: SearchOption, cx: &mut Context<Picker<Self>>| {
            let is_active = search_options.contains(option.as_options());
            let action = option.to_toggle_action();
            let label = option.label();
            let fh = focus_handle.clone();
            let options = option.as_options();

            IconButton::new(label, option.icon())
                .on_click(cx.listener(move |picker, _, window, cx| {
                    picker.delegate.toggle_search_option(options);
                    let query = picker.delegate.current_query.clone();
                    picker.set_query(query, window, cx);
                }))
                .style(ButtonStyle::Subtle)
                .shape(IconButtonShape::Square)
                .toggle_state(is_active)
                .when_some(fh, |this, fh| {
                    this.tooltip(move |_window, cx| Tooltip::for_action_in(label, action, &fh, cx))
                })
        };

        v_flex()
            .bg(cx.theme().colors().toolbar_background)
            .child(
                h_flex()
                    .overflow_hidden()
                    .flex_none()
                    .py_2()
                    .px_2()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_32()
                            .h_8()
                            .pl_2()
                            .pr_1()
                            .border_1()
                            .border_color(cx.theme().colors().border)
                            .rounded_md()
                            .child(editor.clone())
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(render_option_button_fn(SearchOption::CaseSensitive, cx))
                                    .child(render_option_button_fn(SearchOption::WholeWord, cx))
                                    .child(render_option_button_fn(SearchOption::Regex, cx)),
                            ),
                    ),
            )
            .child(Divider::horizontal())
    }

    fn update_matches(
        &mut self,
        query: String,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Task<()> {
        self.current_query = query.clone();
        let window_handle = window.window_handle();

        if let Some(prev_cancelled) = self.search_cancelled.take() {
            prev_cancelled.store(true, Ordering::Relaxed);
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        self.search_cancelled = Some(cancelled.clone());

        let search_options = self.search_options;
        let initial_cursor = self.initial_cursor_offset;
        let buffer_snapshot = self.target_buffer.read(cx).snapshot();

        self.is_searching = true;

        if query.is_empty() {
            // Populate with all lines
            return cx.spawn(async move |picker, cx| {
                if cancelled.load(Ordering::Relaxed) { return; }

                let line_count = buffer_snapshot.max_point().row + 1;
                let mut new_items = Vec::with_capacity(line_count as usize);

                for line in 0..line_count {
                    if cancelled.load(Ordering::Relaxed) { return; }

                    let line_start = buffer_snapshot.point_to_offset(language::Point::new(line, 0));
                    let line_len = buffer_snapshot.line_len(line);
                    let line_end = buffer_snapshot.point_to_offset(language::Point::new(line, line_len));

                    let line_text: String = buffer_snapshot.text_for_range(line_start..line_end).collect();
                    let trim_start = line_text.len() - line_text.trim_start().len();

                    let preview_text = truncate_preview(&line_text, MAX_PREVIEW_BYTES);
                    let preview_len = preview_content_len(&preview_text);
                    let preview_str: &str = preview_text.as_ref();

                    let syntax_highlights = {
                        let mut highlights = Vec::new();
                        let mut current_offset = 0;
                        for chunk in buffer_snapshot.chunks(line_start..line_end, true) {
                            let chunk_len = chunk.text.len();
                            if let Some(highlight_id) = chunk.syntax_highlight_id {
                                let abs_start = current_offset;
                                let abs_end = current_offset + chunk_len;
                                let rel_start = abs_start.saturating_sub(trim_start);
                                let rel_end = abs_end.saturating_sub(trim_start);

                                if rel_end > 0 && rel_start < preview_len {
                                    let clamped_start = rel_start.min(preview_len);
                                    let clamped_end = rel_end.min(preview_len);
                                    if let Some((safe_start, safe_end)) = find_safe_char_boundaries(preview_str, clamped_start, clamped_end) {
                                        highlights.push((safe_start..safe_end, highlight_id));
                                    }
                                }
                            }
                            current_offset += chunk_len;
                        }
                        if highlights.is_empty() { None } else { Some(Arc::new(highlights)) }
                    };

                    new_items.push(LineMatchData {
                        line,
                        line_label: (line + 1).to_string().into(),
                        preview_text,
                        list_match_ranges: Arc::new(Vec::new()),
                        active_match_index_in_list: None,
                        trim_start,
                        syntax_highlights,
                        primary_match_offset: line_start,
                    });
                }

                picker.update(cx, |picker, cx| {
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }

                    picker.delegate.match_count = new_items.len();
                    picker.delegate.items = new_items;
                    picker.delegate.is_searching = false;
                    picker.delegate.all_matches = Arc::new(Vec::new());

                    // Set selected index to cursor line
                    let cursor_line = buffer_snapshot.offset_to_point(initial_cursor).row;
                    picker.delegate.selected_index =
                        cursor_line.min(line_count.saturating_sub(1)) as usize;

                    let selected_index = picker.delegate.selected_index;
                    let buffer_search_modal = picker.delegate.buffer_search_modal.clone();
                    let preview_data = picker.delegate.items.get(selected_index).map(|item| {
                        (
                            item.primary_match_offset,
                            selected_index,
                            picker.delegate.all_matches.clone(),
                        )
                    });

                    if let Some(modal) = buffer_search_modal.upgrade() {
                        window_handle.update(cx, |_, window, cx| {
                            modal.update(cx, |modal, cx| {
                                modal.update_preview(preview_data, window, cx);
                            });
                        });
                    }

                    cx.notify();
                })
                .log_err();
            });
        }

        cx.spawn(async move |picker, cx| {
             if cancelled.load(Ordering::Relaxed) {
                return;
            }

            let search_query = match build_search_query(&query, search_options) {
                Ok(q) => {
                    picker.update(cx, |picker, cx| {
                        picker.delegate.regex_error = None;
                        cx.notify();
                    }).log_err();
                    q
                }
                Err(error_message) => {
                    picker.update(cx, |picker, cx| {
                        picker.delegate.regex_error = Some(error_message);
                        picker.delegate.items.clear();
                        picker.delegate.match_count = 0;
                        picker.delegate.is_searching = false;
                        cx.notify();
                    }).log_err();
                    return;
                }
            };

            let matches = cx.background_executor().spawn({
                let snapshot = buffer_snapshot.clone();
                let query = search_query.clone();
                async move {
                    query.search(&snapshot, None).await
                }
            }).await;

            if cancelled.load(Ordering::Relaxed) {
                return;
            }

            let matches = matches;
            let all_match_ranges: Vec<AnchorRange> = matches
                .iter()
                .map(|r| {
                    buffer_snapshot.anchor_at(r.start, Bias::Left)
                        ..buffer_snapshot.anchor_at(r.end, Bias::Right)
                })
                .collect();
            let all_match_ranges = Arc::new(all_match_ranges);

            // Group matches by line to compute preview text once per line
            let mut lines_data: HashMap<u32, Vec<Range<usize>>> = HashMap::default();
            for range in matches.iter() {
                let start_point = buffer_snapshot.offset_to_point(range.start);
                lines_data.entry(start_point.row).or_default().push(range.clone());
            }

            let mut new_items: Vec<LineMatchData> = Vec::with_capacity(matches.len());
            let mut sorted_lines: Vec<u32> = lines_data.keys().cloned().collect();
            sorted_lines.sort();

            for line in sorted_lines {
                if cancelled.load(Ordering::Relaxed) {
                    return;
                }

                if let Some(ranges) = lines_data.remove(&line) {
                    let line_start = buffer_snapshot.point_to_offset(language::Point::new(line, 0));
                    let line_len = buffer_snapshot.line_len(line);
                    let line_end =
                        buffer_snapshot.point_to_offset(language::Point::new(line, line_len));
                    let line_text: String =
                        buffer_snapshot.text_for_range(line_start..line_end).collect();

                    let trim_start = line_text.len() - line_text.trim_start().len();
                    let preview_text = truncate_preview(&line_text, MAX_PREVIEW_BYTES);
                    let preview_len = preview_content_len(&preview_text);
                    let preview_str: &str = preview_text.as_ref();

                    let syntax_highlights = {
                        let mut highlights = Vec::new();
                        let mut current_offset = 0;
                        for chunk in buffer_snapshot.chunks(line_start..line_end, true) {
                            let chunk_len = chunk.text.len();
                            if let Some(highlight_id) = chunk.syntax_highlight_id {
                                let abs_start = current_offset;
                                let abs_end = current_offset + chunk_len;
                                let rel_start = abs_start.saturating_sub(trim_start);
                                let rel_end = abs_end.saturating_sub(trim_start);

                                if rel_end > 0 && rel_start < preview_len {
                                    let clamped_start = rel_start.min(preview_len);
                                    let clamped_end = rel_end.min(preview_len);
                                    if let Some((safe_start, safe_end)) = find_safe_char_boundaries(
                                        preview_str,
                                        clamped_start,
                                        clamped_end,
                                    ) {
                                        highlights.push((safe_start..safe_end, highlight_id));
                                    }
                                }
                            }
                            current_offset += chunk_len;
                        }
                        if highlights.is_empty() {
                            None
                        } else {
                            Some(Arc::new(highlights))
                        }
                    };

                    let mut visible_preview_ranges = Vec::new();
                    let mut original_index_to_list_index = HashMap::default();

                    for (i, range) in ranges.iter().enumerate() {
                        let ms = range.start;
                        let me = range.end;

                        let start_in_line = ms.saturating_sub(line_start);
                        let end_in_line = me.saturating_sub(line_start);

                        let start_in_preview = start_in_line.saturating_sub(trim_start);
                        let end_in_preview = end_in_line.saturating_sub(trim_start);

                        if start_in_preview < preview_len && end_in_preview > 0 {
                            let clamped_start = start_in_preview.min(preview_len);
                            let clamped_end = end_in_preview.min(preview_len);
                            if let Some((safe_start, safe_end)) = find_safe_char_boundaries(
                                preview_str,
                                clamped_start,
                                clamped_end,
                            ) {
                                visible_preview_ranges.push(safe_start..safe_end);
                                original_index_to_list_index
                                    .insert(i, visible_preview_ranges.len() - 1);
                            }
                        }
                    }
                    let list_match_ranges = Arc::new(visible_preview_ranges);

                    // Create an item for each match
                    for (i, range) in ranges.iter().enumerate() {
                        new_items.push(LineMatchData {
                            line,
                            line_label: (line + 1).to_string().into(),
                            preview_text: preview_text.clone(),
                            list_match_ranges: list_match_ranges.clone(),
                            active_match_index_in_list: original_index_to_list_index
                                .get(&i)
                                .copied(),
                            trim_start,
                            syntax_highlights: syntax_highlights.clone(),
                            primary_match_offset: range.start,
                        });
                    }
                }
            }

            picker
                .update(cx, |picker, cx| {
                    if cancelled.load(Ordering::Relaxed) {
                        return;
                    }

                    picker.delegate.match_count = matches.len();
                    picker.delegate.items = new_items;
                    picker.delegate.is_searching = false;
                    picker.delegate.all_matches = all_match_ranges;

                    // Find closest match to initial cursor
                    // For search results, we look at primary_match_offset (absolute offset in buffer)
                    let mut best_index = 0;
                    let mut min_distance = usize::MAX;

                    for (idx, item) in picker.delegate.items.iter().enumerate() {
                        let dist = if item.primary_match_offset >= initial_cursor {
                            item.primary_match_offset - initial_cursor
                        } else {
                            initial_cursor - item.primary_match_offset
                        };

                        if dist < min_distance {
                            min_distance = dist;
                            best_index = idx;
                        }
                    }

                    picker.delegate.selected_index = best_index;

                    let buffer_search_modal = picker.delegate.buffer_search_modal.clone();
                    let preview_data = picker.delegate.items.get(best_index).map(|item| {
                        (
                            item.primary_match_offset,
                            best_index,
                            picker.delegate.all_matches.clone(),
                        )
                    });

                    if let Some(modal) = buffer_search_modal.upgrade() {
                        window_handle.update(cx, |_, window, cx| {
                            modal.update(cx, |modal, cx| {
                                modal.update_preview(preview_data, window, cx);
                            });
                        });
                    }

                    cx.notify();
                })
                .log_err();
        })
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(item) = self.items.get(self.selected_index) {
            let target_editor = self.target_editor.clone();
            let match_offset = item.primary_match_offset;

            // Dismiss modal
            cx.emit(DismissEvent);

            // Move cursor in actual editor
            target_editor.update(cx, |editor, cx| {
                let buffer_snapshot = editor.buffer().read(cx).snapshot(cx);
                let point = buffer_snapshot.offset_to_point(MultiBufferOffset(match_offset));
                editor.go_to_singleton_buffer_point(point, window, cx);
            });
        } else {
             cx.emit(DismissEvent);
        }
    }

    fn dismissed(&mut self, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        cx.emit(DismissEvent);
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        Some(self.render_match(ix, selected, cx))
    }

    fn render_header(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
         if let Some(error) = &self.regex_error {
            return Some(
                h_flex()
                    .w_full()
                    .px_3()
                    .py_1()
                    .child(
                        Label::new(format!("Invalid regex: {}", error))
                            .size(LabelSize::Small)
                            .color(Color::Error),
                    )
                    .into_any(),
            );
        }
        None
    }

    fn render_footer(
        &self,
        _window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        let focus_handle = self.focus_handle.clone()?;

        Some(
            h_flex()
                .w_full()
                .p_1p5()
                .gap_1()
                .justify_end()
                .border_t_1()
                .border_color(cx.theme().colors().border_variant)
                .child(
                    Button::new("go", "Go to Match")
                        .key_binding(
                            KeyBinding::for_action_in(&menu::Confirm, &focus_handle, cx)
                                .map(|kb| kb.size(rems_from_px(12.))),
                        )
                        .on_click(|_, window, cx| {
                            window.dispatch_action(menu::Confirm.boxed_clone(), cx);
                        }),
                )
                .into_any(),
        )
    }
}