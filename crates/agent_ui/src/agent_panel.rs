use std::cmp::Ordering;
use std::collections::HashMap;
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use acp_thread::{AcpThread, AcpThreadEvent, ThreadStatus};
use agent::{ContextServerRegistry, DbThreadMetadata, HistoryEntry, HistoryStore, ThreadStore};
use agent_client_protocol as acp;
use db::kvp::{Dismissable, KEY_VALUE_STORE};
use project::{
    ExternalAgentServerName,
    agent_server_store::{
        AgentServerCommand, AgentServerStore, AllAgentServersSettings, CLAUDE_CODE_NAME,
        CODEX_NAME, GEMINI_NAME,
    },
};
use serde::{Deserialize, Serialize};
use settings::{
    LanguageModelProviderSetting, LanguageModelSelection, Settings as _, SettingsStore,
};

use zed_actions::agent::{OpenClaudeCodeOnboardingModal, ReauthenticateAgent};

use crate::agent_panel_tab::{AgentPanelTab, AgentPanelTabIdentity, TabId, TabLabelRender};
use crate::ui::{AcpOnboardingModal, ClaudeCodeOnboardingModal};
use crate::{
    ActivateNextTab, ActivatePreviousTab, AddContextServer, AgentDiffPane, CloseActiveThreadTab,
    CloseActiveThreadTabOrDock, CopyThreadToClipboard, DeleteRecentlyOpenThread, ExternalAgent,
    ExternalAgentInitialContent, Follow, InlineAssistant, LoadThreadFromClipboard,
    NewExternalAgentThread, NewTextThread, NewThread,
    OpenActiveThreadAsMarkdown, OpenAgentDiff, OpenHistory, ResetTrialEndUpsell, ResetTrialUpsell,
    ToggleNavigationMenu, ToggleNewThreadMenu, ToggleOptionsMenu, TogglePlan,
    acp::AcpThreadView,
    agent_configuration::{AgentConfiguration, AssistantConfigurationEvent},
    slash_command::SlashCommandCompletionProvider,
    text_thread_editor::{AgentPanelDelegate, TextThreadEditor, make_lsp_adapter_delegate},
    ui::{AgentOnboardingModal, EndTrialUpsell},
};
use agent_settings::{AgentSettings, DefaultView};
use ai_onboarding::AgentPanelOnboarding;
use anyhow::{Result, anyhow};
use assistant_slash_command::SlashCommandWorkingSet;
use assistant_text_thread::{TextThread, TextThreadEvent, TextThreadSummary};
use client::{UserStore, zed_urls};
use cloud_llm_client::{Plan, PlanV1, PlanV2, UsageLimit};
use editor::{Anchor, AnchorRangeExt as _, Editor, EditorEvent, MultiBuffer, actions::Cancel};
use extension::ExtensionEvents;
use extension_host::ExtensionStore;
use fs::Fs;
use gpui::{
    Action, Animation, AnimationExt, AnyElement, App, AsyncWindowContext, Corner, DismissEvent,
    Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, KeyContext, Pixels, ScrollHandle,
    SharedString, Subscription, Task, UpdateGlobal, WeakEntity, prelude::*, pulsating_between,
};
use language::LanguageRegistry;
use language_model::{ConfigurationError, LanguageModelRegistry};
use menu::Confirm;
use project::{Project, ProjectPath, Worktree};
use prompt_store::{PromptBuilder, PromptStore, UserPromptId};
use rules_library::{RulesLibrary, open_rules_library};
use search::{BufferSearchBar, buffer_search};
use settings::{Settings, update_settings_file};
use theme::ThemeSettings;
use ui::{
    Callout, ContextMenu, ContextMenuEntry, IconButtonShape, Indicator, KeyBinding, PopoverMenu,
    PopoverMenuHandle, Tab, TabBar, TabCloseSide, TabPosition, Tooltip, prelude::*,
    utils::WithRemSize,
};
use util::ResultExt as _;
use workspace::{
    CollaboratorId, DraggedSelection, DraggedTab, ToggleZoom, ToolbarItemView, Workspace,
    dock::{DockPosition, Panel, PanelEvent},
};
use zed_actions::{
    DecreaseBufferFontSize, IncreaseBufferFontSize, ResetBufferFontSize,
    agent::{
        OpenAcpOnboardingModal, OpenOnboardingModal, OpenSettings, ResetAgentZoom, ResetOnboarding,
    },
    assistant::{OpenRulesLibrary, ToggleFocus},
};

const AGENT_PANEL_KEY: &str = "agent_panel";
const LOADING_SUMMARY_PLACEHOLDER: &str = "Loading Summary…";

#[derive(Serialize, Deserialize, Debug)]
struct SerializedAgentPanel {
    width: Option<Pixels>,
    selected_agent: Option<AgentType>,
}

pub fn init(cx: &mut App) {
    cx.observe_new(
        |workspace: &mut Workspace, _window, _cx: &mut Context<Workspace>| {
            workspace
                .register_action(|workspace, action: &NewThread, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.new_thread(action, window, cx));
                        workspace.focus_panel::<AgentPanel>(window, cx);
                    }
                })
                .register_action(|workspace, action: &DeleteRecentlyOpenThread, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| {
                            panel.delete_recently_open_thread(action, window, cx)
                        });
                    }
                })
                .register_action(
                    |workspace, action: &crate::NewNativeAgentThreadFromSummary, window, cx| {
                        if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                            workspace.focus_panel::<AgentPanel>(window, cx);
                            panel.update(cx, |panel, cx| {
                                panel.load_agent_thread(
                                    acp_thread::AgentSessionInfo {
                                        session_id: action.from_session_id.clone(),
                                        cwd: None,
                                        title: None,
                                        updated_at: None,
                                        meta: None,
                                    },
                                    window,
                                    cx,
                                )
                            });
                        }
                    },
                )
                .register_action(|workspace, _: &ExpandMessageEditor, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| panel.expand_message_editor(window, cx));
                    }
                })
                .register_action(|workspace, _: &OpenHistory, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| panel.open_history(window, cx));
                    }
                })
                .register_action(|workspace, _: &OpenSettings, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| panel.open_configuration(window, cx));
                    }
                })
                .register_action(|workspace, _: &NewTextThread, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| panel.new_text_thread(window, cx));
                    }
                })
                .register_action(|workspace, _: &Follow, window, cx| {
                    workspace.follow(CollaboratorId::Agent, window, cx);
                })
                .register_action(|workspace, _: &OpenAgentDiff, window, cx| {
                    let thread = workspace
                        .panel::<AgentPanel>(cx)
                        .and_then(|panel| panel.read(cx).active_thread_view().cloned())
                        .map(|thread_view| thread_view.read(cx).thread().cloned())
                        .flatten();

                    if let Some(thread) = thread {
                        AgentDiffPane::deploy_in_workspace(thread, workspace, window, cx);
                    }
                })
                .register_action(|workspace, _: &ToggleNavigationMenu, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| {
                            panel.toggle_navigation_menu(&ToggleNavigationMenu, window, cx);
                        });
                    }
                })
                .register_action(|workspace, _: &ToggleOptionsMenu, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| {
                            panel.toggle_options_menu(&ToggleOptionsMenu, window, cx);
                        });
                    }
                })
                .register_action(|workspace, _: &ToggleNewThreadMenu, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| {
                            panel.toggle_new_thread_menu(&ToggleNewThreadMenu, window, cx);
                        });
                    }
                })
                .register_action(|workspace, _: &OpenOnboardingModal, window, cx| {
                    AgentOnboardingModal::toggle(workspace, window, cx)
                })
                .register_action(|workspace, _: &OpenAcpOnboardingModal, window, cx| {
                    AcpOnboardingModal::toggle(workspace, window, cx)
                })
                .register_action(|workspace, _: &OpenClaudeCodeOnboardingModal, window, cx| {
                    ClaudeCodeOnboardingModal::toggle(workspace, window, cx)
                })
                .register_action(|_workspace, _: &ResetOnboarding, window, cx| {
                    window.dispatch_action(workspace::RestoreBanner.boxed_clone(), cx);
                    window.refresh();
                })
                .register_action(|_workspace, _: &ResetTrialUpsell, _window, cx| {
                    OnboardingUpsell::set_dismissed(false, cx);
                })
                .register_action(|_workspace, _: &ResetTrialEndUpsell, _window, cx| {
                    TrialEndUpsell::set_dismissed(false, cx);
                })
                .register_action(|workspace, _: &ResetAgentZoom, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| {
                            panel.reset_agent_zoom(window, cx);
                        });
                    }
                })
                .register_action(|workspace, action: &NewExternalAgentThread, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| {
                            panel.external_thread(
                                action.agent.clone().unwrap_or(ExternalAgent::NativeAgent),
                                None,
                                None,
                                window,
                                cx,
                            )
                        });
                    }
                })
                .register_action(|workspace, _: &CopyThreadToClipboard, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.copy_thread_to_clipboard(window, cx));
                    }
                })
                .register_action(|workspace, _: &LoadThreadFromClipboard, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        workspace.focus_panel::<AgentPanel>(window, cx);
                        panel.update(cx, |panel, cx| panel.load_thread_from_clipboard(window, cx));
                    }
                })
                .register_action(|workspace, _: &ActivateNextTab, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.activate_next_tab(window, cx));
                    }
                })
                .register_action(|workspace, _: &ActivatePreviousTab, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.activate_previous_tab(window, cx));
                    }
                })
                .register_action(|workspace, action: &CloseActiveThreadTabOrDock, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| {
                            panel.close_active_thread_tab_or_dock(action, window, cx)
                        });
                    }
                });
        },
    )
    .detach();
}

pub enum ActiveView {
    ExternalAgentThread {
        thread_view: Entity<AcpThreadView>,
    },
    TextThread {
        text_thread_editor: Entity<TextThreadEditor>,
        title_editor: Entity<Editor>,
        buffer_search_bar: Entity<BufferSearchBar>,
    },
    History,
    Configuration,
    Uninitialized,
}

enum WhichFontSize {
    AgentFont,
    BufferFont,
    None,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub enum AgentType {
    #[default]
    NativeAgent,
    TextThread,
    Gemini,
    ClaudeCode,
    Codex,
    Custom {
        name: SharedString,
        command: AgentServerCommand,
    },
}

impl AgentType {
    fn label(&self) -> SharedString {
        match self {
            Self::NativeAgent | Self::TextThread => "Zed Agent".into(),
            Self::Gemini => "Gemini CLI".into(),
            Self::ClaudeCode => "Claude Code".into(),
            Self::Codex => "Codex".into(),
            Self::Custom { name, .. } => name.into(),
        }
    }

