use gpui::{
    App, Context, DismissEvent, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, Render,
    Task, Window,
};
use std::sync::Arc;
use ui::{prelude::*, Button, ButtonStyle, Label, LabelSize, TintColor};
use futures::channel::oneshot;
use menu;

use crate::modal_layer::ModalView;

pub struct UnsavedChangesModal {
    message: Arc<str>,
    detail: Option<Arc<str>>,
    buttons: Vec<String>,
    selected_button: usize,
    focus_handle: FocusHandle,
    result_sender: Option<oneshot::Sender<usize>>,
}

impl ModalView for UnsavedChangesModal {}

impl UnsavedChangesModal {


    pub fn show(
        workspace: &mut crate::Workspace,
        message: impl Into<Arc<str>>,
        detail: Option<impl Into<Arc<str>>>,
        buttons: Vec<impl Into<String>>,
        window: &mut Window,
        cx: &mut Context<crate::Workspace>,
    ) -> Task<Option<usize>> {
        let message = message.into();
        let detail = detail.map(|d| d.into());
        let buttons: Vec<String> = buttons.into_iter().map(|b| b.into()).collect();

        let (sender, receiver) = oneshot::channel();

        workspace.toggle_modal(window, cx, |window, cx| {
            let modal = Self {
                message: message.clone(),
                detail: detail.clone(),
                buttons: buttons.clone(),
                selected_button: 0,
                focus_handle: cx.focus_handle(),
                result_sender: Some(sender),
            };
            modal.focus_handle.focus(window);
            modal
        });

        cx.spawn(async move |_workspace, _cx| {
            receiver.await.ok()
        })
    }

    fn select_button(&mut self, index: usize, _window: &mut Window, cx: &mut Context<Self>) {
        if index < self.buttons.len() {
            self.selected_button = index;
            cx.notify();
        }
    }

    fn confirm_selection(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sender) = self.result_sender.take() {
            let _ = sender.send(self.selected_button);
        }
        cx.emit(DismissEvent);
    }

    fn cancel(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(sender) = self.result_sender.take() {
            // Find "Cancel" button index, or use last button as default
            let cancel_index = self.buttons.iter().position(|b| b.to_lowercase().contains("cancel"))
                .unwrap_or(self.buttons.len().saturating_sub(1));
            let _ = sender.send(cancel_index);
        }
        cx.emit(DismissEvent);
    }
}

impl EventEmitter<DismissEvent> for UnsavedChangesModal {}

impl Focusable for UnsavedChangesModal {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for UnsavedChangesModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let focus_handle = self.focus_handle.clone();
        
        v_flex()
            .key_context("UnsavedChangesModal")
            .track_focus(&focus_handle)
            .w(rems(28.))
            .p_6()
            .gap_4()
            .bg(cx.theme().colors().elevated_surface_background)
            .border_1()
            .border_color(cx.theme().colors().border)
            .rounded_lg()
            .shadow_lg()
            .on_action(cx.listener(|this, _: &menu::Confirm, window, cx| {
                this.confirm_selection(window, cx);
            }))
            .on_action(cx.listener(|this, _: &menu::Cancel, window, cx| {
                this.cancel(window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                match event.keystroke.key.as_str() {
                    "Enter" => {
                        // Save (first button)
                        this.select_button(0, window, cx);
                        this.confirm_selection(window, cx);
                    }
                    "h" => {
                        // Don't Save (second button)
                        this.select_button(1, window, cx);
                        this.confirm_selection(window, cx);
                    }
                    "Escape" => {
                        this.cancel(window, cx);
                    }
                    _ => {}
                }
            }))
            .child(
                // Warning icon and message
                h_flex()
                    .gap_3()
                    .items_start()
                    .child(
                        Icon::new(IconName::Warning)
                            .size(IconSize::Medium)
                            .color(Color::Warning)
                    )
                    .child(
                        v_flex()
                            .gap_3()
                            .child(
                                Label::new(self.message.clone())
                                    .size(LabelSize::Default)
                                    .weight(FontWeight::MEDIUM)
                            )
                            .when_some(self.detail.clone(), |this, detail| {
                                this.child(
                                    Label::new(detail)
                                        .size(LabelSize::Small)
                                        .color(Color::Muted)
                                )
                            })
                            .child(
                                v_flex()
                                    .gap_1()
                                    .mt_2()
                                    .child(
                                        Label::new("Enter: Save")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                    )
                                    .child(
                                        Label::new("h: Don't Save")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                    )
                                    .child(
                                        Label::new("Escape: Cancel")
                                            .size(LabelSize::Small)
                                            .color(Color::Muted)
                                    )
                            )
                    )
            )
            .child(
                // Buttons
                h_flex()
                    .gap_2()
                    .justify_end()
                    .children(
                        self.buttons.iter().enumerate().map(|(index, button_text)| {
                            let _is_selected = index == self.selected_button;
                            let is_primary = index == 0; // First button (Save) is primary
                            let is_destructive = button_text.to_lowercase().contains("don't save") 
                                || button_text.to_lowercase().contains("discard");
                            
                            Button::new(("button", index), button_text.clone())
                                .style(if is_primary {
                                    ButtonStyle::Filled
                                } else if is_destructive {
                                    ButtonStyle::Tinted(TintColor::Error)
                                } else {
                                    ButtonStyle::Subtle
                                })
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.select_button(index, window, cx);
                                    this.confirm_selection(window, cx);
                                }))
                        })
                    )
            )
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.cancel(window, cx);
            }))
    }
}
