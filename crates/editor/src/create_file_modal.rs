use crate::{Editor, EditorElement, EditorMode, EditorStyle};
use gpui::{
    App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable, Pixels, Render,
    WeakEntity, Window, px,
};
use language::Buffer;
use multi_buffer::MultiBuffer;
use project::Project;
use settings::Settings;
use std::path::PathBuf;
use theme::ThemeSettings;
use ui::{prelude::*, Button, KeyBinding, Label, TextSize};
use workspace::{ModalView, Workspace};

pub struct CreateFileModal {
    filename_editor: Entity<Editor>,
    current_directory: PathBuf,
    project: Entity<Project>,
    workspace: WeakEntity<Workspace>,
}

impl Focusable for CreateFileModal {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.filename_editor.focus_handle(cx)
    }
}

impl EventEmitter<DismissEvent> for CreateFileModal {}

impl ModalView for CreateFileModal {
    fn on_before_dismiss(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> workspace::DismissDecision {
        workspace::DismissDecision::Dismiss(true)
    }
}

impl CreateFileModal {
    pub fn new(
        current_directory: PathBuf,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let buffer = cx.new(|cx| Buffer::local("", cx));
        let multi_buffer = cx.new(|cx| MultiBuffer::singleton(buffer, cx));

        let filename_editor = cx.new(|cx| {
            let mut editor = Editor::new(
                EditorMode::SingleLine,
                multi_buffer,
                None,
                window,
                cx,
            );
            editor.set_placeholder_text("Enter filename...", window, cx);
            editor.set_use_modal_editing(false); // Disable modal editing to allow Enter key to be handled by the modal
            editor.set_show_gutter(false, cx);
            editor
        });

        let focus_handle = filename_editor.focus_handle(cx);

        cx.on_focus_out(&focus_handle, window, |_, _, _, cx| {
            cx.emit(DismissEvent);
        })
        .detach();

        Self {
            filename_editor,
            current_directory,
            project,
            workspace,
        }
    }

    fn editor_style(window: &Window, cx: &App) -> EditorStyle {
        let settings = ThemeSettings::get_global(cx);
        let font_size = TextSize::Default.rems(cx).to_pixels(window.rem_size());
        let line_height = font_size * settings.buffer_line_height.value();

        EditorStyle {
            background: cx.theme().colors().editor_background,
            local_player: cx.theme().players().local(),
            text: gpui::TextStyle {
                color: cx.theme().colors().text,
                font_family: settings.buffer_font.family.clone(),
                font_fallbacks: settings.buffer_font.fallbacks.clone(),
                font_features: settings.buffer_font.features.clone(),
                font_size: TextSize::Default.rems(cx).into(),
                font_weight: settings.buffer_font.weight,
                line_height: line_height.into(),
                ..Default::default()
            },
            syntax: cx.theme().syntax().clone(),
            ..Default::default()
        }
    }

    fn cancel(&mut self, _: &menu::Cancel, _: &mut Window, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }

    fn confirm(&mut self, _: &menu::Confirm, window: &mut Window, cx: &mut Context<Self>) {
        let filename = self.filename_editor.read(cx).text(cx);
        let filename = filename.trim();
        println!("[CreateFileModal] confirm called, filename: {:?}", filename);

        if filename.is_empty() {
            println!("[CreateFileModal] filename is empty, dismissing");
            cx.emit(DismissEvent);
            return;
        }

        let new_file_path = self.current_directory.join(filename);
        println!("[CreateFileModal] new_file_path: {:?}", new_file_path);
        let project = self.project.clone();
        let workspace = self.workspace.clone();

        cx.spawn_in(window, async move |_, cx| {
            println!("[CreateFileModal] spawn started");
            let project_path = project
                .read_with(cx, |project, cx| project.find_project_path(&new_file_path, cx))
                .ok()
                .flatten();
            println!("[CreateFileModal] project_path: {:?}", project_path);

            if let Some(project_path) = project_path {
                let worktree = project
                    .read_with(cx, |project, cx| {
                        project.worktree_for_id(project_path.worktree_id, cx)
                    })
                    .ok()
                    .flatten();
                println!("[CreateFileModal] worktree found: {}", worktree.is_some());

                if let Some(worktree) = worktree {
                    let abs_path = worktree
                        .read_with(cx, |worktree, _| {
                            worktree.absolutize(&project_path.path)
                        })
                        .ok();
                    println!("[CreateFileModal] abs_path: {:?}", abs_path);

                    if let Some(abs_path) = abs_path {
                        let write_result = smol::fs::write(&abs_path, "").await;
                        println!("[CreateFileModal] write result: {:?}", write_result);
                        if write_result.is_ok() {
                            let open_task = workspace.update_in(cx, |workspace, window, cx| {
                                workspace.open_abs_path(
                                    abs_path.clone(),
                                    workspace::OpenOptions::default(),
                                    window,
                                    cx,
                                )
                            });
                            if let Ok(task) = open_task {
                                let result = task.await;
                                println!("[CreateFileModal] open_abs_path awaited result: {:?}", result.is_ok());
                            }
                        }
                    }
                }
            } else {
                println!("[CreateFileModal] no project_path found");
            }
        })
        .detach();

        cx.emit(DismissEvent);
    }

    fn modal_width() -> Pixels {
        px(400.0)
    }
}

impl Render for CreateFileModal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor_style = Self::editor_style(window, cx);
        let focus_handle = self.focus_handle(cx);

        let directory_display = {
            let path_str = self.current_directory.to_string_lossy().to_string();
            let home_dir = std::env::var("HOME").unwrap_or_else(|_| "".to_string());
            if !home_dir.is_empty() && path_str.starts_with(&home_dir) {
                path_str.replacen(&home_dir, "~", 1)
            } else {
                path_str
            }
        };

        v_flex()
            .key_context("CreateFileModal")
            .elevation_3(cx)
            .on_action(cx.listener(Self::cancel))
            .on_action(cx.listener(Self::confirm))
            .w(Self::modal_width())
            .p_4()
            .gap_3()
            .bg(cx.theme().colors().elevated_surface_background)
            .rounded_lg()
            .border_1()
            .border_color(cx.theme().colors().border)
            .child(
                v_flex()
                    .child(Label::new("Create New File").size(LabelSize::Large))
                    .child(
                        Label::new(directory_display)
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .px_2()
                    .py_1p5()
                    .bg(cx.theme().colors().editor_background)
                    .rounded_md()
                    .border_1()
                    .border_color(cx.theme().colors().border_variant)
                    .child(EditorElement::new(&self.filename_editor, editor_style)),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("cancel", "Cancel")
                            .key_binding(KeyBinding::for_action_in(&menu::Cancel, &focus_handle, cx))
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(DismissEvent))),
                    )
                    .child(
                        Button::new("create", "Create")
                            .key_binding(KeyBinding::for_action_in(&menu::Confirm, &focus_handle, cx))
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm(&menu::Confirm, window, cx);
                            })),
                    ),
            )
    }
}