    fn icon(&self) -> Option<IconName> {
        match self {
            Self::NativeAgent | Self::TextThread => None,
            Self::Gemini => Some(IconName::AiGemini),
            Self::ClaudeCode => Some(IconName::AiClaude),
            Self::Codex => Some(IconName::AiOpenAi),
            Self::Custom { .. } => Some(IconName::Sparkle),
        }
    }
}

impl From<AgentType> for ExternalAgent {
    fn from(value: AgentType) -> Self {
        match value {
            AgentType::NativeAgent => Self::NativeAgent,
            AgentType::TextThread => Self::NativeAgent,
            AgentType::Gemini => Self::Gemini,
            AgentType::ClaudeCode => Self::ClaudeCode,
            AgentType::Codex => Self::Codex,
            AgentType::Custom { name, .. } => Self::Custom { name },
        }
    }
}

impl From<ExternalAgent> for AgentType {
    fn from(value: ExternalAgent) -> Self {
        match value {
            ExternalAgent::Gemini => Self::Gemini,
            ExternalAgent::ClaudeCode => Self::ClaudeCode,
            ExternalAgent::Codex => Self::Codex,
            ExternalAgent::Custom { name } => Self::Custom {
                name,
                command: placeholder_command(),
            },
            crate::ExternalAgent::NativeAgent => Self::NativeAgent,
        }
    }
}

fn placeholder_command() -> AgentServerCommand {
    AgentServerCommand {
        command: "".into(),
        args: vec![],
    }
}

impl ActiveView {
    pub fn which_font_size_used(&self) -> WhichFontSize {
        match self {
            ActiveView::Uninitialized
            | ActiveView::ExternalAgentThread { .. }
            | ActiveView::History => WhichFontSize::AgentFont,
            ActiveView::TextThread { .. } => WhichFontSize::BufferFont,
            ActiveView::Configuration => WhichFontSize::None,
        }
    }

    pub fn native_agent(
        fs: Arc<dyn Fs>,
        prompt_store: Option<Entity<PromptStore>>,
        history_store: Entity<HistoryStore>,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let thread_store = ThreadStore::global(cx);
        let server = Rc::new(agent::NativeAgentServer::new(fs.clone(), thread_store));
        let thread_view = cx.new(|cx| {
            AcpThreadView::new(
                server,
                None,
                None,
                workspace,
                project,
                Some(ThreadStore::global(cx)),
                prompt_store,
                history_store,
                window,
                cx,
            )
        });

        Self::ExternalAgentThread { thread_view }
    }

    pub fn text_thread(
        text_thread_editor: Entity<TextThreadEditor>,
        history_store: Entity<HistoryStore>,
        language_registry: Arc<LanguageRegistry>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let title = text_thread_editor.read(cx).title(cx).to_string();

        let title_editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(title, window, cx);
            editor
        });

        let mut suppress_first_edit = true;

        cx.subscribe(&title_editor, {
            let text_thread_editor = text_thread_editor.clone();
            move |editor, event, cx| match event {
                EditorEvent::BufferEdited => {
                    if suppress_first_edit {
                        suppress_first_edit = false;
                        return;
                    }
                    let new_summary = editor.read(cx).text(cx);

                    text_thread_editor.update(cx, |text_thread_editor, cx| {
                        text_thread_editor
                            .text_thread()
                            .update(cx, |text_thread, cx| {
                                text_thread.set_custom_summary(new_summary, cx);
                            })
                    })
                }
                EditorEvent::Blurred => {
                    if editor.read(cx).text(cx).is_empty() {
                        let summary = text_thread_editor
                            .read(cx)
                            .text_thread()
                            .read(cx)
                            .summary()
                            .or_default();

                        editor.update(cx, |editor, cx| {
                            editor.set_text(summary, cx);
                        });
                    }
                }
                _ => {}
            }
        })
        .detach();

        cx.subscribe(&text_thread_editor.read(cx).text_thread().clone(), {
            let editor = title_editor.clone();
            move |text_thread, event, cx| match event {
                TextThreadEvent::SummaryGenerated => {
                    let summary = text_thread.read(cx).summary().or_default();

                    editor.update(cx, |editor, cx| {
                        editor.set_text(summary, cx);
                    })
                }
                TextThreadEvent::PathChanged { .. } => {}
                _ => {}
            }
        })
        .detach();

        let text_thread = {
            let editor = text_thread_editor.read(cx);
            editor.text_thread().clone()
        };

        history_store.update(cx, |store, cx| {
            if let Some(path) = text_thread.read(cx).path() {
                store.push_recently_opened_entry(agent::HistoryEntryId::TextThread(path.clone()), cx)
            }
        });

        let buffer_search_bar =
            cx.new(|cx| BufferSearchBar::new(Some(language_registry), window, cx));
        buffer_search_bar.update(cx, |buffer_search_bar, cx| {
            buffer_search_bar.set_active_pane_item(Some(&text_thread_editor), window, cx)
        });

        Self::TextThread {
            text_thread_editor,
            title_editor,
            buffer_search_bar,
        }
    }
}

pub struct AgentPanel {
    workspace: WeakEntity<Workspace>,
    user_store: Entity<UserStore>,
    project: Entity<Project>,
    fs: Arc<dyn Fs>,
    language_registry: Arc<LanguageRegistry>,
    history_store: Entity<HistoryStore>,
    acp_history: Entity<acp::thread_history::AcpThreadHistory>,
    thread_store: Entity<ThreadStore>,
    text_thread_store: Entity<assistant_text_thread::TextThreadStore>,
    prompt_store: Option<Entity<PromptStore>>,
    context_server_registry: Entity<ContextServerRegistry>,
    inline_assist_context_store: Entity<ContextStore>,
    configuration: Option<Entity<AgentConfiguration>>,
    configuration_subscription: Option<Subscription>,
    overlay_view: Option<ActiveView>,
    overlay_previous_tab_id: Option<TabId>,
    new_thread_menu_handle: PopoverMenuHandle<ContextMenu>,
    agent_panel_menu_handle: PopoverMenuHandle<ContextMenu>,
    agent_navigation_menu_handle: PopoverMenuHandle<ContextMenu>,
    agent_navigation_menu: Option<Entity<ContextMenu>>,
    panel_focus_handle: FocusHandle,
    _extension_subscription: Option<Subscription>,
    width: Option<Pixels>,
    height: Option<Pixels>,
    zoomed: bool,
    pending_serialization: Option<Task<Result<()>>>,
    onboarding: Entity<AgentPanelOnboarding>,
    selected_agent: AgentType,
    detached_threads: HashMap<acp::SessionId, DetachedThread>,
    pending_tab_removal: Option<TabId>,
    tabs: Vec<AgentPanelTab>,
    active_tab_id: TabId,
    tab_bar_scroll_handle: ScrollHandle,
    title_edit_overlay_tab_id: Option<TabId>,
    loading: bool,
}

struct DetachedThread {
    _thread: Entity<AcpThread>,
    _subscription: Subscription,
}

impl AgentPanel {
    fn serialize(&mut self, cx: &mut Context<Self>) {
        let width = self.width;
        let selected_agent = self.selected_agent.clone();
        self.pending_serialization = Some(cx.background_spawn(async move {
            KEY_VALUE_STORE
                .write_kvp(
                    AGENT_PANEL_KEY.into(),
                    serde_json::to_string(&SerializedAgentPanel {
                        width,
                        selected_agent: Some(selected_agent),
                    })?,
                )
                .await?;
            anyhow::Ok(())
        }));
    }

    pub fn load(
        workspace: WeakEntity<Workspace>,
        prompt_builder: Arc<PromptBuilder>,
        mut cx: AsyncWindowContext,
    ) -> Task<Result<Entity<Self>>> {
        let prompt_store = cx.update(|_window, cx| PromptStore::global(cx));
        cx.spawn(async move |cx| {
            let prompt_store = match prompt_store {
                Ok(prompt_store) => prompt_store.await.ok(),
                Err(_) => None,
            };
            let history_store = cx
                .update(|_window, cx| HistoryStore::global(cx))?
                .await
                .log_err();
            let serialized_panel = if let Some(panel) = cx
                .background_spawn(async move { KEY_VALUE_STORE.read_kvp(AGENT_PANEL_KEY) })
                .await
                .log_err()
                .flatten()
            {
                serde_json::from_str::<SerializedAgentPanel>(&panel).log_err()
            } else {
                None
            };

            let slash_commands = Arc::new(SlashCommandWorkingSet::default());
            let text_thread_store = workspace
                .update(cx, |workspace, cx| {
                    let project = workspace.project().clone();
                    assistant_text_thread::TextThreadStore::new(
                        project,
                        prompt_builder,
                        slash_commands,
                        cx,
                    )
                })?
                .await?;

            let panel = workspace.update_in(cx, |workspace, window, cx| {
                let panel = cx.new(|cx| {
                    Self::new(
                        workspace,
                        text_thread_store,
                        prompt_store,
                        history_store.unwrap(),
                        window,
                        cx,
                    )
                });

                panel.as_mut(cx).loading = true;
                if let Some(serialized_panel) = serialized_panel {
                    panel.update(cx, |panel, cx| {
                        panel.width = serialized_panel.width.map(|w| w.round());
                        if let Some(selected_agent) = serialized_panel.selected_agent {
                            panel.selected_agent = selected_agent.clone();
                            panel.new_agent_thread(selected_agent, window, cx);
                            log::info!("Restore the default panel from serialized panel.");
                            panel.remove_tab_by_id(0, window, cx);
                        }
                        cx.notify();
                    });
                }
                panel.as_mut(cx).loading = false;
                panel
            })?;

            Ok(panel)
        })
    }

