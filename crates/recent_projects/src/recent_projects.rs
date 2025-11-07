pub mod disconnected_overlay;
mod remote_connections;
mod remote_servers;
mod ssh_config;

#[cfg(target_os = "windows")]
mod wsl_picker;

use remote::RemoteConnectionOptions;
pub use remote_connections::{RemoteConnectionModal, connect, open_remote_project};

use disconnected_overlay::DisconnectedOverlay;
use fuzzy::{StringMatch, StringMatchCandidate};
use gpui::{
    Action, AnyElement, App, Context, DismissEvent, Entity, EventEmitter, FocusHandle, Focusable,
    Subscription, Task, WeakEntity, Window,
};
use ordered_float::OrderedFloat;
use picker::{
    Picker, PickerDelegate,
    highlighted_match_with_paths::{HighlightedMatch, HighlightedMatchWithPaths},
};
pub use remote_connections::SshSettings;
pub use remote_servers::RemoteServerProjects;
use settings::Settings;
use std::{path::Path, sync::Arc};
use ui::{KeyBinding, ListItem, ListItemSpacing, Tooltip, prelude::*, tooltip_container};
use util::{ResultExt, paths::PathExt};
use workspace::{
    CloseIntent, HistoryManager, ModalView, OpenOptions, PathList, SerializedWorkspaceLocation,
    WORKSPACE_DB, Workspace, WorkspaceId, notifications::DetachAndPromptErr,
    with_active_or_new_workspace,
};
use zed_actions::{OpenRecent, OpenRecentZoxide, OpenRemote};

