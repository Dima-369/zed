use collections::VecDeque;
use gpui::{
    actions, App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, Global, KeyBinding, KeyContext, Render, SharedString, Subscription, UpdateGlobal,
    WeakEntity, Window,
};
use picker::{Picker, PickerDelegate};
use std::sync::Arc;
use ui::{prelude::*, Color, Label, ListItem};
use workspace::{ModalView, Workspace};

actions!(clipboard_history_modal, [ToggleClipboardHistory]);

const MAX_CLIPBOARD_HISTORY: usize = 100;
const MAX_PREVIEW_LENGTH: usize = 500;

#[derive(Clone, Debug)]
pub struct ClipboardEntry {
    pub text: String,
    pub timestamp: std::time::SystemTime,
}

impl ClipboardEntry {
    fn new(text: String) -> Self {
        Self {
            text,
            timestamp: std::time::SystemTime::now(),
        }
    }

    fn preview(&self) -> SharedString {
        let text = self.text.trim();
        if text.len() <= MAX_PREVIEW_LENGTH {
            text.to_string().into()
        } else {
            let mut preview = text[..MAX_PREVIEW_LENGTH].to_string();
            preview.push_str("…");
            preview.into()
        }
    }

    fn age_description(&self) -> String {
        if let Ok(duration) = self.timestamp.elapsed() {
            let secs = duration.as_secs();
            if secs < 60 {
                format!("{}s ago", secs)
            } else if secs < 3600 {
                format!("{}m ago", secs / 60)
            } else if secs < 86400 {
                format!("{}h ago", secs / 3600)
            } else {
                format!("{}d ago", secs / 86400)
            }
        } else {
            "just now".to_string()
        }
    }
}

pub struct ClipboardHistory {
    entries: VecDeque<ClipboardEntry>,
}

impl Global for ClipboardHistory {}

impl ClipboardHistory {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(MAX_CLIPBOARD_HISTORY),
        }
    }

    pub fn add_entry(&mut self, text: String) {
        // Don't add empty entries or duplicates of the most recent entry
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.entries.front() {
            if last.text == text {
                return;
            }
        }

        self.entries.push_front(ClipboardEntry::new(text));
        if self.entries.len() > MAX_CLIPBOARD_HISTORY {
            self.entries.pop_back();
        }
    }

    pub fn entries(&self) -> &VecDeque<ClipboardEntry> {
        &self.entries
    }
}

/// Helper function to track clipboard text in history
pub fn track_clipboard(text: &str, cx: &mut impl gpui::BorrowAppContext) {
    ClipboardHistory::update_global(cx, |history, _| {
        history.add_entry(text.to_string());
    });
}

pub fn init(cx: &mut App) {
    cx.set_global(ClipboardHistory::new());
    cx.bind_keys([KeyBinding::new(
        "cmd-shift-v",
        ToggleClipboardHistory,
        Some("Workspace"),
    )]);
    cx.observe_new(ClipboardHistoryModal::register).detach();

    // Observe clipboard writes by hooking into Copy/Cut actions
    cx.observe_new(|workspace: &mut Workspace, _window, _cx| {
        workspace.register_action(|_workspace, _: &editor::actions::Copy, _window, cx| {
            // Defer to allow clipboard write to complete first
            cx.defer(|cx| {
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(text) = item.text() {
                        track_clipboard(&text, cx);
                    }
                }
            });
        });
        workspace.register_action(|_workspace, _: &editor::actions::CopyAndTrim, _window, cx| {
            // Defer to allow clipboard write to complete first
            cx.defer(|cx| {
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(text) = item.text() {
                        track_clipboard(&text, cx);
                    }
                }
            });
        });
        workspace.register_action(|_workspace, _: &editor::actions::Cut, _window, cx| {
            // Defer to allow clipboard write to complete first
            cx.defer(|cx| {
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(text) = item.text() {
                        track_clipboard(&text, cx);
                    }
                }
            });
        });
        workspace.register_action(|_workspace, _: &editor::actions::CopyAll, _window, cx| {
            // Defer to allow clipboard write to complete first
            cx.defer(|cx| {
                if let Some(item) = cx.read_from_clipboard() {
                    if let Some(text) = item.text() {
                        track_clipboard(&text, cx);
                    }
                }
            });
        });
    })
    .detach();
}