    fn new(
        workspace: &Workspace,
        text_thread_store: Entity<assistant_text_thread::TextThreadStore>,
        prompt_store: Option<Entity<PromptStore>>,
        history_store: Entity<HistoryStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let fs = workspace.app_state().fs.clone();
        let user_store = workspace.app_state().user_store.clone();
        let project = workspace.project();
        let language_registry = project.read(cx).languages().clone();
        let client = workspace.client().clone();
        let workspace = workspace.weak_handle();

        let context_server_registry =
            cx.new(|cx| ContextServerRegistry::new(project.read(cx).context_server_store(), cx));
        let inline_assist_context_store = cx.new(|cx| {
            ContextStore::new(
                project.clone(),
                fs.clone(),
                <dyn LanguageModelRegistry>::global(cx),
                cx,
            )
        });

        let thread_store = ThreadStore::global(cx);
        let acp_history = cx.new(|cx| {
            acp::thread_history::AcpThreadHistory::new(history_store.clone(), window, cx)
        });
        cx.subscribe_in(&acp_history, window, |this, _, event, window, cx| {
            match event {
                acp::thread_history::ThreadHistoryEvent::Open(thread) => {
                    this.load_agent_thread(thread.clone(), window, cx);
                }
            }
        })
        .detach();

        let panel_type = AgentSettings::get_global(cx).default_view;
        let (active_view, selected_agent) = match panel_type {
            DefaultView::Thread => (
                ActiveView::native_agent(
                    fs.clone(),
                    prompt_store.clone(),
                    history_store.clone(),
                    project.clone(),
                    workspace.clone(),
                    window,
                    cx,
                ),
                AgentType::NativeAgent,
            ),
            DefaultView::TextThread => {
                let context = text_thread_store.update(cx, |store, cx| store.create(cx));
                let lsp_adapter_delegate = make_lsp_adapter_delegate(&project, cx)
                    .log_err()
                    .flatten();

                let text_thread_editor = cx.new(|cx| {
                    let mut editor = TextThreadEditor::for_text_thread(
                        context,
                        fs.clone(),
                        workspace.clone(),
                        project.clone(),
                        lsp_adapter_delegate,
                        window,
                        cx,
                    );
                    editor.insert_default_prompt(window, cx);
                    editor
                });
                (
                    ActiveView::text_thread(
                        text_thread_editor,
                        history_store.clone(),
                        language_registry.clone(),
                        window,
                        cx,
                    ),
                    AgentType::TextThread,
                )
            }
        };

        let weak_panel = cx.entity().downgrade();

        window.defer(cx, move |window, cx| {
            let panel = weak_panel.clone();
            let agent_navigation_menu =
                ContextMenu::build_persistent(window, cx, move |mut menu, _window, cx| {
                    if let Some(panel) = panel.upgrade() {
                        if let Some(history_store) = panel.read(cx).history_store.clone().into() {
                            menu = Self::populate_recently_opened_menu_section(
                                menu,
                                panel,
                                history_store,
                                cx,
                            );
                            menu = menu.action("View All", Box::new(OpenHistory));
                        }
                    }

                    menu = menu
                        .fixed_width(px(320.).into())
                        .keep_open_on_confirm(false)
                        .key_context("NavigationMenu");

                    menu
                });
            weak_panel
                .update(cx, |panel, cx| {
                    cx.subscribe_in(
                        &agent_navigation_menu,
                        window,
                        |_, menu, _: &DismissEvent, window, cx| {
                            menu.update(cx, |menu, _| {
                                menu.clear_selected();
                            });
                            cx.focus_self(window);
                        },
                    )
                    .detach();
                    panel.agent_navigation_menu = Some(agent_navigation_menu);
                })
                .ok();
        });

        let onboarding = cx.new(|cx| {
            AgentPanelOnboarding::new(
                user_store.clone(),
                client,
                |_window, cx| {
                    OnboardingUpsell::set_dismissed(true, cx);
                },
                cx,
            )
        });

        let extension_subscription = if let Some(extension_events) = ExtensionEvents::try_global(cx)
        {
            let project = project.clone();
            Some(cx.subscribe(&extension_events, move |this, _source, event, cx| {
                match event {
                    extension::Event::ExtensionInstalled(_)
                    | extension::Event::ExtensionUninstalled(_)
                    | extension::Event::ExtensionsInstalledChanged => {
                        this.sync_agent_servers_from_extensions(cx);
                    }
                    _ => {}
                }
                if let extension::Event::ExtensionInstalled(extension_id) = event {
                    let agent_server_store = project.read(cx).agent_server_store();
                    if agent_server_store.read(cx).is_extension_agent(extension_id) {
                        this.active_tab_id = this.tabs.len();
                    }
                }
            }))
        } else {
            None
        };

        let panel_focus_handle = cx.focus_handle();
        cx.on_focus_in(&panel_focus_handle, window, |_, _, cx| {
            cx.notify();
        })
        .detach();
        cx.on_focus_out(&panel_focus_handle, window, |_, _, _, cx| {
            cx.notify();
        })
        .detach();

        let mut panel = Self {
            overlay_view: None,
            workspace,
            user_store,
            project: project.clone(),
            fs: fs.clone(),
            language_registry,
            history_store,
            acp_history,
            text_thread_store,
            prompt_store,
            thread_store,
            configuration: None,
            configuration_subscription: None,
            context_server_registry,
            inline_assist_context_store,
            overlay_previous_tab_id: None,
            new_thread_menu_handle: PopoverMenuHandle::default(),
            agent_panel_menu_handle: PopoverMenuHandle::default(),
            agent_navigation_menu_handle: PopoverMenuHandle::default(),
            agent_navigation_menu: None,
            _extension_subscription: extension_subscription,
            width: None,
            height: None,
            zoomed: false,
            pending_serialization: None,
            onboarding,
            selected_agent: AgentType::default(),
            detached_threads: HashMap::new(),
            loading: false,
            pending_tab_removal: None,
            panel_focus_handle,
            tabs: vec![AgentPanelTab::new(active_view, selected_agent)],
            active_tab_id: 0,
            tab_bar_scroll_handle: ScrollHandle::new(),
            title_edit_overlay_tab_id: None,
        };

        panel.sync_agent_servers_from_extensions(cx);
        panel
    }

    pub fn toggle_focus(
        workspace: &mut Workspace,
        _: &ToggleFocus,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        if workspace
            .panel::<Self>(cx)
            .is_some_and(|panel| panel.read(cx).enabled(cx))
        {
            workspace.toggle_panel_focus::<Self>(window, cx);
        }
    }

    pub(crate) fn prompt_store(&self) -> &Option<Entity<PromptStore>> {
        &self.prompt_store
    }

    pub fn thread_store(&self) -> &Entity<ThreadStore> {
        &self.thread_store
    }

    pub fn history(&self) -> &Entity<AcpThreadHistory> {
        &self.acp_history
    }

    pub fn open_thread(
        &mut self,
        thread: AgentSessionInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.external_thread(
            crate::ExternalAgent::NativeAgent,
            Some(thread),
            None,
            window,
            cx,
        );
    }

    pub(crate) fn context_server_registry(&self) -> &Entity<ContextServerRegistry> {
        &self.context_server_registry
    }

    pub fn is_hidden(workspace: &Entity<Workspace>, cx: &App) -> bool {
        let workspace_read = workspace.read(cx);

        workspace_read
            .panel::<AgentPanel>(cx)
            .map(|panel| {
                let panel_id = Entity::entity_id(&panel);

                let is_visible = workspace_read.all_docks().iter().any(|dock| {
                    dock.read(cx)
                        .visible_panel()
                        .is_some_and(|visible_panel| visible_panel.panel_id() == panel_id)
                });

                !is_visible
            })
            .unwrap_or(true)
    }

    fn active_view(&self) -> &ActiveView {
        self.overlay_view
            .as_ref()
            .unwrap_or_else(|| self.active_tab().view())
    }

