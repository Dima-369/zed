mod jump_settings;

use editor::{
    DisplayPoint, Editor, EditorEvent, JumpLabel, MultiBufferOffset, ToPoint,
    display_map::ToDisplayPoint,
};
use gpui::{
    Action, App, Context, DismissEvent, Entity, EventEmitter, Focusable, IntoElement, Render,
    Styled, Window, div,
};
use jump_settings::JumpSettings;
use schemars::JsonSchema;
use serde::Deserialize;
use settings::Settings;
use std::collections::HashSet;
use ui::{IconButton, IconName, Tooltip, prelude::*};
use workspace::{DismissDecision, ModalView, Workspace};

#[derive(PartialEq, Clone, Deserialize, JsonSchema, Debug, Action)]
#[action(namespace = jump)]
#[serde(deny_unknown_fields)]
pub struct Toggle {
    #[serde(default = "util::serde::default_true")]
    pub focus: bool,
}

impl Toggle {
    pub fn default() -> Self {
        Self { focus: true }
    }
}

pub enum Event {
    UpdateLocation,
    Dismiss,
}

pub fn init(cx: &mut App) {
    JumpSettings::register(cx);
    cx.observe_new(JumpBar::register).detach();
}

#[derive(Debug, Clone)]
struct JumpMatch {
    position: DisplayPoint,
    label: String,
    distance: u32,
    editor: Entity<Editor>,
    next_char: Option<char>,
}

pub struct JumpBar {
    query_editor: Entity<Editor>,
    query_editor_focused: bool,
    active_editor: Option<Entity<Editor>>,
    visible_editors: Vec<Entity<Editor>>,
    workspace: Entity<Workspace>,
    search_query: String,
    previous_query_length: usize,
    matches: Vec<JumpMatch>,
}

impl JumpBar {
    fn register(
        workspace: &mut Workspace,
        _window: Option<&mut Window>,
        _: &mut Context<Workspace>,
    ) {
        workspace.register_action(|workspace, _: &Toggle, window, cx| {
            Self::toggle(workspace, window, cx)
        });
    }

    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        let workspace_handle = cx.entity();

        // Collect all visible editors from all panes
        let mut visible_editors = Vec::new();
        let active_editor = workspace.active_pane().read(cx).active_item();
        let active_editor_entity = active_editor
            .and_then(|item| (&*item as &dyn workspace::item::ItemHandle).downcast::<Editor>());

        // Get editors from all panes
        for pane in workspace.panes() {
            for item in pane.read(cx).items() {
                if let Some(editor) =
                    (&**item as &dyn workspace::item::ItemHandle).downcast::<Editor>()
                {
                    if !visible_editors
                        .iter()
                        .any(|e: &Entity<Editor>| e.entity_id() == editor.entity_id())
                    {
                        visible_editors.push(editor);
                    }
                }
            }
        }