/// Match strings with order-insensitive word matching.
/// Splits the query into words and ensures all words match somewhere in the candidate,
/// regardless of order.
async fn match_strings_order_insensitive<T>(
    candidates: &[T],
    query: &str,
    smart_case: bool,
    max_results: usize,
    cancel_flag: &std::sync::atomic::AtomicBool,
) -> Vec<StringMatch>
where
    T: std::borrow::Borrow<StringMatchCandidate> + Sync,
{
    if candidates.is_empty() || max_results == 0 {
        return Default::default();
    }

    if query.is_empty() {
        return candidates
            .iter()
            .map(|candidate| StringMatch {
                candidate_id: candidate.borrow().id,
                score: 0.,
                positions: Default::default(),
                string: candidate.borrow().string.clone(),
            })
            .collect();
    }

    // Split query into words and remove empty ones
    let words: Vec<&str> = if query.trim().contains(' ') {
        query.split_whitespace().collect()
    } else {
        // For single words, treat the whole query as one word
        vec![query.trim()]
    };
    
    if words.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();

    for candidate in candidates {
        if cancel_flag.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }

        let candidate_borrowed = candidate.borrow();
        let candidate_string = &candidate_borrowed.string;
        let candidate_lower = candidate_string.to_lowercase();

        // Check if all words are present in the candidate (case-insensitive)
        let mut all_words_match = true;
        let mut total_score = 0.0;
        let mut all_positions = Vec::new();

        for word in &words {
            let word_lower = if smart_case {
                word.to_string()
            } else {
                word.to_lowercase()
            };
            
            let found_match = if smart_case {
                candidate_string.contains(word)
            } else {
                candidate_lower.contains(&word_lower)
            };
            
            if found_match {
                if let Some(byte_pos) = if smart_case {
                    candidate_string.find(word)
                } else {
                    candidate_lower.find(&word_lower)
                } {
                    // Calculate a simple score based on position and word length
                    let word_score = 1.0 / (byte_pos as f64 + 1.0) * (word.len() as f64 / candidate_string.len() as f64);
                    total_score += word_score;
                    
                    if let Some(original_byte_pos) = if smart_case {
                        candidate_string.find(word)
                    } else {
                        candidate_string.to_lowercase().find(&word_lower)
                    } {
                        let word_byte_len = word.as_bytes().len();
                        for i in 0..word_byte_len {
                            let pos = original_byte_pos + i;
                            if pos < candidate_string.len() && candidate_string.is_char_boundary(pos) {
                                all_positions.push(pos);
                            }
                        }
                    }
                }
            } else {
                all_words_match = false;
                break;
            }
        }

        if all_words_match {
            all_positions.sort_unstable();
            all_positions.dedup();

            results.push(StringMatch {
                candidate_id: candidate_borrowed.id,
                score: total_score / words.len() as f64, // Average score across words
                positions: all_positions,
                string: candidate_string.clone(),
            });
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(max_results);
    results
}

pub fn init(cx: &mut App) {
    SshSettings::register(cx);

    #[cfg(target_os = "windows")]
    cx.on_action(|open_wsl: &zed_actions::wsl_actions::OpenFolderInWsl, cx| {
        let create_new_window = open_wsl.create_new_window;
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            use gpui::PathPromptOptions;
            use project::DirectoryLister;

            let paths = workspace.prompt_for_open_path(
                PathPromptOptions {
                    files: true,
                    directories: true,
                    multiple: false,
                    prompt: None,
                },
                DirectoryLister::Local(
                    workspace.project().clone(),
                    workspace.app_state().fs.clone(),
                ),
                window,
                cx,
            );

            cx.spawn_in(window, async move |workspace, cx| {
                use util::paths::SanitizedPath;

                let Some(paths) = paths.await.log_err().flatten() else {
                    return;
                };

                let paths = paths
                    .into_iter()
                    .filter_map(|path| SanitizedPath::new(&path).local_to_wsl())
                    .collect::<Vec<_>>();

                if paths.is_empty() {
                    let message = indoc::indoc! { r#"
                        Invalid path specified when trying to open a folder inside WSL.

                        Please note that Zed currently does not support opening network share folders inside wsl.
                    "#};

                    let _ = cx.prompt(gpui::PromptLevel::Critical, "Invalid path", Some(&message), &["Ok"]).await;
                    return;
                }

                workspace.update_in(cx, |workspace, window, cx| {
                    workspace.toggle_modal(window, cx, |window, cx| {
                        crate::wsl_picker::WslOpenModal::new(paths, create_new_window, window, cx)
                    });
                }).log_err();
            })
            .detach();
        });
    });

    #[cfg(target_os = "windows")]
    cx.on_action(|open_wsl: &zed_actions::wsl_actions::OpenWsl, cx| {
        let create_new_window = open_wsl.create_new_window;
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            let handle = cx.entity().downgrade();
            let fs = workspace.project().read(cx).fs().clone();
            workspace.toggle_modal(window, cx, |window, cx| {
                RemoteServerProjects::wsl(create_new_window, fs, window, handle, cx)
            });
        });
    });

    #[cfg(target_os = "windows")]
    cx.on_action(|open_wsl: &remote::OpenWslPath, cx| {
        let open_wsl = open_wsl.clone();
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            let fs = workspace.project().read(cx).fs().clone();
            add_wsl_distro(fs, &open_wsl.distro, cx);
            let open_options = OpenOptions {
                replace_window: window.window_handle().downcast::<Workspace>(),
                ..Default::default()
            };

            let app_state = workspace.app_state().clone();

            cx.spawn_in(window, async move |_, cx| {
                open_remote_project(
                    RemoteConnectionOptions::Wsl(open_wsl.distro.clone()),
                    open_wsl.paths,
                    app_state,
                    open_options,
                    cx,
                )
                .await
            })
            .detach();
        });
    });

    cx.on_action(|open_recent: &OpenRecent, cx| {
        let create_new_window = open_recent.create_new_window;
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            let Some(recent_projects) = workspace.active_modal::<RecentProjects>(cx) else {
                RecentProjects::open(workspace, create_new_window, window, cx);
                return;
            };

            recent_projects.update(cx, |recent_projects, cx| {
                recent_projects
                    .picker
                    .update(cx, |picker, cx| picker.cycle_selection(window, cx))
            });
        });
    });
    cx.on_action(|open_recent_zoxide: &OpenRecentZoxide, cx| {
        let create_new_window = open_recent_zoxide.create_new_window;
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            let Some(recent_projects) = workspace.active_modal::<RecentProjectsZoxide>(cx) else {
                RecentProjectsZoxide::open(workspace, create_new_window, window, cx);
                return;
            };

            recent_projects.update(cx, |recent_projects, cx| {
                recent_projects
                    .picker
                    .update(cx, |picker, cx| picker.cycle_selection(window, cx))
            });
        });
    });
    cx.on_action(|open_remote: &OpenRemote, cx| {
        let from_existing_connection = open_remote.from_existing_connection;
        let create_new_window = open_remote.create_new_window;
        with_active_or_new_workspace(cx, move |workspace, window, cx| {
            if from_existing_connection {
                cx.propagate();
                return;
            }
            let handle = cx.entity().downgrade();
            let fs = workspace.project().read(cx).fs().clone();
            workspace.toggle_modal(window, cx, |window, cx| {
                RemoteServerProjects::new(create_new_window, fs, window, handle, cx)
            })
        });
    });

    cx.observe_new(DisconnectedOverlay::register).detach();
}