    fn active_thread_view(&self) -> Option<&Entity<AcpThreadView>> {
        match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view, .. } => Some(thread_view),
            ActiveView::TextThread { .. }
            | ActiveView::History
            | ActiveView::Configuration
            | ActiveView::Uninitialized => None,
        }
    }

    fn new_thread(&mut self, _action: &NewThread, window: &mut Window, cx: &mut Context<Self>) {
        self.new_agent_thread(AgentType::NativeAgent, window, cx);
    }

    fn new_text_thread(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        telemetry::event!("Agent Thread Started", agent = "zed-text");

        let context = self
            .text_thread_store
            .update(cx, |context_store, cx| context_store.create(cx));
        let lsp_adapter_delegate = make_lsp_adapter_delegate(&self.project, cx)
            .log_err()
            .flatten();

        let text_thread_editor = cx.new(|cx| {
            let mut editor = TextThreadEditor::for_text_thread(
                context,
                self.fs.clone(),
                self.workspace.clone(),
                self.project.clone(),
                lsp_adapter_delegate,
                window,
                cx,
            );
            editor.insert_default_prompt(window, cx);
            editor
        });

        if self.selected_agent != AgentType::TextThread {
            self.selected_agent = AgentType::TextThread;
            self.serialize(cx);
        }

        self.push_tab(
            ActiveView::text_thread(
                text_thread_editor.clone(),
                self.history_store.clone(),
                self.language_registry.clone(),
                window,
                cx,
            ),
            AgentType::TextThread,
            window,
            cx,
        );
    }

    fn deploy_rules_library(
        &mut self,
        action: &OpenRulesLibrary,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        open_rules_library(
            self.language_registry.clone(),
            Box::new(PromptLibraryInlineAssist::new(self.workspace.clone())),
            Rc::new(|| {
                Rc::new(SlashCommandCompletionProvider::new(
                    Arc::new(SlashCommandWorkingSet::default()),
                    None,
                    None,
                ))
            }),
            action
                .prompt_to_select
                .map(|uuid| UserPromptId(uuid).into()),
            cx,
        )
        .detach_and_log_err(cx);
    }

    fn expand_message_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread_view) = self.active_thread_view() else {
            return;
        };

        let Some(active_thread) = thread_view.read(cx).as_active_thread() else {
            return;
        };

        active_thread.update(cx, |active_thread, cx| {
            active_thread.expand_message_editor(&ExpandMessageEditor, window, cx);
            active_thread.focus_handle(cx).focus(window, cx);
        })
    }

    fn open_history(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.active_view(), ActiveView::History) {
            if let Some(previous_tab_id) = self.overlay_previous_tab_id.take() {
                self.set_active_tab_by_id(previous_tab_id, window, cx);
            }
        } else {
            self.set_tab_overlay_view(ActiveView::History, window, cx);
        }
        cx.notify();
    }

    pub(crate) fn open_saved_text_thread(
        &mut self,
        path: Arc<Path>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Task<Result<()>> {
        let text_thread_task = self
            .text_thread_store
            .update(cx, |store, cx| store.open_local(path, cx));
        cx.spawn_in(window, async move |this, cx| {
            let text_thread = text_thread_task.await?;
            this.update_in(cx, |this, window, cx| {
                this.open_text_thread(text_thread, window, cx);
            })
        })
    }

    pub(crate) fn open_text_thread(
        &mut self,
        text_thread: Entity<TextThread>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let lsp_adapter_delegate = make_lsp_adapter_delegate(&self.project.clone(), cx)
            .log_err()
            .flatten();
        let editor = cx.new(|cx| {
            TextThreadEditor::for_text_thread(
                text_thread,
                self.fs.clone(),
                self.workspace.clone(),
                self.project.clone(),
                lsp_adapter_delegate,
                window,
                cx,
            )
        });

        if self.selected_agent != AgentType::TextThread {
            self.selected_agent = AgentType::TextThread;
            self.serialize(cx);
        }

        self.push_tab(
            ActiveView::text_thread(
                editor,
                self.history_store.clone(),
                self.language_registry.clone(),
                window,
                cx,
            ),
            AgentType::TextThread,
            window,
            cx,
        );
    }

    pub fn go_back(&mut self, _: &workspace::GoBack, window: &mut Window, cx: &mut Context<Self>) {
        if self.title_edit_overlay_tab_id.take().is_some() {
            self.focus_active_panel_thread(window, cx);
            return;
        }

        match self.active_view() {
            ActiveView::Configuration | ActiveView::History => {
                if let Some(previous_tab_id) = self.overlay_previous_tab_id.take() {
                    self.active_tab_id = previous_tab_id;
                    self.overlay_view = None;
                    self.focus_active_panel_thread(window, cx);
                }
            }
            _ => {}
        }
    }

    pub fn toggle_navigation_menu(
        &mut self,
        _: &ToggleNavigationMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_navigation_menu_handle.toggle(window, cx);
    }

    pub fn toggle_options_menu(
        &mut self,
        _: &ToggleOptionsMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.agent_panel_menu_handle.toggle(window, cx);
    }

    pub fn toggle_new_thread_menu(
        &mut self,
        _: &ToggleNewThreadMenu,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.new_thread_menu_handle.toggle(window, cx);
    }

    pub fn increase_font_size(
        &mut self,
        action: &IncreaseBufferFontSize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_font_size_action(action.persist, px(1.0), cx);
    }

    pub fn decrease_font_size(
        &mut self,
        action: &DecreaseBufferFontSize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.handle_font_size_action(action.persist, px(-1.0), cx);
    }

    fn handle_font_size_action(&mut self, persist: bool, delta: Pixels, cx: &mut Context<Self>) {
        match self.active_view().which_font_size_used() {
            WhichFontSize::AgentFont => {
                if persist {
                    update_settings_file(self.fs.clone(), cx, move |settings, cx| {
                        let agent_ui_font_size =
                            ThemeSettings::get_global(cx).agent_ui_font_size(cx) + delta;
                        let agent_buffer_font_size =
                            ThemeSettings::get_global(cx).agent_buffer_font_size(cx) + delta;

                        let _ = settings
                            .theme
                            .agent_ui_font_size
                            .insert(f32::from(theme::clamp_font_size(agent_ui_font_size)).into());
                        let _ = settings.theme.agent_buffer_font_size.insert(
                            f32::from(theme::clamp_font_size(agent_buffer_font_size)).into(),
                        );
                    });
                } else {
                    theme::adjust_agent_ui_font_size(cx, |size| size + delta);
                    theme::adjust_agent_buffer_font_size(cx, |size| size + delta);
                }
            }
            WhichFontSize::BufferFont => {
                cx.propagate();
            }
            WhichFontSize::None => {}
        }
    }

    pub fn reset_font_size(
        &mut self,
        action: &ResetBufferFontSize,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action.persist {
            update_settings_file(self.fs.clone(), cx, move |settings, _| {
                settings.theme.agent_ui_font_size = None;
                settings.theme.agent_buffer_font_size = None;
            });
        } else {
            theme::reset_agent_ui_font_size(cx);
            theme::reset_agent_buffer_font_size(cx);
        }
    }

    pub fn reset_agent_zoom(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        theme::reset_agent_ui_font_size(cx);
        theme::reset_agent_buffer_font_size(cx);
    }

    pub fn toggle_zoom(&mut self, _: &ToggleZoom, window: &mut Window, cx: &mut Context<Self>) {
        if self.zoomed {
            cx.emit(PanelEvent::ZoomOut);
        } else {
            if !self.focus_handle(cx).contains_focused(window, cx) {
                cx.focus_self(window);
            }
            cx.emit(PanelEvent::ZoomIn);
        }
    }

    pub(crate) fn open_configuration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent_server_store = self.project.read(cx).agent_server_store().clone();
        let context_server_store = self.project.read(cx).context_server_store();
        let fs = self.fs.clone();

        self.set_tab_overlay_view(ActiveView::Configuration, window, cx);
        self.configuration = Some(cx.new(|cx| {
            AgentConfiguration::new(
                fs,
                agent_server_store,
                context_server_store,
                self.context_server_registry.clone(),
                self.language_registry.clone(),
                self.workspace.clone(),
                window,
                cx,
            )
        }));

        if let Some(configuration) = self.configuration.as_ref() {
            self.configuration_subscription = Some(cx.subscribe_in(
                configuration,
                window,
                Self::handle_agent_configuration_event,
            ));

            configuration.focus_handle(cx).focus(window, cx);
        }
    }

    pub(crate) fn open_active_thread_as_markdown(
        &mut self,
        _: &OpenActiveThreadAsMarkdown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(workspace) = self.workspace.upgrade()
            && let Some(thread_view) = self.active_thread_view()
            && let Some(active_thread) = thread_view.read(cx).as_active_thread()
        {
            active_thread.update(cx, |thread, cx| {
                thread
                    .open_thread_as_markdown(workspace, window, cx)
                    .detach_and_log_err(cx);
            });
        }
    }

    fn copy_thread_to_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(thread) = self.active_native_agent_thread(cx) else {
            if let Some(workspace) = self.workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    struct NoThreadToast;
                    workspace.show_toast(
                        workspace::Toast::new(
                            workspace::notifications::NotificationId::unique::<NoThreadToast>(),
                            "No active native thread to copy",
                        )
                        .autohide(),
                        cx,
                    );
                });
            }
            return;
        };

        let workspace = self.workspace.clone();
        let load_task = thread.read(cx).to_db(cx);

        cx.spawn_in(window, async move |_this, cx| {
            let db_thread = load_task.await;
            let shared_thread = SharedThread::from_db_thread(&db_thread);
            let thread_data = shared_thread.to_bytes()?;
            let encoded = base64::Engine::encode(&base64::prelude::BASE64_STANDARD, &thread_data);

            cx.update(|_window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(encoded));
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        struct ThreadCopiedToast;
                        workspace.show_toast(
                            workspace::Toast::new(
                                workspace::notifications::NotificationId::unique::<ThreadCopiedToast>(),
                                "Thread copied to clipboard (base64 encoded)",
                            )
                            .autohide(),
                            cx,
                        );
                    });
                }
            })?;

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn load_thread_from_clipboard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(clipboard) = cx.read_from_clipboard() else {
            if let Some(workspace) = self.workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    struct NoClipboardToast;
                    workspace.show_toast(
                        workspace::Toast::new(
                            workspace::notifications::NotificationId::unique::<NoClipboardToast>(),
                            "No clipboard content available",
                        )
                        .autohide(),
                        cx,
                    );
                });
            }
            return;
        };

        let Some(encoded) = clipboard.text() else {
            if let Some(workspace) = self.workspace.upgrade() {
                workspace.update(cx, |workspace, cx| {
                    struct InvalidClipboardToast;
                    workspace.show_toast(
                        workspace::Toast::new(
                            workspace::notifications::NotificationId::unique::<InvalidClipboardToast>(),
                            "Clipboard does not contain text",
                        )
                        .autohide(),
                        cx,
                    );
                });
            }
            return;
        };

        let thread_data = match base64::Engine::decode(&base64::prelude::BASE64_STANDARD, &encoded)
        {
            Ok(data) => data,
            Err(_) => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        struct DecodeErrorToast;
                        workspace.show_toast(
                            workspace::Toast::new(
                                workspace::notifications::NotificationId::unique::<DecodeErrorToast>(),
                                "Failed to decode clipboard content (expected base64)",
                            )
                            .autohide(),
                            cx,
                        );
                    });
                }
                return;
            }
        };

        let shared_thread = match SharedThread::from_bytes(&thread_data) {
            Ok(thread) => thread,
            Err(_) => {
                if let Some(workspace) = self.workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        struct ParseErrorToast;
                        workspace.show_toast(
                            workspace::Toast::new(
                                workspace::notifications::NotificationId::unique::<ParseErrorToast>(
                                ),
                                "Failed to parse thread data from clipboard",
                            )
                            .autohide(),
                            cx,
                        );
                    });
                }
                return;
            }
        };

        let db_thread = shared_thread.to_db_thread();
        let session_id = acp::SessionId::new(uuid::Uuid::new_v4().to_string());
        let thread_store = self.thread_store.clone();
        let title = db_thread.title.clone();
        let workspace = self.workspace.clone();

        cx.spawn_in(window, async move |this, cx| {
            thread_store
                .update(&mut cx.clone(), |store, cx| {
                    store.save_thread(session_id.clone(), db_thread, cx)
                })
                .await?;

            let thread_metadata = acp_thread::AgentSessionInfo {
                session_id,
                cwd: None,
                title: Some(title),
                updated_at: Some(chrono::Utc::now()),
                meta: None,
            };

            this.update_in(cx, |this, window, cx| {
                this.open_thread(thread_metadata, window, cx);
            })?;

            this.update_in(cx, |_, _window, cx| {
                if let Some(workspace) = workspace.upgrade() {
                    workspace.update(cx, |workspace, cx| {
                        struct ThreadLoadedToast;
                        workspace.show_toast(
                            workspace::Toast::new(
                                workspace::notifications::NotificationId::unique::<ThreadLoadedToast>(),
                                "Thread loaded from clipboard",
                            )
                            .autohide(),
                            cx,
                        );
                    });
                }
            })?;

            anyhow::Ok(())
        })
        .detach_and_log_err(cx);
    }

    fn handle_agent_configuration_event(
        &mut self,
        _entity: &Entity<AgentConfiguration>,
        event: &AssistantConfigurationEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            AssistantConfigurationEvent::NewThread(provider) => {
                if LanguageModelRegistry::read_global(cx)
                    .default_model()
                    .is_none_or(|model| model.provider.id() != provider.id())
                    && let Some(model) = provider.default_model(cx)
                {
                    update_settings_file(self.fs.clone(), cx, move |settings, _| {
                        let provider = model.provider_id().0.to_string();
                        let model = model.id().0.to_string();
                        settings
                            .agent
                            .get_or_insert_default()
                            .set_model(LanguageModelSelection {
                                provider: LanguageModelProviderSetting(provider),
                                model,
                            })
                    });
                }

                self.new_thread(&NewThread, window, cx);
                if let Some((thread, model)) = self
                    .active_native_agent_thread(cx)
                    .zip(provider.default_model(cx))
                {
                    thread.update(cx, |thread, cx| {
                        thread.set_model(model, cx);
                    });
                }
            }
        }
    }

    pub(crate) fn active_agent_thread(&self, cx: &App) -> Option<Entity<AcpThread>> {
        match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view, .. } => {
                thread_view.read(cx).thread().cloned()
            }
            _ => None,
        }
    }

    pub(crate) fn active_native_agent_thread(&self, cx: &App) -> Option<Entity<agent::Thread>> {
        match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view, .. } => {
                thread_view.read(cx).as_native_thread(cx)
            }
            _ => None,
        }
    }

    pub(crate) fn active_text_thread_editor(&self) -> Option<Entity<TextThreadEditor>> {
        match self.active_view() {
            ActiveView::TextThread {
                text_thread_editor, ..
            } => Some(text_thread_editor.clone()),
            _ => None,
        }
    }

    fn populate_recently_opened_menu_section(
        mut menu: ContextMenu,
        panel: Entity<Self>,
        history_store: Entity<HistoryStore>,
        cx: &mut Context<ContextMenu>,
    ) -> ContextMenu {
        let entries = history_store
            .read(cx)
            .ordered_entries()
            .take(6)
            .collect::<Vec<_>>();

        if entries.is_empty() {
            return menu;
        }

        menu = menu.header("Recently Opened");

        for entry in entries {
            let label = entry.title.clone();
            let entry_id = entry.id.clone();
            menu = menu.entry(label, None, {
                let panel = panel.downgrade();
                move |window, cx| {
                    if let Some(panel) = panel.upgrade() {
                        panel.update(cx, |panel, cx| {
                            panel.open_history_entry(entry_id.clone(), window, cx);
                        });
                    }
                }
            });
        }

        menu.separator()
    }

    fn open_history_entry(
        &mut self,
        entry_id: agent::HistoryEntryId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match entry_id {
            agent::HistoryEntryId::AcpThread(session_id) => {
                self.load_agent_thread(
                    acp_thread::AgentSessionInfo {
                        session_id,
                        cwd: None,
                        title: None,
                        updated_at: None,
                        meta: None,
                    },
                    window,
                    cx,
                );
            }
            agent::HistoryEntryId::TextThread(path) => {
                self.open_saved_text_thread(path, window, cx)
                    .detach_and_log_err(cx);
            }
        }
    }

    fn focus_active_panel_thread(&self, window: &mut Window, cx: &mut Context<Self>) {
        match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view } => {
                thread_view.focus_handle(cx).focus(window);
            }
            ActiveView::TextThread {
                text_thread_editor, ..
            } => {
                text_thread_editor.focus_handle(cx).focus(window);
            }
            ActiveView::History | ActiveView::Configuration => {}
        }
        cx.notify();
    }

    fn focus_title_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.overlay_view.is_some()
            || self.title_edit_overlay_tab_id.is_some()
            || !matches!(
                self.tabs.get(self.active_tab_id).map(|tab| tab.view()),
                Some(ActiveView::ExternalAgentThread { .. } | ActiveView::TextThread { .. })
            )
        {
            return;
        }

        self.title_edit_overlay_tab_id = Some(self.active_tab_id);
        if let Some(tab) = self.tabs.get(self.active_tab_id) {
            match tab.view() {
                ActiveView::ExternalAgentThread { thread_view } => {
                    if let Some(editor) = thread_view.read(cx).title_editor() {
                        editor.focus_handle(cx).focus(window);
                    }
                }
                ActiveView::TextThread { title_editor, .. } => {
                    title_editor.focus_handle(cx).focus(window);
                }
                ActiveView::History | ActiveView::Configuration => {}
            }
        }
        cx.notify();
    }

    pub fn selected_agent(&self) -> AgentType {
        self.selected_agent.clone()
    }

    fn sync_agent_servers_from_extensions(&mut self, cx: &mut Context<Self>) {
        if let Some(extension_store) = ExtensionStore::try_global(cx) {
            let (manifests, extensions_dir) = {
                let store = extension_store.read(cx);
                let installed = store.installed_extensions();
                let manifests: Vec<_> = installed
                    .iter()
                    .map(|(id, entry)| (id.clone(), entry.manifest.clone()))
                    .collect();
                let extensions_dir = paths::extensions_dir().join("installed");
                (manifests, extensions_dir)
            };

            self.project.update(cx, |project, cx| {
                project.agent_server_store().update(cx, |store, cx| {
                    let manifest_refs: Vec<_> = manifests
                        .iter()
                        .map(|(id, manifest)| (id.as_ref(), manifest.as_ref()))
                        .collect();
                    store.sync_extension_agents(manifest_refs, extensions_dir, cx);
                });
            });
        }
    }

    pub fn new_agent_thread(
        &mut self,
        agent: AgentType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match agent {
            AgentType::TextThread => {
                self.new_text_thread(window, cx);
            }
            AgentType::NativeAgent => {
                let fs = self.fs.clone();
                let prompt_store = self.prompt_store.clone();
                let project = self.project.clone();
                let workspace = self.workspace.clone();

                let thread_view = cx.new(|cx| {
                    AcpThreadView::new(
                        Rc::new(agent::NativeAgentServer::new(
                            fs,
                            ThreadStore::global(cx),
                        )),
                        None,
                        None,
                        workspace,
                        project,
                        Some(ThreadStore::global(cx)),
                        prompt_store,
                        self.history_store.clone(),
                        window,
                        cx,
                    )
                });

                self.push_tab(
                    ActiveView::ExternalAgentThread { thread_view },
                    AgentType::NativeAgent,
                    window,
                    cx,
                );
            }
            AgentType::Gemini | AgentType::ClaudeCode | AgentType::Codex => {
                self.external_thread(agent.into(), None, None, window, cx);
            }
            AgentType::Custom { name, command } => {
                let server = Rc::new(agent_servers::CustomAgentServer::new(
                    name.clone(),
                    command.clone(),
                ));
                let thread_view = cx.new(|cx| {
                    AcpThreadView::new(
                        server,
                        None,
                        None,
                        self.workspace.clone(),
                        self.project.clone(),
                        None,
                        self.prompt_store.clone(),
                        self.history_store.clone(),
                        window,
                        cx,
                    )
                });

                self.push_tab(
                    ActiveView::ExternalAgentThread { thread_view },
                    AgentType::Custom { name, command },
                    window,
                    cx,
                );
            }
        }
    }

    pub fn load_agent_thread(
        &mut self,
        thread: acp_thread::AgentSessionInfo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.external_thread(self.selected_agent.clone().into(), Some(thread), None, window, cx);
    }

    fn external_thread(
        &mut self,
        agent: ExternalAgent,
        resume_thread: Option<acp_thread::AgentSessionInfo>,
        initial_content: Option<ExternalAgentInitialContent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let workspace = self.workspace.clone();
        let project = self.project.clone();
        let fs = self.fs.clone();

        let thread_store = self.thread_store.clone();
        let this = cx.entity().downgrade();
        cx.spawn_in(window, move |window, mut cx| async move {
            let server = agent.server(fs, thread_store);
            cx.update_in(&window, |window, cx| {
                let thread_view = cx.new(|cx| {
                    AcpThreadView::new(
                        server,
                        resume_thread,
                        initial_content,
                        workspace,
                        project,
                        None,
                        this.read(cx).prompt_store.clone(),
                        this.read(cx).history_store.clone(),
                        window,
                        cx,
                    )
                });

                this.update(cx, |this, cx| {
                    let selected_agent = agent.into();
                    if this.selected_agent != selected_agent {
                        this.selected_agent = selected_agent.clone();
                        this.serialize(cx);
                    }

                    this.push_tab(
                        ActiveView::ExternalAgentThread { thread_view },
                        selected_agent,
                        window,
                        cx,
                    );
                })
            })
        })
        .detach_and_log_err(cx);
    }

    fn delete_recently_open_thread(
        &mut self,
        action: &DeleteRecentlyOpenThread,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.history_store.update(cx, |store, cx| {
            store.remove_entry(action.id.clone(), cx).detach();
        });
    }

    fn activate_next_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }

        let next_id = if self.active_tab_id + 1 >= self.tabs.len() {
            0
        } else {
            self.active_tab_id + 1
        };

        self.set_active_tab_by_id(next_id, window, cx);
    }

    fn activate_previous_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            return;
        }

        let prev_id = if self.active_tab_id == 0 {
            self.tabs.len() - 1
        } else {
            self.active_tab_id - 1
        };

        self.set_active_tab_by_id(prev_id, window, cx);
    }

    pub fn close_active_thread_tab_or_dock(
        &mut self,
        _: &CloseActiveThreadTabOrDock,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.len() > 1 {
            self.remove_tab_by_id(self.active_tab_id, window, cx);
        } else if let Some(workspace) = self.workspace.upgrade() {
            window.defer(cx, move |window, cx| {
                workspace.update(cx, |workspace, cx| {
                    workspace.close_panel::<Self>(window, cx);
                });
            });
        }
    }
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view, .. } => thread_view.focus_handle(cx),
            ActiveView::History => self.acp_history.focus_handle(cx),
            ActiveView::TextThread {
                text_thread_editor, ..
            } => text_thread_editor.focus_handle(cx),
            ActiveView::Configuration => {
                if let Some(configuration) = self.configuration.as_ref() {
                    configuration.focus_handle(cx)
                } else {
                    self.panel_focus_handle.clone()
                }
            }
            ActiveView::Uninitialized => self.panel_focus_handle.clone(),
        }
    }
}