pub struct ClipboardHistoryDelegate {
    entries: Vec<ClipboardEntry>,
    selected_index: usize,
    matches: Vec<usize>,
    clipboard_history_modal: WeakEntity<ClipboardHistoryModal>,
}

pub struct ClipboardHistoryModal {
    picker: Entity<Picker<ClipboardHistoryDelegate>>,
    _picker_subscription: Subscription,
}

impl ModalView for ClipboardHistoryModal {}

impl EventEmitter<DismissEvent> for ClipboardHistoryModal {}

impl Focusable for ClipboardHistoryModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for ClipboardHistoryModal {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("ClipboardHistoryModal");

        v_flex()
            .key_context(key_context)
            .w(rems(40.))
            .child(self.picker.clone())
    }
}

impl ClipboardHistoryModal {
    fn register(
        workspace: &mut Workspace,
        _window: Option<&mut Window>,
        _cx: &mut Context<Workspace>,
    ) {
        workspace.register_action(|workspace, _: &ToggleClipboardHistory, window, cx| {
            Self::toggle(workspace, window, cx);
        });
    }

    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        workspace.toggle_modal(window, cx, |window, cx| Self::new(window, cx));
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak_self = cx.entity().downgrade();

        let entries: Vec<ClipboardEntry> = cx
            .global::<ClipboardHistory>()
            .entries()
            .iter()
            .cloned()
            .collect();

        let delegate = ClipboardHistoryDelegate {
            entries: entries.clone(),
            selected_index: 0,
            matches: (0..entries.len()).collect(),
            clipboard_history_modal: weak_self,
        };

        let picker = cx.new(|cx| {
            Picker::uniform_list(delegate, window, cx)
                .modal(false)
                .max_height(Some(rems(20.).into()))
        });

        let picker_subscription = cx.subscribe_in(&picker, window, Self::on_picker_event);

        Self {
            picker,
            _picker_subscription: picker_subscription,
        }
    }

    fn on_picker_event(
        &mut self,
        _picker: &Entity<Picker<ClipboardHistoryDelegate>>,
        _event: &DismissEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DismissEvent);
    }
}

impl PickerDelegate for ClipboardHistoryDelegate {
    type ListItem = ListItem;

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
        _cx: &mut Context<Picker<Self>>,
    ) {
        self.selected_index = ix;
    }

    fn placeholder_text(&self, _window: &mut Window, _cx: &mut App) -> Arc<str> {
        "Search clipboard history...".into()
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> gpui::Task<()> {
        let query = query.to_lowercase();

        if query.is_empty() {
            self.matches = (0..self.entries.len()).collect();
        } else {
            self.matches = self
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.text.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }

        self.selected_index = 0;
        gpui::Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(&entry_index) = self.matches.get(self.selected_index) {
            if let Some(entry) = self.entries.get(entry_index) {
                let text = entry.text.clone();
                cx.write_to_clipboard(ClipboardItem::new_string(text));

                if let Some(modal) = self.clipboard_history_modal.upgrade() {
                    modal.update(cx, |_, cx| {
                        cx.emit(DismissEvent);
                    });
                }

                // Paste the selected text
                window.dispatch_action(Box::new(editor::actions::Paste), cx);
            }
        }
    }

    fn dismissed(&mut self, _window: &mut Window, _cx: &mut Context<Picker<Self>>) {}

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let entry_index = *self.matches.get(ix)?;
        let entry = self.entries.get(entry_index)?;

        let preview = entry.preview();
        let age = entry.age_description();

        Some(
            ListItem::new(ix)
                .inset(true)
                .toggle_state(selected)
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .child(Label::new(preview)),
                        )
                        .child(Label::new(age).size(ui::LabelSize::Small).color(Color::Muted)),
                ),
        )
    }
}