#[cfg(target_os = "windows")]
pub fn add_wsl_distro(
    fs: Arc<dyn project::Fs>,
    connection_options: &remote::WslConnectionOptions,
    cx: &App,
) {
    use gpui::ReadGlobal;
    use settings::SettingsStore;

    let distro_name = SharedString::from(&connection_options.distro_name);
    let user = connection_options.user.clone();
    SettingsStore::global(cx).update_settings_file(fs, move |setting, _| {
        let connections = setting
            .remote
            .wsl_connections
            .get_or_insert(Default::default());

        if !connections
            .iter()
            .any(|conn| conn.distro_name == distro_name && conn.user == user)
        {
            use std::collections::BTreeSet;

            connections.push(settings::WslConnection {
                distro_name,
                user,
                projects: BTreeSet::new(),
            })
        }
    });
}

pub struct RecentProjects {
    pub picker: Entity<Picker<RecentProjectsDelegate>>,
    rem_width: f32,
    _subscription: Subscription,
}

impl ModalView for RecentProjects {}

impl RecentProjects {
    fn new(
        delegate: RecentProjectsDelegate,
        rem_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| {
            // We want to use a list when we render paths, because the items can have different heights (multiple paths).
            if delegate.render_paths {
                Picker::list(delegate, window, cx)
            } else {
                Picker::uniform_list(delegate, window, cx)
            }
        });
        let _subscription = cx.subscribe(&picker, |_, _, _, cx| cx.emit(DismissEvent));
        // We do not want to block the UI on a potentially lengthy call to DB, so we're gonna swap
        // out workspace locations once the future runs to completion.
        cx.spawn_in(window, async move |this, cx| {
            let workspaces = WORKSPACE_DB
                .recent_workspaces_on_disk()
                .await
                .log_err()
                .unwrap_or_default();
            this.update_in(cx, move |this, window, cx| {
                this.picker.update(cx, move |picker, cx| {
                    picker.delegate.set_workspaces(workspaces);
                    picker.update_matches(picker.query(cx), window, cx)
                })
            })
            .ok()
        })
        .detach();
        Self {
            picker,
            rem_width,
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
            let delegate = RecentProjectsDelegate::new(weak, create_new_window, true);

            Self::new(delegate, 34., window, cx)
        })
    }
}

impl EventEmitter<DismissEvent> for RecentProjects {}

impl Focusable for RecentProjects {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for RecentProjects {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("RecentProjects")
            .w(rems(self.rem_width))
            .child(self.picker.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.picker.update(cx, |this, cx| {
                    this.cancel(&Default::default(), window, cx);
                })
            }))
    }
}

pub struct RecentProjectsDelegate {
    workspace: WeakEntity<Workspace>,
    workspaces: Vec<(WorkspaceId, SerializedWorkspaceLocation, PathList)>,
    selected_match_index: usize,
    matches: Vec<StringMatch>,
    render_paths: bool,
    create_new_window: bool,
    // Flag to reset index when there is a new query vs not reset index when user delete an item
    reset_selected_match_index: bool,
    has_any_non_local_projects: bool,
}

impl RecentProjectsDelegate {
    fn new(workspace: WeakEntity<Workspace>, create_new_window: bool, render_paths: bool) -> Self {
        Self {
            workspace,
            workspaces: Vec::new(),
            selected_match_index: 0,
            matches: Default::default(),
            create_new_window,
            render_paths,
            reset_selected_match_index: true,
            has_any_non_local_projects: false,
        }
    }

    pub fn set_workspaces(
        &mut self,
        workspaces: Vec<(WorkspaceId, SerializedWorkspaceLocation, PathList)>,
    ) {
        self.workspaces = workspaces;
        self.has_any_non_local_projects = !self
            .workspaces
            .iter()
            .all(|(_, location, _)| matches!(location, SerializedWorkspaceLocation::Local));
    }
}
impl EventEmitter<DismissEvent> for RecentProjectsDelegate {}
impl PickerDelegate for RecentProjectsDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, window: &mut Window, _: &mut App) -> Arc<str> {
        let (create_window, reuse_window) = if self.create_new_window {
            (
                window.keystroke_text_for(&menu::Confirm),
                window.keystroke_text_for(&menu::SecondaryConfirm),
            )
        } else {
            (
                window.keystroke_text_for(&menu::SecondaryConfirm),
                window.keystroke_text_for(&menu::Confirm),
            )
        };
        Arc::from(format!(
            "{reuse_window} reuses this window, {create_window} opens a new one",
        ))
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
            .workspaces
            .iter()
            .enumerate()
            .filter(|(_, (id, _, _))| !self.is_current_workspace(*id, cx))
            .map(|(id, (_, _, paths))| {
                let combined_string = paths
                    .ordered_paths()
                    .map(|path| path.compact().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("");
                StringMatchCandidate::new(id, &combined_string)
            })
            .collect::<Vec<_>>();
        self.matches = smol::block_on(match_strings_order_insensitive(
            candidates.as_slice(),
            query,
            smart_case,
            100,
            &Default::default(),
        ));
        self.matches.sort_unstable_by(|a, b| {
            b.score
                .partial_cmp(&a.score) // Descending score
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.candidate_id.cmp(&b.candidate_id)) // Ascending candidate_id for ties
        });