impl AgentPanel {
    fn active_tab(&self) -> &AgentPanelTab {
        self.tabs
            .get(self.active_tab_id)
            .unwrap_or_else(|| &self.tabs[0])
    }

    fn find_tab_by_identity(
        &self,
        identity: &AgentPanelTabIdentity,
        cx: &mut Context<Self>,
    ) -> Option<TabId> {
        for (index, tab) in self.tabs.iter().enumerate() {
            if Self::tab_view_identity(tab.view(), cx).is_some_and(|existing| existing == *identity)
            {
                return Some(index);
            }
        }
        None
    }

    fn set_active_tab_by_id(&mut self, new_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((tab_agent, text_thread_editor)) = self.tabs.get(new_id).map(|tab| {
            let editor = match tab.view() {
                ActiveView::TextThread {
                    text_thread_editor, ..
                } => Some(text_thread_editor.clone()),
                _ => None,
            };
            (tab.agent().clone(), editor)
        }) else {
            return;
        };

        self.overlay_view = None;
        self.overlay_previous_tab_id = None;
        self.title_edit_overlay_tab_id = None;
        self.active_tab_id = new_id;
        self.tab_bar_scroll_handle.scroll_to_item(new_id);

        if self.selected_agent != tab_agent {
            self.selected_agent = tab_agent.clone();
            self.serialize(cx);
        }

        if let Some(text_thread_editor) = text_thread_editor {
            self.history_store.update(cx, |store, cx| {
                if let Some(path) = text_thread_editor.read(cx).text_thread().read(cx).path() {
                    store.push_recently_opened_entry(
                        agent::HistoryEntryId::TextThread(path.clone()),
                        cx,
                    )
                }
            });
        }

        self.focus_handle(cx).focus(window);
    }