        workspace.toggle_modal(window, cx, |window, cx| {
            JumpBar::new(
                workspace_handle,
                active_editor_entity,
                visible_editors,
                window,
                cx,
            )
        });
    }

    pub fn new(
        workspace: Entity<Workspace>,
        active_editor: Option<Entity<Editor>>,
        visible_editors: Vec<Entity<Editor>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let query_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_placeholder_text("Jump to…", window, cx);
            editor.set_use_autoclose(false);
            editor
        });
        cx.subscribe_in(&query_editor, window, Self::on_query_editor_event)
            .detach();

        cx.focus_view(&query_editor, window);

        Self {
            query_editor,
            query_editor_focused: false,
            workspace,
            active_editor,
            visible_editors,
            search_query: String::new(),
            previous_query_length: 0,
            matches: Vec::new(),
        }
    }

    fn generate_labels(count: usize, next_chars: &HashSet<char>) -> Vec<String> {
        if count == 0 {
            return Vec::new();
        }

        // my custom Dvorak Programmer keyboard layout
        // first home row, then top row, then bottom row without pinky finger keys (s and l)
        let lowercase_priority = "htndueoifgcrypzbmwvxkjq";
        let priority_chars: Vec<char> = lowercase_priority.chars().collect();

        // Filter out forbidden characters (case-insensitive comparison)
        let available: Vec<char> = priority_chars
            .into_iter()
            .filter(|c| !next_chars.contains(&c.to_lowercase().next().unwrap()))
            .collect();

        let mut labels = Vec::new();

        for &ch in &available {
            if labels.len() >= count {
                break;
            }
            labels.push(ch.to_string());
        }

        // If we don't have enough labels, that's okay - user needs to filter more
        labels
    }

    fn on_query_editor_event(
        &mut self,
        _editor: &Entity<Editor>,
        event: &EditorEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorEvent::BufferEdited => {
                let query = self.query_editor.read(cx).text(cx);

                // Check if the query ends with a label (allow typing search + label)
                if query.len() > self.previous_query_length
                    && !query.is_empty()
                    && !self.matches.is_empty()
                {
                    for jump_match in &self.matches {
                        if !jump_match.label.is_empty() {
                            if query.ends_with(&jump_match.label) {
                                let position = jump_match.position;
                                let target_editor = jump_match.editor.clone();
                                self.jump_to_position(position, target_editor, window, cx);
                                self.previous_query_length = query.len();
                                return;
                            }
                        }
                    }
                }

                self.previous_query_length = query.len();
                self.update_search(window, cx);
            }
            EditorEvent::Focused => {
                self.query_editor_focused = true;
            }
            EditorEvent::Blurred => {
                self.query_editor_focused = false;
            }
            _ => {}
        }
    }

    fn jump_to_position(
        &mut self,
        position: DisplayPoint,
        target_editor: Entity<Editor>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Activate the target editor's pane if it's not already active
        self.workspace.update(cx, |workspace, cx| {
            for pane in workspace.panes() {
                let target_id = target_editor.entity_id();
                if pane.read(cx).items().any(|item| {
                    if let Some(editor) =
                        (&**item as &dyn workspace::item::ItemHandle).downcast::<Editor>()
                    {
                        editor.entity_id() == target_id
                    } else {
                        false
                    }
                }) {
                    // Activate this pane and the editor item
                    let item_index = {
                        let pane_read = pane.read(cx);
                        pane_read.items().position(|item| {
                            if let Some(editor) =
                                (&**item as &dyn workspace::item::ItemHandle).downcast::<Editor>()
                            {
                                editor.entity_id() == target_id
                            } else {
                                false
                            }
                        })
                    };

                    if let Some(item_index) = item_index {
                        pane.update(cx, |pane, cx| {
                            pane.activate_item(item_index, true, true, window, cx);
                        });
                    }
                    window.focus(&pane.focus_handle(cx));
                    break;
                }
            }
        });

        // Move cursor in the target editor
        target_editor.update(cx, |editor, cx| {
            editor.change_selections(editor::SelectionEffects::default(), window, cx, |s| {
                s.select_display_ranges(vec![position..position]);
            });
        });

        // Clear query and dismiss
        self.query_editor.update(cx, |editor, cx| {
            editor.clear(window, cx);
        });
        cx.emit(gpui::DismissEvent);
    }

    fn update_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.query_editor.read(cx).text(cx);
        self.search_query = query.clone();
        let autojump = JumpSettings::get_global(cx).autojump;

        if query.is_empty() {
            // Clear all editors
            for editor in &self.visible_editors {
                editor.update(cx, |editor, cx| {
                    editor.set_jump_labels(Vec::new(), cx);
                });
            }
            self.matches.clear();
            cx.notify();
            return;
        }

        // Get active editor cursor position for distance calculation
        let active_cursor_info = self.active_editor.as_ref().map(|editor_entity| {
            let editor_id = editor_entity.entity_id();
            let cursor_point = editor_entity.update(cx, |editor, cx| {
                let snapshot = editor.snapshot(window, cx);
                let display_snapshot = snapshot.display_snapshot;
                let cursor_anchor = editor.selections.newest_anchor().head();
                cursor_anchor.to_display_point(&display_snapshot)
            });
            (cursor_point, editor_id)
        });

        let (active_cursor_point, active_editor_id) = match active_cursor_info {
            Some((point, id)) => (point, Some(id)),
            None => {
                // No active editor, use first visible editor's cursor
                if let Some(first_editor) = self.visible_editors.first() {
                    let editor_id = first_editor.entity_id();
                    let point = first_editor.update(cx, |editor, cx| {
                        let snapshot = editor.snapshot(window, cx);
                        let display_snapshot = snapshot.display_snapshot;
                        let cursor_anchor = editor.selections.newest_anchor().head();
                        cursor_anchor.to_display_point(&display_snapshot)
                    });
                    (point, Some(editor_id))
                } else {
                    return; // No editors at all
                }
            }
        };

        let mut all_matches = Vec::new();
        let query_len = query.len();

        // Search each visible editor
        for editor_entity in &self.visible_editors {
            let is_active = active_editor_id
                .map(|id| id == editor_entity.entity_id())
                .unwrap_or(false);
            let editor_distance_penalty = if is_active { 0 } else { 100000 };

            let editor_matches = editor_entity.update(cx, |editor, cx| {
                let snapshot = editor.snapshot(window, cx);
                let display_snapshot = snapshot.display_snapshot;

                // Get the visible range
                let visible_line_count = editor.visible_line_count().unwrap_or(50.0);
                let scroll_position = editor
                    .scroll_manager
                    .anchor()
                    .scroll_position(&display_snapshot);

                let visible_start_row = scroll_position.y as u32;
                let visible_end_row = visible_start_row + visible_line_count.ceil() as u32;

                let buffer = editor.buffer().read(cx);
                let buffer_snapshot = buffer.snapshot(cx);

                // Convert visible display rows to buffer positions
                let visible_start_point = display_snapshot
                    .buffer_snapshot()
                    .anchor_before(language::Point::new(visible_start_row, 0))
                    .to_point(&buffer_snapshot);

                let visible_end_point = display_snapshot
                    .buffer_snapshot()
                    .anchor_before(language::Point::new(visible_end_row, 0))
                    .to_point(&buffer_snapshot);

                // Get start and end offsets for the visible range
                let start_offset = buffer_snapshot.point_to_offset(visible_start_point);
                let end_offset = buffer_snapshot.point_to_offset(visible_end_point);

                let mut matches = Vec::new();

                let text = buffer_snapshot.text();
                let query_str = query.as_str();

                if query_len == 0 {
                    return matches;
                }

                let query_first = query_str.chars().next().unwrap();
                let query_first_lower = query_first.to_ascii_lowercase();
                let query_first_upper = query_first.to_ascii_uppercase();

                let bytes = text.as_bytes();

                // Only search within the visible range
                for offset_usize in start_offset.0..end_offset.0 {
                    let offset = MultiBufferOffset(offset_usize);
                    // Skip if remaining text is shorter than query
                    if offset + query_len > end_offset {
                        break;
                    }

                    // Check first character quickly to skip most positions
                    let c = bytes[offset_usize] as char;
                    if c != query_first_lower && c != query_first_upper {
                        continue;
                    }

                    // Extract slice safely and compare case-insensitively
                    if !text.is_char_boundary(offset_usize)
                        || !text.is_char_boundary(offset_usize + query_len)
                    {
                        continue;
                    }

                    let slice = &text[offset_usize..offset_usize + query_len];
                    if slice.eq_ignore_ascii_case(query_str) {
                        let point = buffer_snapshot.offset_to_point(offset);
                        let display_point = display_snapshot
                            .buffer_snapshot()
                            .anchor_after(point)
                            .to_display_point(&display_snapshot);

                        let dy = (display_point.row().0 as i32
                            - active_cursor_point.row().0 as i32)
                            .unsigned_abs();
                        let dx = (display_point.column() as i32
                            - active_cursor_point.column() as i32)
                            .unsigned_abs();
                        let distance = dy * 1000 + dx;

                        // Get the next character after the match
                        let next_char = if offset + query_len < buffer_snapshot.len() {
                            let next_offset_usize = offset_usize + query_len;
                            if text.is_char_boundary(next_offset_usize) {
                                text[next_offset_usize..].chars().next()
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        matches.push((display_point, distance, next_char));
                    }
                }

                matches
            });

            // Add matches from this editor with distance penalty
            for (position, distance, next_char) in editor_matches {
                all_matches.push(JumpMatch {
                    position,
                    label: String::new(),
                    distance: distance + editor_distance_penalty,
                    editor: editor_entity.clone(),
                    next_char,
                });
            }
        }

        // Sort all matches globally by distance
        all_matches.sort_by_key(|m| m.distance);

        // Collect next characters for label generation (forbidden chars)
        let next_chars: HashSet<char> = all_matches
            .iter()
            .filter_map(|m| m.next_char)
            .flat_map(|c| vec![c.to_ascii_lowercase(), c.to_ascii_uppercase()])
            .collect();

        // Generate labels globally
        let match_count = all_matches.len();
        let labels = Self::generate_labels(match_count, &next_chars);

        // Assign labels
        for (match_item, label) in all_matches.iter_mut().zip(labels.iter()) {
            match_item.label = label.clone();
        }

        // Group matches by editor and set labels
        for editor_entity in &self.visible_editors {
            let editor_labels: Vec<JumpLabel> = all_matches
                .iter()
                .filter(|m| {
                    m.editor.entity_id() == editor_entity.entity_id() && !m.label.is_empty()
                })
                .map(|m| JumpLabel {
                    position: m.position,
                    label: m.label.clone(),
                    match_length: query_len,
                })
                .collect();

            editor_entity.update(cx, |editor, cx| {
                editor.set_jump_labels(editor_labels, cx);
            });
        }

        self.matches = all_matches;

        // Autojump if enabled and exactly one match
        if autojump && self.matches.len() == 1 {
            if let Some(jump_match) = self.matches.first() {
                let position = jump_match.position;
                let target_editor = jump_match.editor.clone();
                self.jump_to_position(position, target_editor, window, cx);
                return;
            }
        }

        cx.notify();
    }
}