        if self.reset_selected_match_index {
            self.selected_match_index = self
                .matches
                .iter()
                .enumerate()
                .rev()
                .max_by_key(|(_, m)| OrderedFloat(m.score))
                .map(|(ix, _)| ix)
                .unwrap_or(0);
        }
        self.reset_selected_match_index = true;
        Task::ready(())
    }

    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some((selected_match, workspace)) = self
            .matches
            .get(self.selected_index())
            .zip(self.workspace.upgrade())
        {
            let (candidate_workspace_id, candidate_workspace_location, candidate_workspace_paths) =
                &self.workspaces[selected_match.candidate_id];
            let replace_current_window = if self.create_new_window {
                secondary
            } else {
                !secondary
            };
            workspace.update(cx, |workspace, cx| {
                if workspace.database_id() == Some(*candidate_workspace_id) {
                    return;
                }
                match candidate_workspace_location.clone() {
                    SerializedWorkspaceLocation::Local => {
                        let paths = candidate_workspace_paths.paths().to_vec();
                        if replace_current_window {
                            cx.spawn_in(window, async move |workspace, cx| {
                                let continue_replacing = workspace
                                    .update_in(cx, |workspace, window, cx| {
                                        workspace.prepare_to_close(
                                            CloseIntent::ReplaceWindow,
                                            window,
                                            cx,
                                        )
                                    })?
                                    .await?;
                                if continue_replacing {
                                    workspace
                                        .update_in(cx, |workspace, window, cx| {
                                            workspace
                                                .open_workspace_for_paths(true, paths, window, cx)
                                        })?
                                        .await
                                } else {
                                    Ok(())
                                }
                            })
                        } else {
                            workspace.open_workspace_for_paths(false, paths, window, cx)
                        }
                    }
                    SerializedWorkspaceLocation::Remote(mut connection) => {
                        let app_state = workspace.app_state().clone();

                        let replace_window = if replace_current_window {
                            window.window_handle().downcast::<Workspace>()
                        } else {
                            None
                        };

                        let open_options = OpenOptions {
                            replace_window,
                            ..Default::default()
                        };

                        if let RemoteConnectionOptions::Ssh(connection) = &mut connection {
                            SshSettings::get_global(cx)
                                .fill_connection_options_from_settings(connection);
                        };

                        let paths = candidate_workspace_paths.paths().to_vec();

                        cx.spawn_in(window, async move |_, cx| {
                            open_remote_project(
                                connection.clone(),
                                paths,
                                app_state,
                                open_options,
                                cx,
                            )
                            .await
                        })
                    }
                }
                .detach_and_prompt_err(
                    "Failed to open project",
                    window,
                    cx,
                    |_, _, _| None,
                );
            });
            cx.emit(DismissEvent);
        }
    }

    fn dismissed(&mut self, _window: &mut Window, _: &mut Context<Picker<Self>>) {}

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        let text = if self.workspaces.is_empty() {
            "Recently opened projects will show up here".into()
        } else {
            "No matches".into()
        };
        Some(text)
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let hit = self.matches.get(ix)?;

        let (_, location, paths) = self.workspaces.get(hit.candidate_id)?;

        let mut path_start_offset = 0;

        let (match_labels, paths): (Vec<_>, Vec<_>) = paths
            .ordered_paths()
            .map(|p| p.compact())
            .map(|path| {
                let highlighted_text =
                    highlights_for_path(path.as_ref(), &hit.positions, path_start_offset);
                path_start_offset += highlighted_text.1.text.len();
                highlighted_text
            })
            .unzip();

        let prefix = match &location {
            SerializedWorkspaceLocation::Remote(RemoteConnectionOptions::Wsl(wsl)) => {
                Some(SharedString::from(&wsl.distro_name))
            }
            _ => None,
        };

        let highlighted_match = HighlightedMatchWithPaths {
            prefix,
            match_label: HighlightedMatch::join(match_labels.into_iter().flatten(), ", "),
            paths,
        };

        Some(
            ListItem::new(ix)
                .toggle_state(selected)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .child(
                    h_flex()
                        .flex_grow()
                        .gap_3()
                        .when(self.has_any_non_local_projects, |this| {
                            this.child(match location {
                                SerializedWorkspaceLocation::Local => Icon::new(IconName::Screen)
                                    .color(Color::Muted)
                                    .into_any_element(),
                                SerializedWorkspaceLocation::Remote(options) => {
                                    Icon::new(match options {
                                        RemoteConnectionOptions::Ssh { .. } => IconName::Server,
                                        RemoteConnectionOptions::Wsl { .. } => IconName::Linux,
                                    })
                                    .color(Color::Muted)
                                    .into_any_element()
                                }
                            })
                        })
                        .child({
                            let mut highlighted = highlighted_match.clone();
                            if !self.render_paths {
                                highlighted.paths.clear();
                            }
                            highlighted.render(window, cx)
                        }),
                )
                .map(|el| {
                    let delete_button = div()
                        .child(
                            IconButton::new("delete", IconName::Close)
                                .icon_size(IconSize::Small)
                                .on_click(cx.listener(move |this, _event, window, cx| {
                                    cx.stop_propagation();
                                    window.prevent_default();

                                    this.delegate.delete_recent_project(ix, window, cx)
                                }))
                                .tooltip(Tooltip::text("Delete from Recent Projects...")),
                        )
                        .into_any_element();

                    if self.selected_index() == ix {
                        el.end_slot::<AnyElement>(delete_button)
                    } else {
                        el.end_hover_slot::<AnyElement>(delete_button)
                    }
                })
                .tooltip(move |_, cx| {
                    let tooltip_highlighted_location = highlighted_match.clone();
                    cx.new(|_| MatchTooltip {
                        highlighted_location: tooltip_highlighted_location,
                    })
                    .into()
                }),
        )
    }

    fn render_footer(&self, _: &mut Window, cx: &mut Context<Picker<Self>>) -> Option<AnyElement> {
        Some(
            h_flex()
                .w_full()
                .p_2()
                .gap_2()
                .justify_end()
                .border_t_1()
                .border_color(cx.theme().colors().border_variant)
                .child(
                    Button::new("remote", "Open Remote Folder")
                        .key_binding(KeyBinding::for_action(
                            &OpenRemote {
                                from_existing_connection: false,
                                create_new_window: false,
                            },
                            cx,
                        ))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(
                                OpenRemote {
                                    from_existing_connection: false,
                                    create_new_window: false,
                                }
                                .boxed_clone(),
                                cx,
                            )
                        }),
                )
                .child(
                    Button::new("local", "Open Local Folder")
                        .key_binding(KeyBinding::for_action(&workspace::Open, cx))
                        .on_click(|_, window, cx| {
                            window.dispatch_action(workspace::Open.boxed_clone(), cx)
                        }),
                )
                .into_any(),
        )
    }
}