    fn set_tab_overlay_view(
        &mut self,
        view: ActiveView,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.title_edit_overlay_tab_id = None;
        self.overlay_previous_tab_id = Some(self.active_tab_id);
        self.overlay_view = Some(view);
        self.focus_handle(cx).focus(window);
    }

    fn push_tab(
        &mut self,
        new_view: ActiveView,
        agent: AgentType,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let tab_view_identity = Self::tab_view_identity(&new_view, cx);

        if let Some(identity) = tab_view_identity.as_ref() {
            if let Some(existing_id) = self.find_tab_by_identity(identity, cx) {
                self.set_active_tab_by_id(existing_id, window, cx);
                return;
            }
        }

        match &new_view {
            ActiveView::TextThread { .. } | ActiveView::ExternalAgentThread { .. } => {
                self.tabs.push(AgentPanelTab::new(new_view, agent));
                let new_id = self.tabs.len() - 1;
                self.set_active_tab_by_id(new_id, window, cx);

                if let Some(pending_id) = self.pending_tab_removal.take() {
                    if self.tabs.len() > 1 {
                        self.remove_tab_by_id(pending_id, window, cx);
                    } else {
                        self.pending_tab_removal = Some(pending_id);
                    }
                }
            }
            ActiveView::History | ActiveView::Configuration => {
                self.set_tab_overlay_view(new_view, window, cx);
            }
        }
    }

    fn remove_tab_by_id(&mut self, id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() == 1 {
            if self.loading && self.tabs.get(id).is_some() {
                self.pending_tab_removal = Some(id);
            }
            return;
        }

        if self.tabs.get(id).is_some() {
            let removed_id = id;
            self.tabs.remove(removed_id);
            let new_id = if self.active_tab_id == removed_id {
                removed_id.min(self.tabs.len() - 1)
            } else if self.active_tab_id > removed_id {
                self.active_tab_id - 1
            } else {
                self.active_tab_id
            };

            if let Some(edit_id) = self.title_edit_overlay_tab_id {
                if edit_id == removed_id {
                    self.title_edit_overlay_tab_id = None;
                } else if edit_id > removed_id {
                    self.title_edit_overlay_tab_id = Some(edit_id - 1);
                }
            }

            if new_id == self.active_tab_id {
                self.tab_bar_scroll_handle.scroll_to_item(new_id);
            } else {
                self.set_active_tab_by_id(new_id, window, cx);
            }
        }
    }

    fn display_tab_label(
        title: impl Into<SharedString>,
        is_active: bool,
    ) -> (SharedString, Option<SharedString>) {
        const MAX_CHARS: usize = 20;
        let title: SharedString = title.into();
        if is_active || title.chars().count() <= MAX_CHARS {
            (title, None)
        } else {
            let preview: String = title.chars().take(MAX_CHARS).collect();
            (format!("{preview}...").into(), Some(title))
        }
    }

    fn tab_view_identity(
        view: &ActiveView,
        cx: &mut Context<Self>,
    ) -> Option<AgentPanelTabIdentity> {
        match view {
            ActiveView::ExternalAgentThread { thread_view, .. } => thread_view
                .read(cx)
                .session_id(cx)
                .map(AgentPanelTabIdentity::AcpThread),
            ActiveView::TextThread {
                text_thread_editor, ..
            } => {
                let text_thread = {
                    let editor = text_thread_editor.read(cx);
                    editor.text_thread().clone()
                };
                text_thread
                    .read(cx)
                    .path()
                    .cloned()
                    .map(AgentPanelTabIdentity::TextThread)
            }
            ActiveView::History | ActiveView::Configuration => None,
        }
    }

