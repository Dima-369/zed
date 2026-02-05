mod agent_configuration;
mod agent_diff;
mod agent_model_selector;
mod agent_panel;
mod agent_panel_tab;
mod buffer_codegen;
mod context;
mod context_picker;
mod slash_command;
mod text_thread_editor;
mod text_thread_history;
mod ui;

pub use agent_panel::*;
pub use context::*;
pub use context_picker::*;
pub use text_thread_editor::*;

use gpui::{actions, App, AppContext, Window};
use ui::prelude::*;

actions!(
    agent,
    [
        /// Toggles the agent panel visibility.
        ToggleFocus,
        /// Create a new agent thread.
        NewThread,
        /// Create a new native agent thread from a summary.
        NewNativeAgentThreadFromSummary,
        /// Create a new native agent thread from an existing session.
        NewNativeAgentThreadFromSession,
        /// Create a new text thread.
        NewTextThread,
        /// Create a new external agent thread.
        NewExternalAgentThread,
        /// Opens the history view in the agent panel.
        OpenHistory,
        /// Toggles the navigation menu.
        ToggleNavigationMenu,
        /// Toggles the new thread menu.
        ToggleNewThreadMenu,
        /// Toggles the options menu.
        ToggleOptionsMenu,
        /// Resets the agent panel zoom level.
        ResetAgentZoom,
        /// Resets the trial upsell.
        ResetTrialUpsell,
        /// Resets the trial end upsell.
        ResetTrialEndUpsell,
        /// Resets the onboarding flow.
        ResetOnboarding,
        /// Opens the agent panel settings.
        OpenSettings,
        /// Adds a context server.
        AddContextServer,
        /// Removes the currently selected thread.
        RemoveSelectedThread,
        /// Closes the currently active thread tab.
        CloseActiveThreadTab,
        /// Starts a chat conversation with follow-up enabled.
        ChatWithFollow,
        /// Cycles to the next inline assist suggestion.
        CycleInlineAssistNext,
        /// Cycles to the previous inline assist suggestion.
        CycleInlineAssistPrevious,
        /// Cycles to the next inline assist model.
        CycleInlineAssistModelNext,
        /// Cycles to the previous inline assist model.
        CycleInlineAssistModelPrevious,
        /// Opens the inline assistant.
        InlineAssistant,
        /// Continues a thread.
        ContinueThread,
        /// Continues a thread with burn mode enabled.
        ContinueWithBurnMode,
        /// Toggles burn mode for faster responses.
        ToggleBurnMode,
        /// Toggles the plan view visibility.
        TogglePlan,
        /// Activates the next tab in the agent panel.
        ActivateNextTab,
        /// Activates the previous tab in the agent panel.
        ActivatePreviousTab,
        /// Closes the active thread tab or the dock.
        CloseActiveThreadTabOrDock,
        /// Dismisses all OS-level agent notifications.
        DismissOsNotifications,
    ]
);

pub fn init(
    client: std::sync::Arc<client::Client>,
    language_registry: std::sync::Arc<language::LanguageRegistry>,
    project: gpui::Entity<project::Project>,
    workspace: gpui::WeakEntity<workspace::Workspace>,
    is_eval: bool,
    cx: &mut App,
) {
    // Register global action to dismiss all agent notifications
    cx.on_action(|_: &DismissOsNotifications, cx| {
        dismiss_all_agent_notifications(cx);
    });

    assistant_text_thread::init(client, cx);
    rules_library::init(cx);
    if !is_eval {
        agent_panel::init(
            language_registry,
            project,
            workspace,
            cx,
        );
    }
}

fn dismiss_all_agent_notifications(cx: &mut App) {
    // Find all windows that contain AgentNotification and dismiss them
    let agent_notification_windows: Vec<_> = cx
        .windows()
        .iter()
        .filter_map(|window| window.downcast::<crate::ui::AgentNotification>())
        .collect();

    for window in agent_notification_windows {
        window
            .update(cx, |_, window, _| {
                window.remove_window();
            })
            .ok();
    }
}