// Compute the highlighted text for the name and path
fn highlights_for_path(
    path: &Path,
    match_positions: &Vec<usize>,
    path_start_offset: usize,
) -> (Option<HighlightedMatch>, HighlightedMatch) {
    let path_string = path.to_string_lossy();
    let path_text = path_string.to_string();
    let path_byte_len = path_text.len();
    // Get the subset of match highlight positions that line up with the given path.
    // Also adjusts them to start at the path start
    let path_positions = match_positions
        .iter()
        .copied()
        .skip_while(|position| *position < path_start_offset)
        .take_while(|position| *position < path_start_offset + path_byte_len)
        .map(|position| position - path_start_offset)
        .collect::<Vec<_>>();

    // Again subset the highlight positions to just those that line up with the file_name
    // again adjusted to the start of the file_name
    let file_name_text_and_positions = path.file_name().map(|file_name| {
        let file_name_text = file_name.to_string_lossy().into_owned();
        let file_name_start_byte = path_byte_len - file_name_text.len();
        let highlight_positions = path_positions
            .iter()
            .copied()
            .skip_while(|position| *position < file_name_start_byte)
            .take_while(|position| *position < file_name_start_byte + file_name_text.len())
            .map(|position| position - file_name_start_byte)
            .collect::<Vec<_>>();
        HighlightedMatch {
            text: file_name_text,
            highlight_positions,
            color: Color::Default,
        }
    });

    (
        file_name_text_and_positions,
        HighlightedMatch {
            text: path_text,
            highlight_positions: path_positions,
            color: Color::Default,
        },
    )
}
impl RecentProjectsDelegate {
    fn delete_recent_project(
        &self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) {
        if let Some(selected_match) = self.matches.get(ix) {
            let (workspace_id, _, _) = self.workspaces[selected_match.candidate_id];
            cx.spawn_in(window, async move |this, cx| {
                let _ = WORKSPACE_DB.delete_workspace_by_id(workspace_id).await;
                let workspaces = WORKSPACE_DB
                    .recent_workspaces_on_disk()
                    .await
                    .unwrap_or_default();
                this.update_in(cx, move |picker, window, cx| {
                    picker.delegate.set_workspaces(workspaces);
                    picker
                        .delegate
                        .set_selected_index(ix.saturating_sub(1), window, cx);
                    picker.delegate.reset_selected_match_index = false;
                    picker.update_matches(picker.query(cx), window, cx);
                    // After deleting a project, we want to update the history manager to reflect the change.
                    // But we do not emit a update event when user opens a project, because it's handled in `workspace::load_workspace`.
                    if let Some(history_manager) = HistoryManager::global(cx) {
                        history_manager
                            .update(cx, |this, cx| this.delete_history(workspace_id, cx));
                    }
                })
            })
            .detach();
        }
    }