    fn render_tab_label(
        &self,
        view: &ActiveView,
        is_active: bool,
        cx: &mut Context<Self>,
    ) -> TabLabelRender {
        match view {
            ActiveView::ExternalAgentThread { thread_view } => {
                let text = thread_view
                    .read(cx)
                    .title_editor()
                    .as_ref()
                    .map(|editor| editor.read(cx).text(cx))
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| thread_view.read(cx).title(cx).to_string().into());

                let (label_text, tooltip) = Self::display_tab_label(text, is_active);

                let is_generating = thread_view
                    .read(cx)
                    .thread()
                    .map(|thread| thread.read(cx).status() == ThreadStatus::Generating)
                    .unwrap_or(false);

                let label = Label::new(label_text)
                    .truncate()
                    .when(!is_active, |label| label.color(Color::Muted))
                    .into_any_element();

                TabLabelRender {
                    element: label,
                    tooltip,
                    is_generating,
                }
            }
            ActiveView::TextThread {
                title_editor,
                text_thread_editor,
                ..
            } => {
                let summary = text_thread_editor.read(cx).text_thread().read(cx).summary();

                let is_generating = text_thread_editor
                    .read(cx)
                    .text_thread()
                    .read(cx)
                    .messages(cx)
                    .any(|message| message.status == assistant_text_thread::MessageStatus::Pending);

                match summary {
                    TextThreadSummary::Pending => {
                        let label = Label::new(TextThreadSummary::DEFAULT)
                            .truncate()
                            .when(!is_active, |label| label.color(Color::Muted))
                            .into_any_element();

                        TabLabelRender {
                            element: label,
                            tooltip: None,
                            is_generating,
                        }
                    }
                    TextThreadSummary::Content(summary) => {
                        if summary.done {
                            let mut text = title_editor.read(cx).text(cx);
                            if text.is_empty() {
                                text = summary.text.clone().into();
                            }
                            let (label_text, tooltip) = Self::display_tab_label(text, is_active);

                            let label = Label::new(label_text)
                                .truncate()
                                .when(!is_active, |label| label.color(Color::Muted))
                                .into_any_element();

                            TabLabelRender {
                                element: label,
                                tooltip,
                                is_generating,
                            }
                        } else {
                            TabLabelRender {
                                element: Label::new(LOADING_SUMMARY_PLACEHOLDER)
                                    .truncate()
                                    .color(Color::Muted)
                                    .into_any_element(),
                                tooltip: None,
                                is_generating,
                            }
                        }
                    }
                    TextThreadSummary::Error => {
                        let text = title_editor.read(cx).text(cx);
                        let (label_text, tooltip) = Self::display_tab_label(text, is_active);

                        let label = Label::new(label_text)
                            .truncate()
                            .when(!is_active, |label| label.color(Color::Muted))
                            .into_any_element();

                        TabLabelRender {
                            element: label,
                            tooltip,
                            is_generating,
                        }
                    }
                }
            }
            ActiveView::History => TabLabelRender {
                element: Label::new("History").truncate().into_any_element(),
                tooltip: None,
                is_generating: false,
            },
            ActiveView::Configuration => TabLabelRender {
                element: Label::new("Settings").truncate().into_any_element(),
                tooltip: None,
                is_generating: false,
            },
            ActiveView::Uninitialized => TabLabelRender {
                element: Label::new("Agent").truncate().into_any_element(),
                tooltip: None,
                is_generating: false,
            },
        }
    }

    fn render_tab_agent_icon(
        &self,
        index: usize,
        agent: &AgentType,
        agent_server_store: &Entity<AgentServerStore>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let agent_label = agent.label();
        let tooltip_title = "Selected Agent";
        let agent_custom_icon = if let AgentType::Custom { name, .. } = agent {
            agent_server_store
                .read(cx)
                .agent_icon(&ExternalAgentServerName(name.clone()))
        } else {
            None
        };

        let has_custom_icon = agent_custom_icon.is_some();
        div()
            .id(("agent-tab-agent-icon", index))
            .when_some(agent_custom_icon, |this, icon_path| {
                let label = agent_label.clone();
                this.px(DynamicSpacing::Base02.rems(cx))
                    .child(Icon::from_path(icon_path).color(Color::Muted))
                    .tooltip(move |_window, cx| {
                        Tooltip::with_meta(label.clone(), None, tooltip_title, cx)
                    })
            })
            .when(!has_custom_icon, |this| {
                this.when_some(agent.icon(), |this, icon| {
                    let label = agent_label.clone();
                    this.px(DynamicSpacing::Base02.rems(cx))
                        .child(Icon::new(icon).color(Color::Muted))
                        .tooltip(move |_window, cx| {
                            Tooltip::with_meta(label.clone(), None, tooltip_title, cx)
                        })
                })
            })
            .into_any_element()
    }

    fn render_tab_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let agent_server_store = self.project.read(cx).agent_server_store().clone();
        let focus_handle = self.focus_handle(cx);

        let active_thread = match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view } => {
                thread_view.read(cx).as_native_thread(cx)
            }
            ActiveView::TextThread { .. } | ActiveView::History | ActiveView::Configuration => None,
        };

        let new_thread_menu_store = agent_server_store.clone();
        let new_thread_menu = PopoverMenu::new("new_thread_menu")
            .trigger_with_tooltip(
                IconButton::new("new_thread_menu_btn", IconName::Plus).icon_size(IconSize::Small),
                {
                    let focus_handle = focus_handle.clone();
                    move |_window, cx| {
                        Tooltip::for_action_in(
                            "New Thread…",
                            &ToggleNewThreadMenu,
                            &focus_handle,
                            cx,
                        )
                    }
                },
            )
            .anchor(Corner::TopRight)
            .with_handle(self.new_thread_menu_handle.clone())
            .menu({
                let workspace = self.workspace.clone();
                let is_via_collab = workspace
                    .update(cx, |workspace, cx| {
                        workspace.project().read(cx).is_via_collab()
                    })
                    .unwrap_or_default();

                move |window, cx| {
                    telemetry::event!("New Thread Clicked");

                    let active_thread = active_thread.clone();
                    Some(ContextMenu::build(window, cx, |menu, _window, cx| {
                        menu.context(focus_handle.clone())
                            .header("Zed Agent")
                            .when_some(active_thread, |this, active_thread| {
                                let thread = active_thread.read(cx);

                                if !thread.is_empty() {
                                    let session_id = thread.id().clone();
                                    this.item(
                                        ContextMenuEntry::new("New From Summary")
                                            .icon(IconName::ThreadFromSummary)
                                            .icon_color(Color::Muted)
                                            .handler(move |window, cx| {
                                                window.dispatch_action(
                                                    Box::new(crate::NewNativeAgentThreadFromSummary {
                                                        from_session_id: session_id.clone(),
                                                    }),
                                                    cx,
                                                );
                                            }),
                                    )
                                } else {
                                    this
                                }
                            })
                            .item(
                                ContextMenuEntry::new("New Thread")
                                    .action(NewThread.boxed_clone())
                                    .icon(IconName::Thread)
                                    .icon_color(Color::Muted)
                                    .handler({
                                        let workspace = workspace.clone();
                                        move |window, cx| {
                                            if let Some(workspace) = workspace.upgrade() {
                                                workspace.update(cx, |workspace, cx| {
                                                    if let Some(panel) =
                                                        workspace.panel::<AgentPanel>(cx)
                                                    {
                                                        panel.update(cx, |panel, cx| {
                                                            panel.new_agent_thread(
                                                                AgentType::NativeAgent,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                });
                                            }
                                        }
                                    }),
                            )
                            .item(
                                ContextMenuEntry::new("Text Thread")
                                    .action(NewTextThread.boxed_clone())
                                    .icon(IconName::TextThread)
                                    .icon_color(Color::Muted)
                                    .handler({
                                        let workspace = workspace.clone();
                                        move |window, cx| {
                                            if let Some(workspace) = workspace.upgrade() {
                                                workspace.update(cx, |workspace, cx| {
                                                    if let Some(panel) =
                                                        workspace.panel::<AgentPanel>(cx)
                                                    {
                                                        panel.update(cx, |panel, cx| {
                                                            panel.new_agent_thread(
                                                                AgentType::TextThread,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                });
                                            }
                                        }
                                    }),
                            )
                            .separator()
                            .header("External Agents")
                            .item(
                                ContextMenuEntry::new("Claude Code")
                                    .icon(IconName::AiClaude)
                                    .disabled(is_via_collab)
                                    .icon_color(Color::Muted)
                                    .handler({
                                        let workspace = workspace.clone();
                                        move |window, cx| {
                                            if let Some(workspace) = workspace.upgrade() {
                                                workspace.update(cx, |workspace, cx| {
                                                    if let Some(panel) =
                                                        workspace.panel::<AgentPanel>(cx)
                                                    {
                                                        panel.update(cx, |panel, cx| {
                                                            panel.new_agent_thread(
                                                                AgentType::ClaudeCode,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                });
                                            }
                                        }
                                    }),
                            )
                            .item(
                                ContextMenuEntry::new("Codex CLI")
                                    .icon(IconName::AiOpenAi)
                                    .disabled(is_via_collab)
                                    .icon_color(Color::Muted)
                                    .handler({
                                        let workspace = workspace.clone();
                                        move |window, cx| {
                                            if let Some(workspace) = workspace.upgrade() {
                                                workspace.update(cx, |workspace, cx| {
                                                    if let Some(panel) =
                                                        workspace.panel::<AgentPanel>(cx)
                                                    {
                                                        panel.update(cx, |panel, cx| {
                                                            panel.new_agent_thread(
                                                                AgentType::Codex,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                });
                                            }
                                        }
                                    }),
                            )
                            .item(
                                ContextMenuEntry::new("Gemini CLI")
                                    .icon(IconName::AiGemini)
                                    .icon_color(Color::Muted)
                                    .disabled(is_via_collab)
                                    .handler({
                                        let workspace = workspace.clone();
                                        move |window, cx| {
                                            if let Some(workspace) = workspace.upgrade() {
                                                workspace.update(cx, |workspace, cx| {
                                                    if let Some(panel) =
                                                        workspace.panel::<AgentPanel>(cx)
                                                    {
                                                        panel.update(cx, |panel, cx| {
                                                            panel.new_agent_thread(
                                                                AgentType::Gemini,
                                                                window,
                                                                cx,
                                                            );
                                                        });
                                                    }
                                                });
                                            }
                                        }
                                    }),
                            )
                            .map(|mut menu| {
                                let agent_server_store = new_thread_menu_store.read(cx);
                                let agent_names = agent_server_store
                                    .external_agents()
                                    .filter(|name| {
                                        name.0 != GEMINI_NAME
                                            && name.0 != CLAUDE_CODE_NAME
                                            && name.0 != CODEX_NAME
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>();

                                let custom_settings = AllAgentServersSettings::get_global(cx);
                                for agent_name in agent_names {
                                    let icon_path = agent_server_store.agent_icon(&agent_name);
                                    let display_name = agent_server_store
                                        .agent_display_name(&agent_name)
                                        .unwrap_or_else(|| agent_name.0.clone());

                                    let mut entry = ContextMenuEntry::new(display_name);

                                    if let Some(icon_path) = icon_path {
                                        entry = entry.custom_icon_svg(icon_path);
                                    } else {
                                        entry = entry.icon(IconName::Terminal);
                                    }
                                    entry = entry
                                        .icon_color(Color::Muted)
                                        .disabled(is_via_collab)
                                        .handler({
                                            let workspace = workspace.clone();
                                            let agent_name = agent_name.clone();
                                            let custom_settings = custom_settings.clone();
                                            move |window, cx| {
                                                if let Some(workspace) = workspace.upgrade() {
                                                    workspace.update(cx, |workspace, cx| {
                                                        if let Some(panel) =
                                                            workspace.panel::<AgentPanel>(cx)
                                                        {
                                                            panel.update(cx, |panel, cx| {
                                                                panel.new_agent_thread(
                                                                    AgentType::Custom {
                                                                        name: agent_name
                                                                            .clone()
                                                                            .into(),
                                                                        command: custom_settings
                                                                            .get(&agent_name.0)
                                                                            .map(|settings| {
                                                                                settings
                                                                                    .command
                                                                                    .clone()
                                                                            })
                                                                            .unwrap_or(
                                                                                placeholder_command(
                                                                                ),
                                                                            ),
                                                                    },
                                                                    window,
                                                                    cx,
                                                                );
                                                            });
                                                        }
                                                    });
                                                }
                                            }
                                        });
                                    menu = menu.item(entry);
                                }

                                menu
                            })
                            .separator()
                            .item(
                                ContextMenuEntry::new("Add More Agents")
                                    .icon(IconName::Plus)
                                    .icon_color(Color::Muted)
                                    .handler({
                                        move |window, cx| {
                                            window.dispatch_action(Box::new(zed_actions::Extensions {
                                                category_filter: Some(
                                                    zed_actions::ExtensionCategoryFilter::AgentServers,
                                                ),
                                                id: None,
                                            }), cx)
                                        }
                                    }),
                            )
                    }))
                }
            });

        let end_slot = h_flex()
            .gap(DynamicSpacing::Base02.rems(cx))
            .pl(DynamicSpacing::Base04.rems(cx))
            .pr(DynamicSpacing::Base06.rems(cx))
            .child(new_thread_menu)
            .child(self.render_recent_entries_menu(IconName::MenuAltTemp, Corner::TopRight, cx))
            .child(self.render_panel_options_menu(window, cx));

        let mut tab_bar = TabBar::new("agent-tab-bar")
            .track_scroll(self.tab_bar_scroll_handle.clone())
            .end_child(end_slot);

        if let Some(overlay_view) = &self.overlay_view {
            let TabLabelRender {
                element: overlay_label,
                ..
            } = self.render_tab_label(&overlay_view, true, cx);

            let overlay_title = h_flex()
                .flex_grow()
                .h(Tab::content_height(cx))
                .px(DynamicSpacing::Base04.px(cx))
                .gap(DynamicSpacing::Base04.rems(cx))
                .bg(cx.theme().colors().tab_bar_background)
                .child(self.render_toolbar_back_button(cx).into_any_element())
                .child(overlay_label)
                .into_any_element();

            return tab_bar.child(overlay_title).into_any_element();
        }

        if let Some(overlay_editor) = self.render_overlay_title_editor(cx) {
            return tab_bar.child(overlay_editor).into_any_element();
        }

        let active_index = self.active_tab_id;
        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == active_index;
            let position = if index == 0 {
                TabPosition::First
            } else if index == self.tabs.len() - 1 {
                TabPosition::Last
            } else {
                let ordering = if index < active_index {
                    Ordering::Less
                } else if index > active_index {
                    Ordering::Greater
                } else {
                    Ordering::Equal
                };
                TabPosition::Middle(ordering)
            };

            let TabLabelRender {
                element: tab_label,
                tooltip,
                is_generating,
            } = self.render_tab_label(tab.view(), is_active, cx);

            let indicator = is_generating.then(|| Indicator::dot().color(Color::Accent));
            let agent_icon =
                self.render_tab_agent_icon(index, tab.agent(), &agent_server_store, cx);
            let start_slot = h_flex()
                .gap(DynamicSpacing::Base04.rems(cx))
                .children(indicator)
                .child(agent_icon);

            let mut tab_component = Tab::new(("agent-tab", index))
                .position(position)
                .close_side(TabCloseSide::End)
                .toggle_state(is_active)
                .on_click(cx.listener(move |this: &mut Self, _, window, cx| {
                    if is_active {
                        this.focus_title_editor(window, cx);
                    } else {
                        this.set_active_tab_by_id(index, window, cx);
                    }
                }))
                .child(tab_label)
                .start_slot(start_slot)
                .end_slot(
                    IconButton::new(("close-agent-tab", index), IconName::Close)
                        .shape(IconButtonShape::Square)
                        .icon_size(IconSize::Small)
                        .visible_on_hover("")
                        .on_click(cx.listener(move |this: &mut Self, _, window, cx| {
                            this.remove_tab_by_id(index, window, cx);
                        }))
                        .tooltip(|_window, cx| cx.new(|_| Tooltip::new("Close Thread")).into()),
                );

            if let Some(tooltip_text) = tooltip {
                tab_component = tab_component.tooltip(Tooltip::text(tooltip_text));
            }
            tab_bar = tab_bar.child(tab_component);
        }

        tab_bar.into_any_element()
    }
}

fn agent_panel_dock_position(cx: &App) -> DockPosition {
    AgentSettings::get_global(cx).dock.into()
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Panel for AgentPanel {
    fn persistent_name() -> &'static str {
        "AgentPanel"
    }

    fn panel_key() -> &'static str {
        AGENT_PANEL_KEY
    }

    fn position(&self, _window: &Window, cx: &App) -> DockPosition {
        agent_panel_dock_position(cx)
    }

    fn position_is_valid(&self, position: DockPosition) -> bool {
        position != DockPosition::Bottom
    }

    fn set_position(&mut self, position: DockPosition, _: &mut Window, cx: &mut Context<Self>) {
        update_settings_file(self.fs.clone(), cx, move |settings, _| {
            settings
                .agent
                .get_or_insert_default()
                .set_dock(position.into());
        });
    }

    fn size(&self, window: &Window, cx: &App) -> Pixels {
        let settings = AgentSettings::get_global(cx);
        match self.position(window, cx) {
            DockPosition::Left | DockPosition::Right => {
                self.width.unwrap_or(settings.default_width)
            }
            DockPosition::Bottom => self.height.unwrap_or(settings.default_height),
        }
    }

    fn set_size(&mut self, size: Option<Pixels>, window: &mut Window, cx: &mut Context<Self>) {
        match self.position(window, cx) {
            DockPosition::Left | DockPosition::Right => self.width = size,
            DockPosition::Bottom => self.height = size,
        }
        self.serialize(cx);
        cx.notify();
    }

    fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
        if active && matches!(self.active_view(), ActiveView::Uninitialized) {
            let selected_agent = self.selected_agent.clone();
            self.new_agent_thread(selected_agent, window, cx);
        }
    }

    fn remote_id() -> Option<proto::PanelId> {
        Some(proto::PanelId::AssistantPanel)
    }

    fn icon(&self, _window: &Window, cx: &App) -> Option<IconName> {
        (self.enabled(cx) && AgentSettings::get_global(cx).button).then_some(IconName::ZedAssistant)
    }

    fn icon_tooltip(&self, _window: &Window, _cx: &App) -> Option<&'static str> {
        Some("Agent Panel")
    }

    fn toggle_action(&self) -> Box<dyn Action> {
        Box::new(ToggleFocus)
    }

    fn activation_priority(&self) -> u32 {
        3
    }

    fn enabled(&self, cx: &App) -> bool {
        AgentSettings::get_global(cx).enabled(cx)
    }

    fn is_zoomed(&self, _window: &Window, _cx: &App) -> bool {
        self.zoomed
    }

    fn set_zoomed(&mut self, zoomed: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoomed = zoomed;
        cx.notify();
    }
}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_flex()
            .relative()
            .size_full()
            .justify_between()
            .key_context(self.key_context())
            .track_focus(&self.panel_focus_handle)
            .on_action(cx.listener(|this, action: &NewThread, window, cx| {
                this.new_thread(action, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenHistory, window, cx| {
                this.open_history(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_configuration(window, cx);
            }))
            .on_action(cx.listener(Self::open_active_thread_as_markdown))
            .on_action(cx.listener(|this, action: &crate::rules_library::OpenRulesLibrary, window, cx| {
                this.deploy_rules_library(action, window, cx)
            }))
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(Self::toggle_navigation_menu))
            .on_action(cx.listener(Self::toggle_options_menu))
            .on_action(cx.listener(|this, _: &CloseActiveThreadTab, window, cx| {
                this.remove_tab_by_id(this.active_tab_id, window, cx);
            }))
            .on_action(cx.listener(|this, action: &CloseActiveThreadTabOrDock, window, cx| {
                this.close_active_thread_tab_or_dock(action, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ActivateNextTab, window, cx| {
                this.activate_next_tab(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ActivatePreviousTab, window, cx| {
                this.activate_previous_tab(window, cx);
            }))
            .on_action(cx.listener(Self::increase_font_size))
            .on_action(cx.listener(Self::decrease_font_size))
            .on_action(cx.listener(Self::reset_font_size))
            .on_action(cx.listener(Self::toggle_zoom))
            .on_action(cx.listener(|this, _: &ReauthenticateAgent, window, cx| {
                if let Some(thread_view) = this.active_thread_view() {
                    thread_view.update(cx, |thread_view, cx| thread_view.reauthenticate(window, cx))
                }
            }))
            .child(self.render_tab_bar(window, cx))
            .children(self.render_onboarding(window, cx))
            .map(|parent| match self.active_view() {
                ActiveView::ExternalAgentThread { thread_view, .. } => parent
                    .child(thread_view.clone())
                    .child(self.render_drag_target(cx)),
                ActiveView::History => parent.child(self.acp_history.clone()),
                ActiveView::TextThread {
                    text_thread_editor,
                    buffer_search_bar,
                    ..
                } => {
                    let model_registry = LanguageModelRegistry::read_global(cx);
                    let configuration_error =
                        model_registry.configuration_error(model_registry.default_model(), cx);
                    parent
                        .map(|this| {
                            if !self.should_render_onboarding(cx)
                                && let Some(err) = configuration_error.as_ref()
                            {
                                this.child(self.render_configuration_error(
                                    true,
                                    err,
                                    &self.focus_handle(cx),
                                    cx,
                                ))
                            } else {
                                this
                            }
                        })
                        .child(self.render_text_thread(
                            text_thread_editor,
                            buffer_search_bar,
                            window,
                            cx,
                        ))
                }
                ActiveView::Configuration => parent.children(self.configuration.clone()),
                ActiveView::Uninitialized => parent,
            })
            .children(self.render_trial_end_upsell(window, cx));

        match self.active_view().which_font_size_used() {
            WhichFontSize::AgentFont => {
                WithRemSize::new(ThemeSettings::get_global(cx).agent_ui_font_size(cx))
                    .size_full()
                    .child(content)
                    .into_any()
            }
            _ => content.into_any(),
        }
    }
}

struct PromptLibraryInlineAssist {
    workspace: WeakEntity<Workspace>,
}

impl PromptLibraryInlineAssist {
    pub fn new(workspace: WeakEntity<Workspace>) -> Self {
        Self { workspace }
    }
}

impl rules_library::InlineAssistDelegate for PromptLibraryInlineAssist {
    fn assist(
        &self,
        prompt_editor: &Entity<Editor>,
        initial_prompt: Option<String>,
        window: &mut Window,
        cx: &mut Context<RulesLibrary>,
    ) {
        InlineAssistant::update_global(cx, |assistant, cx| {
            let Some(workspace) = self.workspace.upgrade() else {
                return;
            };
            let Some(panel) = workspace.read(cx).panel::<AgentPanel>(cx) else {
                return;
            };
            let project = workspace.read(cx).project().downgrade();
            let panel = panel.read(cx);
            let thread_store = panel.thread_store().clone();
            let history = panel.history().downgrade();
            assistant.assist(
                prompt_editor,
                self.workspace.clone(),
                project,
                thread_store,
                None,
                history,
                initial_prompt,
                window,
                cx,
            );
        })
    }

    fn focus_agent_panel(
        &self,
        workspace: &mut Workspace,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> bool {
        workspace.focus_panel::<AgentPanel>(window, cx).is_some()
    }
}

pub struct ConcreteAssistantPanelDelegate;

impl AgentPanelDelegate for ConcreteAssistantPanelDelegate {
    fn active_text_thread_editor(
        &self,
        workspace: &mut Workspace,
        _window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Option<Entity<TextThreadEditor>> {
        let panel = workspace.panel::<AgentPanel>(cx)?;
        panel.read(cx).active_text_thread_editor()
    }

    fn open_local_text_thread(
        &self,
        workspace: &mut Workspace,
        path: Arc<Path>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) -> Task<Result<()>> {
        let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
            return Task::ready(Err(anyhow!("Agent panel not found")));
        };

        panel.update(cx, |panel, cx| {
            panel.open_saved_text_thread(path, window, cx)
        })
    }

    fn open_remote_text_thread(
        &self,
        _workspace: &mut Workspace,
        _text_thread_id: assistant_text_thread::TextThreadId,
        _window: &mut Window,
        _cx: &mut Context<Workspace>,
    ) -> Task<Result<Entity<TextThreadEditor>>> {
        Task::ready(Err(anyhow!("opening remote context not implemented")))
    }

    fn quote_selection(
        &self,
        workspace: &mut Workspace,
        selection_ranges: Vec<Range<Anchor>>,
        buffer: Entity<MultiBuffer>,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
            return;
        };

        if !panel.focus_handle(cx).contains_focused(window, cx) {
            workspace.toggle_panel_focus::<AgentPanel>(window, cx);
        }

        panel.update(cx, |_, cx| {
            cx.defer_in(window, move |panel, window, cx| {
                if let Some(thread_view) = panel.active_thread_view() {
                    thread_view.update(cx, |thread_view, cx| {
                        thread_view.insert_selections(window, cx);
                    });
                } else if let Some(text_thread_editor) = panel.active_text_thread_editor() {
                    let snapshot = buffer.read(cx).snapshot(cx);
                    let selection_ranges = selection_ranges
                        .into_iter()
                        .map(|range| range.to_point(&snapshot))
                        .collect::<Vec<_>>();

                    text_thread_editor.update(cx, |text_thread_editor, cx| {
                        text_thread_editor.quote_ranges(selection_ranges, snapshot, window, cx)
                    });
                }
            });
        });
    }

    fn quote_terminal_text(
        &self,
        workspace: &mut Workspace,
        text: String,
        window: &mut Window,
        cx: &mut Context<Workspace>,
    ) {
        let Some(panel) = workspace.panel::<AgentPanel>(cx) else {
            return;
        };

        if !panel.focus_handle(cx).contains_focused(window, cx) {
            workspace.toggle_panel_focus::<AgentPanel>(window, cx);
        }

        panel.update(cx, |_, cx| {
            cx.defer_in(window, move |panel, window, cx| {
                if let Some(thread_view) = panel.active_thread_view() {
                    thread_view.update(cx, |thread_view, cx| {
                        thread_view.insert_terminal_text(text, window, cx);
                    });
                } else if let Some(text_thread_editor) = panel.active_text_thread_editor() {
                    text_thread_editor.update(cx, |text_thread_editor, cx| {
                        text_thread_editor.quote_terminal_text(text, window, cx)
                    });
                }
            });
        });
    }
}

struct OnboardingUpsell;

impl Dismissable for OnboardingUpsell {
    const KEY: &'static str = "dismissed-trial-upsell";
}

struct TrialEndUpsell;

impl Dismissable for TrialEndUpsell {
    const KEY: &'static str = "dismissed-trial-end-upsell";
}

#[cfg(feature = "test-support")]
impl AgentPanel {
    pub fn open_external_thread_with_server(
        &mut self,
        server: Rc<dyn agent_servers::AgentServer>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let thread_view = cx.new(|cx| {
            AcpThreadView::new(
                server.clone(),
                None,
                None,
                self.workspace.clone(),
                self.project.clone(),
                None,
                self.prompt_store.clone(),
                self.history_store.clone(),
                window,
                cx,
            )
        });

        self.push_tab(
            ActiveView::ExternalAgentThread { thread_view },
            AgentType::Custom {
                name: server.name(),
                command: placeholder_command(),
            },
            window,
            cx,
        );
    }

    pub fn active_thread_view_for_tests(&self) -> Option<&Entity<AcpThreadView>> {
        self.active_thread_view()
    }
}
