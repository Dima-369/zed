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
    LanguageModelProviderSetting, LanguageModelSelection, Settings, SettingsStore,
    update_settings_file,
};

use zed_actions::agent::{OpenClaudeCodeOnboardingModal, ReauthenticateAgent};

use crate::agent_panel_tab::{AgentPanelTab, AgentPanelTabIdentity, TabId, TabLabelRender};
use crate::ui::{AcpOnboardingModal, ClaudeCodeOnboardingModal};
use crate::{
    AddContextServer, AgentDiffPane, CloseActiveThreadTab, DeleteRecentlyOpenThread, Follow,
    InlineAssistant, NewTextThread, NewThread, OpenActiveThreadAsMarkdown, OpenAgentDiff,
    OpenHistory, ResetTrialEndUpsell, ResetTrialUpsell, ToggleNavigationMenu, ToggleNewThreadMenu,
    ToggleOptionsMenu,
    acp::AcpThreadView,
    agent_configuration::{AgentConfiguration, AssistantConfigurationEvent},
    slash_command::SlashCommandCompletionProvider,
    text_thread_editor::{AgentPanelDelegate, TextThreadEditor, make_lsp_adapter_delegate},
    ui::{AgentOnboardingModal, EndTrialUpsell},
};
use crate::{
    ExpandMessageEditor,
    acp::{AcpThreadHistory, ThreadHistoryEvent},
    text_thread_history::{TextThreadHistory, TextThreadHistoryEvent},
};
use crate::{
    ExternalAgent, ExternalAgentInitialContent, NewExternalAgentThread,
    NewNativeAgentThreadFromSummary,
};
use agent_settings::AgentSettings;
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
                .register_action(
                    |workspace, action: &NewNativeAgentThreadFromSummary, window, cx| {
                        if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                            panel.update(cx, |panel, cx| {
                                panel.new_native_agent_thread_from_summary(action, window, cx)
                            });
                            workspace.focus_panel::<AgentPanel>(window, cx);
                        }
                    },
                )
                .register_action(
                    |workspace, action: &NewNativeAgentThreadFromSession, window, cx| {
                        if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                            panel.update(cx, |panel, cx| {
                                panel.new_native_agent_thread_from_session(action, window, cx)
                            });
                            workspace.focus_panel::<AgentPanel>(window, cx);
                        }
                    },
                )
                .register_action(|workspace, action: &NewTextThread, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.new_text_thread(action, window, cx));
                        workspace.focus_panel::<AgentPanel>(window, cx);
                    }
                })
                .register_action(|workspace, action: &NewExternalAgentThread, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| {
                            panel.new_external_agent_thread(action, window, cx)
                        });
                        workspace.focus_panel::<AgentPanel>(window, cx);
                    }
                })
                .register_action(|workspace, _: &OpenHistory, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.open_history(window, cx));
                        workspace.focus_panel::<AgentPanel>(window, cx);
                    }
                })
                .register_action(|workspace, _: &ToggleNavigationMenu, _window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.toggle_navigation_menu(cx));
                    }
                })
                .register_action(|workspace, _: &ToggleNewThreadMenu, _window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.toggle_new_thread_menu(cx));
                    }
                })
                .register_action(|workspace, _: &ToggleOptionsMenu, _window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.toggle_options_menu(cx));
                    }
                })
                .register_action(|workspace, _: &crate::ActivateNextTab, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.activate_next_tab(window, cx));
                    }
                })
                .register_action(|workspace, _: &crate::ActivatePreviousTab, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.activate_previous_tab(window, cx));
                    }
                })
                .register_action(|workspace, action: &crate::CloseActiveThreadTabOrDock, window, cx| {
                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                        panel.update(cx, |panel, cx| panel.close_active_thread_tab_or_dock(action, window, cx));
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
        _subscriptions: Vec<gpui::Subscription>,
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
    GeminiCode,
    ClaudeCode,
    Codex,
    Custom {
        name: SharedString,
        command: AgentServerCommand,
    },
}

impl AgentType {
    pub fn label(&self) -> SharedString {
        match self {
            Self::NativeAgent | Self::TextThread => "Zed Agent".into(),
            Self::GeminiCode => "Gemini CLI".into(),
            Self::ClaudeCode => "Claude Code".into(),
            Self::Codex => "Codex".into(),
            Self::Custom { name, .. } => name.into(),
        }
    }

    pub fn icon(&self) -> Option<IconName> {
        match self {
            Self::NativeAgent | Self::TextThread => None,
            Self::GeminiCode => Some(IconName::AiGemini),
            Self::ClaudeCode => Some(IconName::AiClaude),
            Self::Codex => Some(IconName::AiOpenAi),
            Self::Custom { .. } => Some(IconName::Sparkle),
        }
    }
}

impl From<ExternalAgent> for AgentType {
    fn from(value: ExternalAgent) -> Self {
        match value {
            ExternalAgent::Gemini => Self::GeminiCode,
            ExternalAgent::ClaudeCode => Self::ClaudeCode,
            ExternalAgent::Codex => Self::Codex,
            ExternalAgent::Custom { name } => Self::Custom {
                name: name.into(),
                command: placeholder_command(),
            },
            ExternalAgent::NativeAgent => Self::NativeAgent,
        }
    }
}

fn placeholder_command() -> AgentServerCommand {
    AgentServerCommand {
        command: "false".into(),
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
        prompt_store: Entity<PromptStore>,
        history_store: Entity<HistoryStore>,
        project: Entity<Project>,
        workspace: WeakEntity<Workspace>,
        window: &mut Window,
        cx: &mut App,
    ) -> Self {
        let thread_view = cx.new(|cx| {
            AcpThreadView::native_agent(fs, prompt_store, history_store, project, workspace, window, cx)
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

        let editor = cx.new(|cx| {
            let mut editor = Editor::single_line(window, cx);
            editor.set_text(title, window, cx);
            editor
        });

        let mut suppress_first_edit = true;

        let subscriptions = vec![
            window.subscribe(&editor, cx, {
                let text_thread_editor = text_thread_editor.clone();
                move |editor, event, _window, cx| match event {
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

                            editor.update(cx, |editor, window, cx| {
                                editor.set_text(summary, window, cx);
                            });
                        }
                    }
                    _ => {}
                }
            }),
            window.subscribe(&text_thread_editor.read(cx).text_thread().clone(), cx, {
                let editor = editor.clone();
                move |text_thread, event, window, cx| match event {
                    TextThreadEvent::SummaryGenerated => {
                        let summary = text_thread.read(cx).summary().or_default();

                        editor.update(cx, |editor, window, cx| {
                            editor.set_text(summary, window, cx);
                        })
                    }
                    _ => {}
                }
            }),
        ];

        let buffer_search_bar =
            cx.new(|cx| BufferSearchBar::new(Some(language_registry), window, cx));
        buffer_search_bar.update(cx, |buffer_search_bar, cx| {
            buffer_search_bar.set_active_pane_item(Some(&text_thread_editor), window, cx)
        });

        Self::TextThread {
            text_thread_editor,
            title_editor: editor,
            buffer_search_bar,
            _subscriptions: subscriptions,
        }
    }
}

pub struct AgentPanel {
    workspace: WeakEntity<Workspace>,
    project: Entity<Project>,
    fs: Arc<dyn Fs>,
    user_store: Entity<UserStore>,
    history_store: Entity<HistoryStore>,
    prompt_store: Entity<PromptStore>,
    language_registry: Arc<LanguageRegistry>,
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
    loading: bool,
    acp_history: Entity<AcpThreadHistory>,
    text_thread_history: Entity<TextThreadHistory>,
    onboarding: Entity<AgentPanelOnboarding>,
    selected_agent: AgentType,
    detached_threads: HashMap<acp::SessionId, DetachedThread>,
    pending_tab_removal: Option<TabId>,
    tabs: Vec<AgentPanelTab>,
    active_tab_id: TabId,
    tab_bar_scroll_handle: ScrollHandle,
    title_edit_overlay_tab_id: Option<TabId>,
}

#[derive(Clone)]
struct DetachedThread {
    thread_view: Entity<AcpThreadView>,
}

impl AgentPanel {
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
                    workspace.project().read(cx).text_thread_store().clone()
                })?;

            let history_store = workspace
                .update(cx, |workspace, cx| {
                    workspace.project().read(cx).history_store().clone()
                })?;

            let acp_history = cx.update(|_window, cx| {
                cx.new(|cx| AcpThreadHistory::new(history_store.clone(), cx))
            })?;

            let text_thread_history = cx.update(|_window, cx| {
                cx.new(|cx| TextThreadHistory::new(history_store.clone(), cx))
            })?;

            let project = workspace
                .update(cx, |workspace, _cx| workspace.project().clone())?;

            let user_store = cx.update(|_window, cx| UserStore::global(cx))?;

            let inline_assist_context_store = cx.update(|_window, cx| {
                cx.new(|cx| ContextStore::new(project.clone(), cx))
            })?;

            let fs = project.update(cx, |project, _cx| project.fs().clone())?;
            let language_registry = project.update(cx, |project, _cx| project.languages().clone())?;

            let panel = cx.update(|window, cx| {
                let panel = cx.new(|cx| {
                    Self::new(
                        workspace,
                        project,
                        fs,
                        user_store,
                        history_store,
                        prompt_store.unwrap(),
                        language_registry,
                        acp_history,
                        text_thread_history,
                        text_thread_store,
                        inline_assist_context_store,
                        window,
                        cx,
                    )
                });

                panel.update(cx, |panel, window, cx| {
                    panel.loading = true;
                    if let Some(serialized_panel) = serialized_panel {
                        panel.width = serialized_panel.width;
                        if let Some(selected_agent) = serialized_panel.selected_agent {
                            panel.selected_agent = selected_agent.clone();
                            panel.new_agent_thread(selected_agent, window, cx);
                            log::info!("Restore the default panel from serialized panel.");
                            panel.remove_tab_by_id(0, window, cx);
                        }
                        cx.notify();
                    }
                    panel.loading = false;
                });
                panel
            })?;

            Ok(panel)
        })
    }

    fn new(
        workspace: WeakEntity<Workspace>,
        project: Entity<Project>,
        fs: Arc<dyn Fs>,
        user_store: Entity<UserStore>,
        history_store: Entity<HistoryStore>,
        prompt_store: Entity<PromptStore>,
        language_registry: Arc<LanguageRegistry>,
        acp_history: Entity<AcpThreadHistory>,
        text_thread_history: Entity<TextThreadHistory>,
        text_thread_store: Entity<assistant_text_thread::TextThreadStore>,
        inline_assist_context_store: Entity<ContextStore>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let context_server_registry = project.read(cx).context_server_registry().clone();
        let onboarding = cx.new(|cx| AgentPanelOnboarding::new(cx));

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

        let panel_focus_handle = cx.focus_handle();
        cx.on_focus_in(&panel_focus_handle, window, |_, _, cx| {
            cx.notify();
        })
        .detach();
        cx.on_focus_out(&panel_focus_handle, window, |_, _, _, cx| {
            cx.notify();
        })
        .detach();

        Self {
            overlay_view: None,
            workspace,
            user_store,
            project: project.clone(),
            fs: fs.clone(),
            history_store,
            prompt_store,
            language_registry,
            acp_history,
            text_thread_history,
            configuration: None,
            configuration_subscription: None,
            context_server_registry,
            inline_assist_context_store,
            overlay_previous_tab_id: None,
            new_thread_menu_handle: PopoverMenuHandle::default(),
            agent_panel_menu_handle: PopoverMenuHandle::default(),
            agent_navigation_menu_handle: PopoverMenuHandle::default(),
            agent_navigation_menu: None,
            _extension_subscription: None,
            width: None,
            height: None,
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
        }
    }

    fn serialize(&mut self, cx: &mut Context<Self>) {
        let width = self.width;
        let selected_agent = self.selected_agent.clone();
        cx.background_spawn(async move {
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
        })
        .detach();
    }

    fn active_view(&self) -> &ActiveView {
        self.overlay_view
            .as_ref()
            .unwrap_or_else(|| self.active_tab().view())
    }

    fn active_thread_view(&self) -> Option<&Entity<AcpThreadView>> {
        match self.active_view() {
            ActiveView::ExternalAgentThread { thread_view, .. } => Some(thread_view),
            ActiveView::TextThread { .. } | ActiveView::History | ActiveView::Configuration => None,
            ActiveView::Uninitialized => None,
        }
    }

    fn new_thread(&mut self, _action: &NewThread, window: &mut Window, cx: &mut Context<Self>) {
        self.new_agent_thread(AgentType::NativeAgent, window, cx);
    }

    fn new_agent_thread(&mut self, agent_type: AgentType, window: &mut Window, cx: &mut Context<Self>) {
        match agent_type {
            AgentType::NativeAgent => {
                let view = ActiveView::native_agent(
                    self.fs.clone(),
                    self.prompt_store.clone(),
                    self.history_store.clone(),
                    self.project.clone(),
                    self.workspace.clone(),
                    window,
                    cx,
                );
                self.push_tab(view, agent_type, window, cx);
            }
            AgentType::TextThread => {
                self.new_text_thread(&NewTextThread, window, cx);
            }
            _ => {
                // Handle external agents if necessary
            }
        }
    }

    fn new_text_thread(&mut self, _action: &NewTextThread, window: &mut Window, cx: &mut Context<Self>) {
        let context = self.project.read(cx).text_thread_store().update(cx, |store, cx| store.create(cx));
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
            ActiveView::Uninitialized => {}
        }
    }

    fn set_active_tab_by_id(&mut self, new_id: TabId, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(new_id) {
            let tab_agent = tab.agent().clone();
            self.overlay_view = None;
            self.overlay_previous_tab_id = None;
            self.title_edit_overlay_tab_id = None;
            self.active_tab_id = new_id;
            self.tab_bar_scroll_handle.scroll_to_item(new_id);

            if self.selected_agent != tab_agent {
                self.selected_agent = tab_agent.clone();
                self.serialize(cx);
            }
            self.focus_handle(cx).focus(window);
        }
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
                let text_thread = text_thread_editor.read(cx).text_thread().clone();
                text_thread
                    .read(cx)
                    .path()
                    .cloned()
                    .map(AgentPanelTabIdentity::TextThread)
            }
            ActiveView::History | ActiveView::Configuration | ActiveView::Uninitialized => None,
        }
    }

    fn active_tab(&self) -> &AgentPanelTab {
        self.tabs.get(self.active_tab_id).unwrap_or(&self.tabs[0])
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

    fn go_back(&mut self, _: &workspace::GoBack, window: &mut Window, cx: &mut Context<Self>) {
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
        cx.notify();
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
            _ => {}
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
                _ => {}
            }
        }
        cx.notify();
    }

    fn key_context(&self) -> KeyContext {
        let mut key_context = KeyContext::new_with_defaults();
        key_context.add("AgentPanel");
        match self.active_view() {
            ActiveView::ExternalAgentThread { .. } => key_context.add("acp_thread"),
            ActiveView::TextThread { .. } => key_context.add("text_thread"),
            _ => {}
        }
        key_context
    }

    fn render_tab_label(
        &self,
        view: &ActiveView,
        is_active: bool,
        cx: &mut Context<Self>,
    ) -> TabLabelRender {
        match view {
            ActiveView::ExternalAgentThread { thread_view } => {
                let text = thread_view.read(cx).title(cx).to_string().into();
                let (label_text, tooltip) = Self::display_tab_label(text, is_active);
                let is_generating = thread_view
                    .read(cx)
                    .thread()
                    .map(|thread| thread.read(cx).status() == ThreadStatus::Generating)
                    .unwrap_or(false);

                TabLabelRender {
                    element: Label::new(label_text).truncate().when(!is_active, |l| l.color(Color::Muted)).into_any_element(),
                    tooltip,
                    is_generating,
                }
            }
            ActiveView::TextThread { title_editor, text_thread_editor, .. } => {
                let text = title_editor.read(cx).text(cx).into();
                let (label_text, tooltip) = Self::display_tab_label(text, is_active);
                let is_generating = text_thread_editor.read(cx).text_thread().read(cx).status() == ThreadStatus::Generating;

                TabLabelRender {
                    element: Label::new(label_text).truncate().when(!is_active, |l| l.color(Color::Muted)).into_any_element(),
                    tooltip,
                    is_generating,
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

    fn display_tab_label(
        title: SharedString,
        is_active: bool,
    ) -> (SharedString, Option<SharedString>) {
        const MAX_CHARS: usize = 20;
        if is_active || title.chars().count() <= MAX_CHARS {
            (title, None)
        } else {
            let preview: String = title.chars().take(MAX_CHARS).collect();
            (format!("{preview}...").into(), Some(title))
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
        let agent_custom_icon = if let AgentType::Custom { name, .. } = agent {
            agent_server_store.read(cx).agent_icon(&ExternalAgentServerName(name.to_string()))
        } else {
            None
        };

        div()
            .id(("agent-tab-agent-icon", index))
            .when_some(agent_custom_icon, |this, icon_path| {
                this.px(DynamicSpacing::Base02.rems(cx))
                    .child(Icon::from_path(icon_path).color(Color::Muted))
            })
            .when(agent_custom_icon.is_none(), |this| {
                this.when_some(agent.icon(), |this, icon| {
                    this.px(DynamicSpacing::Base02.rems(cx))
                        .child(Icon::new(icon).color(Color::Muted))
                })
            })
            .into_any_element()
    }

    fn render_tab_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let agent_server_store = self.project.read(cx).agent_server_store().clone();
        let mut tab_bar = TabBar::new("agent-tab-bar")
            .track_scroll(self.tab_bar_scroll_handle.clone());

        for (index, tab) in self.tabs.iter().enumerate() {
            let is_active = index == self.active_tab_id;
            let TabLabelRender { element, tooltip, is_generating } = self.render_tab_label(tab.view(), is_active, cx);
            let indicator = is_generating.then(|| Indicator::dot().color(Color::Accent));
            let agent_icon = self.render_tab_agent_icon(index, tab.agent(), &agent_server_store, cx);
            let start_slot = h_flex().gap(DynamicSpacing::Base04.rems(cx)).children(indicator).child(agent_icon);

            let mut tab_component = Tab::new(("agent-tab", index))
                .toggle_state(is_active)
                .on_click(cx.listener(move |this, _, window, cx| {
                    if is_active {
                        this.focus_title_editor(window, cx);
                    } else {
                        this.set_active_tab_by_id(index, window, cx);
                    }
                }))
                .child(element)
                .start_slot(start_slot)
                .end_slot(
                    IconButton::new(("close-agent-tab", index), IconName::Close)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.remove_tab_by_id(index, window, cx);
                        }))
                );

            if let Some(tooltip_text) = tooltip {
                tab_component = tab_component.tooltip(Tooltip::text(tooltip_text));
            }
            tab_bar = tab_bar.child(tab_component);
        }

        tab_bar.into_any_element()
    }

    fn render_toolbar_back_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        IconButton::new("back", IconName::ArrowLeft)
            .on_click(cx.listener(|this, _, window, cx| {
                this.go_back(&workspace::GoBack, window, cx);
            }))
    }

    fn render_recent_entries_menu(&self, icon: IconName, corner: Corner, cx: &mut Context<Self>) -> impl IntoElement {
        // Dummy implementation for brevity
        IconButton::new("recent", icon)
    }

    fn render_panel_options_menu(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Dummy implementation for brevity
        IconButton::new("options", IconName::Settings)
    }

    fn render_onboarding(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        None
    }

    fn render_trial_end_upsell(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<impl IntoElement> {
        None
    }

    fn render_drag_target(&self, cx: &Context<Self>) -> impl IntoElement {
        div()
    }

    fn render_text_thread(&self, editor: &Entity<TextThreadEditor>, search: &Entity<BufferSearchBar>, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().child(editor.clone())
    }

    fn render_configuration_error(&self, _b: bool, _err: &ConfigurationError, _f: &FocusHandle, _cx: &mut App) -> impl IntoElement {
        div()
    }

    fn reset_agent_zoom(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn copy_thread_to_clipboard(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn load_thread_from_clipboard(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn toggle_navigation_menu(&mut self, _cx: &mut Context<Self>) {}
    fn toggle_new_thread_menu(&mut self, _cx: &mut Context<Self>) {}
    fn toggle_options_menu(&mut self, _cx: &mut Context<Self>) {}
    fn toggle_zoom(&mut self, _: &workspace::ToggleZoom, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn increase_font_size(&mut self, _: &IncreaseBufferFontSize, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn decrease_font_size(&mut self, _: &DecreaseBufferFontSize, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn reset_font_size(&mut self, _: &ResetBufferFontSize, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn new_external_agent_thread(&mut self, _action: &NewExternalAgentThread, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn new_native_agent_thread_from_summary(&mut self, _action: &NewNativeAgentThreadFromSummary, _window: &mut Window, _cx: &mut Context<Self>) {}
    fn new_native_agent_thread_from_session(&mut self, _action: &NewNativeAgentThreadFromSession, _window: &mut Window, _cx: &mut Context<Self>) {}
}

impl Focusable for AgentPanel {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.panel_focus_handle.clone()
    }
}

impl Panel for AgentPanel {
    fn position(&self, _cx: &WindowContext) -> DockPosition {
        DockPosition::Right
    }

    fn set_position(&mut self, _position: DockPosition, _cx: &mut ViewContext<Self>) {}

    fn size(&self, _cx: &WindowContext) -> Pixels {
        self.width.unwrap_or(Pixels::from(400.))
    }

    fn set_size(&mut self, size: Option<Pixels>, cx: &mut ViewContext<Self>) {
        self.width = size;
        self.serialize(cx);
    }

    fn is_zoomable(&self, _cx: &WindowContext) -> bool {
        true
    }

    fn is_zoomed(&self, _cx: &WindowContext) -> bool {
        false
    }

    fn set_zoomed(&mut self, _zoomed: bool, _cx: &mut ViewContext<Self>) {}
}

impl EventEmitter<PanelEvent> for AgentPanel {}

impl Render for AgentPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let content = v_flex()
            .size_full()
            .key_context(self.key_context())
            .track_focus(&self.panel_focus_handle)
            .on_action(cx.listener(Self::go_back))
            .on_action(cx.listener(|this, _: &CloseActiveThreadTab, window, cx| {
                this.remove_tab_by_id(this.active_tab_id, window, cx);
            }))
            .child(self.render_tab_bar(window, cx))
            .map(|parent| match self.active_view() {
                ActiveView::ExternalAgentThread { thread_view, .. } => parent
                    .child(thread_view.clone())
                    .child(self.render_drag_target(cx)),
                ActiveView::TextThread { text_thread_editor, buffer_search_bar, .. } => parent
                    .child(self.render_text_thread(text_thread_editor, buffer_search_bar, window, cx)),
                ActiveView::History => parent.child(self.acp_history.clone()),
                ActiveView::Configuration => parent.children(self.configuration.clone()),
                ActiveView::Uninitialized => parent,
            });

        WithRemSize::new(ThemeSettings::get_global(cx).agent_ui_font_size(cx))
            .size_full()
            .child(content)
    }
}

pub struct ConcreteAssistantPanelDelegate;
impl AgentPanelDelegate for ConcreteAssistantPanelDelegate {
    fn active_text_thread_editor(&self, workspace: &mut Workspace, _window: &mut Window, cx: &mut Context<Workspace>) -> Option<Entity<TextThreadEditor>> {
        workspace.panel::<AgentPanel>(cx).and_then(|p| p.read(cx).active_text_thread_editor())
    }
    fn open_local_text_thread(&self, workspace: &mut Workspace, path: Arc<Path>, window: &mut Window, cx: &mut Context<Workspace>) -> Task<Result<()>> {
        if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
            panel.update(cx, |panel, cx| panel.open_saved_text_thread(path, window, cx))
        } else {
            Task::ready(Err(anyhow!("Agent panel not found")))
        }
    }
    fn open_remote_text_thread(&self, _w: &mut Workspace, _id: assistant_text_thread::TextThreadId, _window: &mut Window, _cx: &mut Context<Workspace>) -> Task<Result<Entity<TextThreadEditor>>> {
        Task::ready(Err(anyhow!("Not implemented")))
    }
    fn quote_selection(&self, workspace: &mut Workspace, _ranges: Vec<Range<Anchor>>, _buffer: Entity<MultiBuffer>, _window: &mut Window, _cx: &mut Context<Workspace>) {}
    fn quote_terminal_text(&self, workspace: &mut Workspace, _text: String, _window: &mut Window, _cx: &mut Context<Workspace>) {}
}