    fn is_current_workspace(
        &self,
        workspace_id: WorkspaceId,
        cx: &mut Context<Picker<Self>>,
    ) -> bool {
        if let Some(workspace) = self.workspace.upgrade() {
            let workspace = workspace.read(cx);
            if Some(workspace_id) == workspace.database_id() {
                return true;
            }
        }

        false
    }
}

pub struct RecentProjectsZoxide {
    pub picker: Entity<Picker<RecentProjectsZoxideDelegate>>,
    rem_width: f32,
    _subscription: Subscription,
}

impl ModalView for RecentProjectsZoxide {}

impl RecentProjectsZoxide {
    fn new(
        delegate: RecentProjectsZoxideDelegate,
        rem_width: f32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let picker = cx.new(|cx| Picker::uniform_list(delegate, window, cx));
        let _subscription = cx.subscribe(&picker, |_, _, _, cx| cx.emit(DismissEvent));

        // Load zoxide directories asynchronously
        cx.spawn_in(window, async move |this, cx| {
            let output = std::process::Command::new("zoxide")
                .args(&["query", "--list"])
                .output();

            let directories = match output {
                Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .map(|line| line.trim().to_string())
                    .filter(|line| !line.is_empty())
                    .collect::<Vec<_>>(),
                _ => Vec::new(),
            };

            this.update_in(cx, move |this, window, cx| {
                this.picker.update(cx, move |picker, cx| {
                    picker.delegate.set_directories(directories);
                    picker.update_matches(picker.query(cx), window, cx)
                })
            })
            .ok()
        })
        .detach();

        Self {
            picker,
            rem_width,
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
            let delegate = RecentProjectsZoxideDelegate::new(weak, create_new_window);
            Self::new(delegate, 34., window, cx)
        })
    }
}

impl EventEmitter<DismissEvent> for RecentProjectsZoxide {}

impl Focusable for RecentProjectsZoxide {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.picker.focus_handle(cx)
    }
}

impl Render for RecentProjectsZoxide {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .key_context("RecentProjectsZoxide")
            .w(rems(self.rem_width))
            .child(self.picker.clone())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.picker.update(cx, |this, cx| {
                    this.cancel(&Default::default(), window, cx);
                })
            }))
    }
}

pub struct RecentProjectsZoxideDelegate {
    workspace: WeakEntity<Workspace>,
    directories: Vec<String>,
    selected_match_index: usize,
    matches: Vec<StringMatch>,
    create_new_window: bool,
    reset_selected_match_index: bool,
}

impl RecentProjectsZoxideDelegate {
    fn new(workspace: WeakEntity<Workspace>, create_new_window: bool) -> Self {
        Self {
            workspace,
            directories: Vec::new(),
            selected_match_index: 0,
            matches: Default::default(),
            create_new_window,
            reset_selected_match_index: true,
        }
    }

    pub fn set_directories(&mut self, directories: Vec<String>) {
        self.directories = directories;
    }

    fn format_path_for_display(&self, path: &str) -> String {
        if let Some(home_dir) = std::env::var("HOME").ok() {
            if path.starts_with(&home_dir) {
                return path.replacen(&home_dir, "~", 1);
            }
        }
        path.to_string()
    }
}

impl EventEmitter<DismissEvent> for RecentProjectsZoxideDelegate {}

impl PickerDelegate for RecentProjectsZoxideDelegate {
    type ListItem = ListItem;

    fn placeholder_text(&self, window: &mut Window, _: &mut App) -> Arc<str> {
        let (create_window, reuse_window) = if self.create_new_window {
            (
                window.keystroke_text_for(&menu::SecondaryConfirm),
                window.keystroke_text_for(&menu::Confirm),
            )
        } else {
            (
                window.keystroke_text_for(&menu::Confirm),
                window.keystroke_text_for(&menu::SecondaryConfirm),
            )
        };
        Arc::from(format!(
            "{reuse_window} reuses this window, {create_window} opens a new one (zoxide)",
        ))
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
        _cx: &mut Context<Picker<Self>>,
    ) -> gpui::Task<()> {
        let query = query.trim_start();
        let smart_case = query.chars().any(|c| c.is_uppercase());
        let candidates = self
            .directories
            .iter()
            .enumerate()
            .map(|(id, path)| StringMatchCandidate::new(id, path))
            .collect::<Vec<_>>();

        self.matches = smol::block_on(match_strings_order_insensitive(
            candidates.as_slice(),
            query,
            smart_case,
            100,
            &Default::default(),
        ));

        // Don't sort - preserve zoxide's order
        self.matches.sort_unstable_by_key(|m| m.candidate_id);

        if self.reset_selected_match_index {
            self.selected_match_index = self
                .matches
                .iter()
                .enumerate()
                .rev()
                .max_by_key(|(_, m)| OrderedFloat(m.score))
                .map(|(ix, _)| ix)
                .unwrap_or(0);
        }
        self.reset_selected_match_index = true;
        Task::ready(())
    }