impl ModalView for JumpBar {
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> DismissDecision {
        // Clear jump labels from all visible editors
        for editor in &self.visible_editors {
            editor.update(cx, |editor, cx| {
                editor.set_jump_labels(Vec::new(), cx);
            });
        }
        DismissDecision::Dismiss(true)
    }
}

impl EventEmitter<DismissEvent> for JumpBar {}
impl EventEmitter<Event> for JumpBar {}

impl Render for JumpBar {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex().key_context("JumpBar").w(rems(14.)).child(
            div()
                .id("jump_bar")
                .flex()
                .items_center()
                .gap_2()
                .px_3()
                .py_2()
                .border_1()
                .border_color(cx.theme().colors().border)
                .rounded_md()
                .bg(cx.theme().colors().editor_background.opacity(0.5))
                .shadow_lg()
                .min_w_64()
                .max_w_96()
                .on_action(cx.listener(|_this, _: &Toggle, _window, cx| {
                    cx.emit(DismissEvent);
                }))
                .child(self.query_editor.clone())
                .child(
                    IconButton::new("close", IconName::Close)
                        .tooltip(Tooltip::text("Close (Escape)"))
                        .on_click(cx.listener(|_this, _, _window, cx| {
                            cx.emit(DismissEvent);
                        })),
                ),
        )
    }
}

impl Focusable for JumpBar {
    fn focus_handle(&self, cx: &App) -> gpui::FocusHandle {
        self.query_editor.focus_handle(cx)
    }
}
