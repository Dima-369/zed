use gpui::{
    App, ClipboardItem, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    KeyBinding, KeyContext, Render, Subscription, UniformListScrollHandle, WeakEntity, Window,
    actions,
};
use picker::{Picker, PickerDelegate};
use settings::Settings;
use std::sync::Arc;
use ui::{Label, ListItem, prelude::*};
use workspace::{ModalView, Workspace};

use crate::emoji_picker_settings::EmojiPickerSettings;

actions!(emoji_picker_modal, [ToggleEmojiPicker]);

pub fn init(cx: &mut App) {
    cx.bind_keys([KeyBinding::new(
        "cmd-ctrl-e",
        ToggleEmojiPicker,
        Some("Workspace"),
    )]);
    cx.observe_new(EmojiPickerModal::register).detach();
}

pub struct EmojiPickerDelegate {
    emojis: Vec<EmojiEntry>,
    selected_index: usize,
    matches: Vec<usize>,
    emoji_picker_modal: WeakEntity<EmojiPickerModal>,
}

#[derive(Clone)]
pub struct EmojiEntry {
    emoji: String,
    description: String,
}

pub struct EmojiPickerModal {
    picker: Entity<Picker<EmojiPickerDelegate>>,
    _picker_subscription: Subscription,
}

impl ModalView for EmojiPickerModal {}

impl EventEmitter<DismissEvent> for EmojiPickerModal {}

impl Focusable for EmojiPickerModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for EmojiPickerModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("EmojiPickerModal");

        v_flex()
            .key_context(key_context)
            .elevation_3(cx)
            .w(rems(40.))
            .child(self.picker.clone())
    }
}

impl EmojiPickerModal {
    fn register(
        workspace: &mut Workspace,
        _window: Option<&mut Window>,
        _cx: &mut Context<Workspace>,
    ) {
        workspace.register_action(|workspace, _: &ToggleEmojiPicker, window, cx| {
            Self::toggle(workspace, window, cx);
        });
    }

    pub fn toggle(workspace: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
        workspace.toggle_modal(window, cx, |window, cx| Self::new(window, cx));
    }

    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let weak_self = cx.entity().downgrade();
        let scroll_handle = UniformListScrollHandle::new();

        // Get emoji settings from global settings
        let emoji_strings = EmojiPickerSettings::get_global(cx).emoji_picker.clone();

        let emojis: Vec<EmojiEntry> = emoji_strings
            .iter()
            .filter_map(|s| {
                let mut parts = s.splitn(2, ' ');
                if let (Some(emoji), Some(description)) = (parts.next(), parts.next()) {
                    Some(EmojiEntry {
                        emoji: emoji.to_string(),
                        description: description.to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        let delegate = EmojiPickerDelegate {
            emojis: emojis.clone(),
            selected_index: 0,
            matches: (0..emojis.len()).collect(),
            emoji_picker_modal: weak_self,
        };

        let picker = cx.new(|cx| {
            Picker::uniform_list(delegate, window, cx)
                .modal(false)
                .max_height(Some(rems(20.).into()))
                .track_scroll(scroll_handle.clone())
                .show_scrollbar(true)
        });

        let picker_subscription = cx.subscribe_in(&picker, window, Self::on_picker_event);

        Self {
            picker,
            _picker_subscription: picker_subscription,
        }
    }

    fn on_picker_event(
        &mut self,
        _picker: &Entity<Picker<EmojiPickerDelegate>>,
        _event: &DismissEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.emit(DismissEvent);
    }
}

impl PickerDelegate for EmojiPickerDelegate {
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
        "Choose an emoji to copy to clipboard...".into()
    }

    fn update_matches(
        &mut self,
        query: String,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> gpui::Task<()> {
        let query = query.to_lowercase();

        if query.is_empty() {
            self.matches = (0..self.emojis.len()).collect();
        } else {
            self.matches = self
                .emojis
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    entry.emoji.to_lowercase().contains(&query)
                        || entry.description.to_lowercase().contains(&query)
                })
                .map(|(i, _)| i)
                .collect();
        }

        self.selected_index = 0;
        gpui::Task::ready(())
    }

    fn confirm(&mut self, _secondary: bool, _window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some(&entry_index) = self.matches.get(self.selected_index) {
            if let Some(entry) = self.emojis.get(entry_index) {
                // Extract emoji (up to first whitespace) from the entry
                let emoji = entry
                    .emoji
                    .split_whitespace()
                    .next()
                    .unwrap_or(&entry.emoji);

                // Copy to clipboard only
                cx.write_to_clipboard(ClipboardItem::new_string(emoji.to_string()));

                // Dismiss the modal
                if let Some(modal) = self.emoji_picker_modal.upgrade() {
                    modal.update(cx, |_, cx| {
                        cx.emit(DismissEvent);
                    });
                }
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
        let entry = self.emojis.get(entry_index)?;

        Some(
            ListItem::new(ix).inset(true).toggle_state(selected).child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(Label::new(entry.emoji.clone()))
                            .child(Label::new(entry.description.clone())),
                    ),
            ),
        )
    }
}