    fn confirm(&mut self, secondary: bool, window: &mut Window, cx: &mut Context<Picker<Self>>) {
        if let Some((selected_match, workspace)) = self
            .matches
            .get(self.selected_index())
            .zip(self.workspace.upgrade())
        {
            let directory_path = &self.directories[selected_match.candidate_id];
            let path = std::path::PathBuf::from(directory_path);

            // Add to zoxide
            let _ = std::process::Command::new("zoxide")
                .args(&["add", directory_path])
                .output();

            let replace_current_window = if self.create_new_window {
                !secondary
            } else {
                secondary
            };

            workspace
                .update(cx, |workspace, cx| {
                    let paths = vec![path];
                    if replace_current_window {
                        cx.spawn_in(window, async move |workspace, cx| {
                            let continue_replacing = workspace
                                .update_in(cx, |workspace, window, cx| {
                                    workspace.prepare_to_close(
                                        CloseIntent::ReplaceWindow,
                                        window,
                                        cx,
                                    )
                                })?
                                .await?;
                            if continue_replacing {
                                workspace
                                    .update_in(cx, |workspace, window, cx| {
                                        workspace.open_workspace_for_paths(true, paths, window, cx)
                                    })?
                                    .await
                            } else {
                                Ok(())
                            }
                        })
                    } else {
                        workspace.open_workspace_for_paths(false, paths, window, cx)
                    }
                })
                .detach_and_log_err(cx);
            cx.emit(DismissEvent);
        }
    }

    fn dismissed(&mut self, _window: &mut Window, _: &mut Context<Picker<Self>>) {}

    fn no_matches_text(&self, _window: &mut Window, _cx: &mut App) -> Option<SharedString> {
        let text = if self.directories.is_empty() {
            "No zoxide directories found. Make sure zoxide is installed and has been used.".into()
        } else {
            "No matches".into()
        };
        Some(text)
    }

