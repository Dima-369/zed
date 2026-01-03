use clipboard_history::{ClipboardEntry, ClipboardHistory};
use gpui::{
    actions, App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, FocusHandle,
    Focusable, KeyBinding, KeyContext, Render, SharedString, Subscription,
    WeakEntity, Window,
};
use picker::{Picker, PickerDelegate};
use std::sync::Arc;
use ui::{prelude::*, Color, Label, ListItem};
use workspace::{ModalView, Workspace};

actions!(clipboard_history_modal, [ToggleClipboardHistory]);

pub fn init(cx: &mut App) {
    clipboard_history::init(cx);
    cx.bind_keys([KeyBinding::new(
        "cmd-shift-v",
        ToggleClipboardHistory,
        Some("Workspace"),
    )]);
    cx.observe_new(ClipboardHistoryModal::register).detach();
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("ClipboardHistoryModal");

        v_flex()
            .key_context(key_context)
            .elevation_3(cx)
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

        println!("[ClipboardHistory] Modal opened with {} entries", entries.len());
        for (i, entry) in entries.iter().enumerate() {
            println!("[ClipboardHistory]   Entry {}: {:?}", i, entry.preview());
        }

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