    fn render_match(
        &self,
        ix: usize,
        selected: bool,
        window: &mut Window,
        cx: &mut Context<Picker<Self>>,
    ) -> Option<Self::ListItem> {
        let hit = self.matches.get(ix)?;
        let directory_path = self.directories.get(hit.candidate_id)?;
        let display_path = self.format_path_for_display(directory_path);

        // Adjust highlight positions if the path was shortened
        let adjusted_positions = if display_path != *directory_path {
            let display_chars: Vec<char> = display_path.chars().collect();

            // Find the offset where the paths start to match
            let mut offset = 0;
            if let Some(home_dir) = std::env::var("HOME").ok() {
                if directory_path.starts_with(&home_dir) && display_path.starts_with('~') {
                    // The home directory was replaced with ~, so offset is home_dir.len() - 1
                    offset = home_dir.chars().count().saturating_sub(1);
                }
            }

            // Adjust positions and filter out any that are now out of bounds
            hit.positions
                .iter()
                .filter_map(|&pos| {
                    if pos >= offset {
                        let adjusted_pos = pos - offset;
                        if adjusted_pos < display_chars.len() {
                            Some(adjusted_pos)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            hit.positions.clone()
        };

        let highlighted_text = HighlightedMatch {
            text: display_path.clone(),
            highlight_positions: adjusted_positions,
            color: Color::Default,
        };

        let tooltip_text = display_path.clone();
        Some(
            ListItem::new(ix)
                .toggle_state(selected)
                .inset(true)
                .spacing(ListItemSpacing::Sparse)
                .child(
                    h_flex()
                        .flex_grow()
                        .gap_3()
                        .child(Icon::new(IconName::Folder).color(Color::Muted))
                        .child(highlighted_text.render(window, cx)),
                )
                .tooltip(move |_, cx| {
                    cx.new(|_| SimpleTooltip {
                        text: tooltip_text.clone(),
                    })
                    .into()
                }),
        )
    }

    fn render_footer(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Picker<Self>>,
    ) -> Option<AnyElement> {
        None
    }
}

struct SimpleTooltip {
    text: String,
}

impl Render for SimpleTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tooltip_container(cx, |div, _| div.child(self.text.clone()))
    }
}

struct MatchTooltip {
    highlighted_location: HighlightedMatchWithPaths,
}

impl Render for MatchTooltip {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        tooltip_container(cx, |div, _| {
            self.highlighted_location.render_paths_children(div)
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use dap::debugger_settings::DebuggerSettings;
    use editor::Editor;
    use gpui::{TestAppContext, UpdateGlobal, WindowHandle};
    use project::Project;
    use serde_json::json;
    use settings::SettingsStore;
    use util::path;
    use workspace::{AppState, open_paths};

    use super::*;

    #[gpui::test]
    async fn test_prompts_on_dirty_before_submit(cx: &mut TestAppContext) {
        let app_state = init_test(cx);

        cx.update(|cx| {
            SettingsStore::update_global(cx, |store, cx| {
                store.update_user_settings(cx, |settings| {
                    settings
                        .session
                        .get_or_insert_default()
                        .restore_unsaved_buffers = Some(false)
                });
            });
        });

        app_state
            .fs
            .as_fake()
            .insert_tree(
                path!("/dir"),
                json!({
                    "main.ts": "a"
                }),
            )
            .await;
        cx.update(|cx| {
            open_paths(
                &[PathBuf::from(path!("/dir/main.ts"))],
                app_state,
                workspace::OpenOptions::default(),
                cx,
            )
        })
        .await
        .unwrap();
        assert_eq!(cx.update(|cx| cx.windows().len()), 1);

        let workspace = cx.update(|cx| cx.windows()[0].downcast::<Workspace>().unwrap());
        workspace
            .update(cx, |workspace, _, _| assert!(!workspace.is_edited()))
            .unwrap();

        let editor = workspace
            .read_with(cx, |workspace, cx| {
                workspace
                    .active_item(cx)
                    .unwrap()
                    .downcast::<Editor>()
                    .unwrap()
            })
            .unwrap();
        workspace
            .update(cx, |_, window, cx| {
                editor.update(cx, |editor, cx| editor.insert("EDIT", window, cx));
            })
            .unwrap();
        workspace
            .update(cx, |workspace, _, _| assert!(workspace.is_edited(), "After inserting more text into the editor without saving, we should have a dirty project"))
            .unwrap();

        let recent_projects_picker = open_recent_projects(&workspace, cx);
        workspace
            .update(cx, |_, _, cx| {
                recent_projects_picker.update(cx, |picker, cx| {
                    assert_eq!(picker.query(cx), "");
                    let delegate = &mut picker.delegate;
                    delegate.matches = vec![StringMatch {
                        candidate_id: 0,
                        score: 1.0,
                        positions: Vec::new(),
                        string: "fake candidate".to_string(),
                    }];
                    delegate.set_workspaces(vec![(
                        WorkspaceId::default(),
                        SerializedWorkspaceLocation::Local,
                        PathList::new(&[path!("/test/path")]),
                    )]);
                });
            })
            .unwrap();

        assert!(
            !cx.has_pending_prompt(),
            "Should have no pending prompt on dirty project before opening the new recent project"
        );
        cx.dispatch_action(*workspace, menu::Confirm);
        workspace
            .update(cx, |workspace, _, cx| {
                assert!(
                    workspace.active_modal::<RecentProjects>(cx).is_none(),
                    "Should remove the modal after selecting new recent project"
                )
            })
            .unwrap();
        assert!(
            cx.has_pending_prompt(),
            "Dirty workspace should prompt before opening the new recent project"
        );
        cx.simulate_prompt_answer("Cancel");
        assert!(
            !cx.has_pending_prompt(),
            "Should have no pending prompt after cancelling"
        );
        workspace
            .update(cx, |workspace, _, _| {
                assert!(
                    workspace.is_edited(),
                    "Should be in the same dirty project after cancelling"
                )
            })
            .unwrap();
    }

    fn open_recent_projects(
        workspace: &WindowHandle<Workspace>,
        cx: &mut TestAppContext,
    ) -> Entity<Picker<RecentProjectsDelegate>> {
        cx.dispatch_action(
            (*workspace).into(),
            OpenRecent {
                create_new_window: false,
            },
        );
        workspace
            .update(cx, |workspace, _, cx| {
                workspace
                    .active_modal::<RecentProjects>(cx)
                    .unwrap()
                    .read(cx)
                    .picker
                    .clone()
            })
            .unwrap()
    }

    fn init_test(cx: &mut TestAppContext) -> Arc<AppState> {
        cx.update(|cx| {
            let state = AppState::test(cx);
            language::init(cx);
            crate::init(cx);
            editor::init(cx);
            workspace::init_settings(cx);
            DebuggerSettings::register(cx);
            Project::init_settings(cx);
            state
        })
    }
}
