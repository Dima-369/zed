From commit 760dc7698f7b:

Your task is to bring this large diff into the current codebase.
They add multiple tabs to the AI thread dock and add those actions:

- agent::ActivatePreviousTab
- agent::ActivateNextTab
- agent::CloseActiveThread TabOrDock
- agent::TogglePlan
- agent::DismissOsNotifications

Verify compilation via `cargo check`.

```diff
diff --git a/crates/agent_ui/src/acp/thread_view/active_thread.rs b/crates/agent_ui/src/acp/thread_view/active_thread.rs
index 6e30c6d276..0b289c2dc6 100644
--- a/crates/agent_ui/src/acp/thread_view/active_thread.rs
+++ b/crates/agent_ui/src/acp/thread_view/active_thread.rs
@@ -3003,6 +3003,11 @@ impl AcpThreadView {
                 this.toggle_following(window, cx);
             }))
     }
+
+    fn toggle_plan(&mut self, _: &crate::TogglePlan, _window: &mut Window, cx: &mut Context<Self>) {
+        self.plan_expanded = !self.plan_expanded;
+        cx.notify();
+    }
 }

 impl AcpThreadView {
@@ -6898,6 +6903,7 @@ impl Render for AcpThreadView {
                         .update(cx, |model_selector, cx| model_selector.toggle(window, cx));
                 }
             }))
+            .on_action(cx.listener(Self::toggle_plan))
             .on_action(cx.listener(|this, _: &CycleFavoriteModels, window, cx| {
                 if let Some(config_options_view) = this.config_options_view.clone() {
                     let handled = config_options_view.update(cx, |view, cx| {
diff --git a/crates/agent_ui/src/agent_panel.rs b/crates/agent_ui/src/agent_panel.rs
index 8f540cd1de..078b628757 100644
--- a/crates/agent_ui/src/agent_panel.rs
+++ b/crates/agent_ui/src/agent_panel.rs
@@ -1,3 +1,4 @@
+use std::collections::HashMap;
 use std::{ops::Range, path::Path, rc::Rc, sync::Arc, time::Duration};

 use acp_thread::{AcpThread, AgentSessionInfo};
@@ -36,6 +37,7 @@ use crate::{
     ExternalAgent, ExternalAgentInitialContent, NewExternalAgentThread,
     NewNativeAgentThreadFromSummary,
 };
+use crate::agent_panel_tab::{AgentPanelTab, TabId};
 use agent_settings::AgentSettings;
 use ai_onboarding::AgentPanelOnboarding;
 use anyhow::{Result, anyhow};
@@ -49,8 +51,8 @@ use extension_host::ExtensionStore;
 use fs::Fs;
 use gpui::{
     Action, Animation, AnimationExt, AnyElement, App, AsyncWindowContext, ClipboardItem, Corner,
-    DismissEvent, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, KeyContext, Pixels,
-    Subscription, Task, UpdateGlobal, WeakEntity, prelude::*, pulsating_between,
+    DismissEvent, Empty, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, KeyContext, Pixels,
+    ScrollHandle, Subscription, Task, UpdateGlobal, WeakEntity, prelude::*, pulsating_between,
 };
 use language::LanguageRegistry;
 use language_model::{ConfigurationError, LanguageModelRegistry};
@@ -61,8 +63,8 @@ use search::{BufferSearchBar, buffer_search};
 use settings::{Settings, update_settings_file};
 use theme::ThemeSettings;
 use ui::{
-    Callout, ContextMenu, ContextMenuEntry, KeyBinding, PopoverMenu, PopoverMenuHandle, Tab,
-    Tooltip, prelude::*, utils::WithRemSize,
+    Callout, ContextMenu, ContextMenuEntry, IconButtonShape, KeyBinding, PopoverMenu, PopoverMenuHandle, Tab,
+    TabBar, TabCloseSide, TabPosition, Tooltip, prelude::*, utils::WithRemSize,
 };
 use util::ResultExt as _;
 use workspace::{
@@ -76,10 +78,19 @@ use zed_actions::{
     },
     assistant::{OpenRulesLibrary, ToggleFocus},
 };
+use std::cmp::Ordering;

 const AGENT_PANEL_KEY: &str = "agent_panel";
 const RECENTLY_UPDATED_MENU_LIMIT: usize = 6;
 const DEFAULT_THREAD_TITLE: &str = "New Thread";
+const LOADING_SUMMARY_PLACEHOLDER: &str = "Loading Summary…";
+
+#[derive(Debug)]
+struct DetachedThread {
+    session_id: acp::SessionId,
+    title: SharedString,
+    updated_at: chrono::DateTime<chrono::Utc>,
+}

 #[derive(Serialize, Deserialize, Debug)]
 struct SerializedAgentPanel {
@@ -113,6 +124,21 @@ pub fn init(cx: &mut App) {
                         panel.update(cx, |panel, cx| panel.expand_message_editor(window, cx));
                     }
                 })
+                .register_action(|workspace, _: &crate::ActivateNextTab, window, cx| {
+                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
+                        panel.update(cx, |panel, cx| panel.activate_next_tab(window, cx));
+                    }
+                })
+                .register_action(|workspace, _: &crate::ActivatePreviousTab, window, cx| {
+                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
+                        panel.update(cx, |panel, cx| panel.activate_previous_tab(window, cx));
+                    }
+                })
+                .register_action(|workspace, action: &crate::CloseActiveThreadTabOrDock, window, cx| {
+                    if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
+                        panel.update(cx, |panel, cx| panel.close_active_thread_tab_or_dock(action, window, cx));
+                    }
+                })
                 .register_action(|workspace, _: &OpenHistory, window, cx| {
                     if let Some(panel) = workspace.panel::<AgentPanel>(cx) {
                         workspace.focus_panel::<AgentPanel>(window, cx);
@@ -241,9 +267,9 @@ enum HistoryKind {
     TextThreads,
 }

-enum ActiveView {
+pub enum ActiveView {
     Uninitialized,
-    AgentThread {
+    ExternalAgentThread {
         thread_view: Entity<AcpServerView>,
     },
     TextThread {
@@ -258,6 +284,7 @@ enum ActiveView {
     Configuration,
 }

+
 enum WhichFontSize {
     AgentFont,
     BufferFont,
@@ -316,7 +343,7 @@ impl ActiveView {
     pub fn which_font_size_used(&self) -> WhichFontSize {
         match self {
             ActiveView::Uninitialized
-            | ActiveView::AgentThread { .. }
+            | ActiveView::ExternalAgentThread { .. }
             | ActiveView::History { .. } => WhichFontSize::AgentFont,
             ActiveView::TextThread { .. } => WhichFontSize::BufferFont,
             ActiveView::Configuration => WhichFontSize::None,
@@ -426,12 +453,13 @@ pub struct AgentPanel {
     configuration: Option<Entity<AgentConfiguration>>,
     configuration_subscription: Option<Subscription>,
     focus_handle: FocusHandle,
-    active_view: ActiveView,
-    previous_view: Option<ActiveView>,
+    overlay_view: Option<ActiveView>,
+    overlay_previous_tab_id: Option<TabId>,
     new_thread_menu_handle: PopoverMenuHandle<ContextMenu>,
     agent_panel_menu_handle: PopoverMenuHandle<ContextMenu>,
     agent_navigation_menu_handle: PopoverMenuHandle<ContextMenu>,
     agent_navigation_menu: Option<Entity<ContextMenu>>,
+    panel_focus_handle: FocusHandle,
     _extension_subscription: Option<Subscription>,
     width: Option<Pixels>,
     height: Option<Pixels>,
@@ -439,7 +467,14 @@ pub struct AgentPanel {
     pending_serialization: Option<Task<Result<()>>>,
     onboarding: Entity<AgentPanelOnboarding>,
     selected_agent: AgentType,
+    detached_threads: HashMap<acp::SessionId, DetachedThread>,
+    pending_tab_removal: Option<TabId>,
+    tabs: Vec<AgentPanelTab>,
+    active_tab_id: TabId,
+    tab_bar_scroll_handle: ScrollHandle,
+    title_edit_overlay_tab_id: Option<TabId>,
     show_trust_workspace_message: bool,
+    overlay_title_editor: Option<Entity<Editor>>,
 }

 impl AgentPanel {
@@ -632,7 +667,6 @@ impl AgentPanel {
         };

         let mut panel = Self {
-            active_view,
             workspace,
             user_store,
             project: project.clone(),
@@ -644,11 +678,13 @@ impl AgentPanel {
             configuration_subscription: None,
             focus_handle: cx.focus_handle(),
             context_server_registry,
-            previous_view: None,
+            overlay_view: None,
+            overlay_previous_tab_id: None,
             new_thread_menu_handle: PopoverMenuHandle::default(),
             agent_panel_menu_handle: PopoverMenuHandle::default(),
             agent_navigation_menu_handle: PopoverMenuHandle::default(),
             agent_navigation_menu: None,
+            panel_focus_handle: cx.focus_handle(),
             _extension_subscription: extension_subscription,
             width: None,
             height: None,
@@ -659,9 +695,21 @@ impl AgentPanel {
             text_thread_history,
             thread_store,
             selected_agent: AgentType::default(),
+            detached_threads: HashMap::default(),
+            pending_tab_removal: None,
+            tabs: vec![],
+            active_tab_id: 0,
+            tab_bar_scroll_handle: ScrollHandle::new(),
+            title_edit_overlay_tab_id: None,
             show_trust_workspace_message: false,
+            overlay_title_editor: None,
         };

+        // Initialize with an empty tab to ensure there's always at least one tab
+        let initial_view = ActiveView::Uninitialized;
+        let initial_tab = AgentPanelTab::new(initial_view, AgentType::default());
+        panel.tabs.push(initial_tab);
+
         // Initial sync of agent servers from extensions
         panel.sync_agent_servers_from_extensions(cx);
         panel
@@ -732,8 +780,8 @@ impl AgentPanel {
     }

     pub(crate) fn active_thread_view(&self) -> Option<&Entity<AcpServerView>> {
-        match &self.active_view {
-            ActiveView::AgentThread { thread_view, .. } => Some(thread_view),
+        match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view, .. } => Some(thread_view),
             ActiveView::Uninitialized
             | ActiveView::TextThread { .. }
             | ActiveView::History { .. }
@@ -952,11 +1000,10 @@ impl AgentPanel {
             return;
         };

-        if let ActiveView::History { kind: active_kind } = self.active_view {
-            if active_kind == kind {
-                if let Some(previous_view) = self.previous_view.take() {
-                    self.set_active_view(previous_view, true, window, cx);
-                }
+        if let ActiveView::History { kind: active_kind } = self.active_view() {
+            if *active_kind == kind {
+                // In the new tab system, we don't have a previous view concept
+                // Just return if we're already on the same history kind
                 return;
             }
         }
@@ -1017,24 +1064,28 @@ impl AgentPanel {
     }

     pub fn go_back(&mut self, _: &workspace::GoBack, window: &mut Window, cx: &mut Context<Self>) {
-        match self.active_view {
+        match self.active_view() {
             ActiveView::Configuration | ActiveView::History { .. } => {
-                if let Some(previous_view) = self.previous_view.take() {
-                    self.active_view = previous_view;
+                // In the new tab system, we don't have a previous view concept
+                // We need to handle this differently
+                if let Some(overlay_view) = self.overlay_view.take() {
+                    if let Some(overlay_tab_id) = self.overlay_previous_tab_id.take() {
+                        self.set_active_tab_by_id(overlay_tab_id, window, cx);
+                    }
+                }

-                    match &self.active_view {
-                        ActiveView::AgentThread { thread_view } => {
-                            thread_view.focus_handle(cx).focus(window, cx);
-                        }
-                        ActiveView::TextThread {
-                            text_thread_editor, ..
-                        } => {
-                            text_thread_editor.focus_handle(cx).focus(window, cx);
-                        }
-                        ActiveView::Uninitialized
-                        | ActiveView::History { .. }
-                        | ActiveView::Configuration => {}
+                match &self.active_view() {
+                    ActiveView::ExternalAgentThread { thread_view } => {
+                        thread_view.focus_handle(cx).focus(window, cx);
+                    }
+                    ActiveView::TextThread {
+                        text_thread_editor, ..
+                    } => {
+                        text_thread_editor.focus_handle(cx).focus(window, cx);
                     }
+                    ActiveView::Uninitialized
+                    | ActiveView::History { .. }
+                    | ActiveView::Configuration => {}
                 }
                 cx.notify();
             }
@@ -1091,7 +1142,7 @@ impl AgentPanel {
     }

     fn handle_font_size_action(&mut self, persist: bool, delta: Pixels, cx: &mut Context<Self>) {
-        match self.active_view.which_font_size_used() {
+        match self.active_view().which_font_size_used() {
             WhichFontSize::AgentFont => {
                 if persist {
                     update_settings_file(self.fs.clone(), cx, move |settings, cx| {
@@ -1416,8 +1467,8 @@ impl AgentPanel {
     }

     pub(crate) fn active_agent_thread(&self, cx: &App) -> Option<Entity<AcpThread>> {
-        match &self.active_view {
-            ActiveView::AgentThread { thread_view, .. } => thread_view
+        match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view, .. } => thread_view
                 .read(cx)
                 .as_active_thread()
                 .map(|r| r.read(cx).thread.clone()),
@@ -1426,8 +1477,8 @@ impl AgentPanel {
     }

     pub(crate) fn active_native_agent_thread(&self, cx: &App) -> Option<Entity<agent::Thread>> {
-        match &self.active_view {
-            ActiveView::AgentThread { thread_view, .. } => {
+        match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view, .. } => {
                 thread_view.read(cx).as_native_thread(cx)
             }
             _ => None,
@@ -1435,7 +1486,7 @@ impl AgentPanel {
     }

     pub(crate) fn active_text_thread_editor(&self) -> Option<Entity<TextThreadEditor>> {
-        match &self.active_view {
+        match &self.active_view() {
             ActiveView::TextThread {
                 text_thread_editor, ..
             } => Some(text_thread_editor.clone()),
@@ -1450,30 +1501,136 @@ impl AgentPanel {
         window: &mut Window,
         cx: &mut Context<Self>,
     ) {
-        let current_is_uninitialized = matches!(self.active_view, ActiveView::Uninitialized);
-        let current_is_history = matches!(self.active_view, ActiveView::History { .. });
+        let current_is_uninitialized = matches!(self.active_view(), ActiveView::Uninitialized);
+        let current_is_history = matches!(self.active_view(), ActiveView::History { .. });
         let new_is_history = matches!(new_view, ActiveView::History { .. });

-        let current_is_config = matches!(self.active_view, ActiveView::Configuration);
+        let current_is_config = matches!(self.active_view(), ActiveView::Configuration);
         let new_is_config = matches!(new_view, ActiveView::Configuration);

         let current_is_special = current_is_history || current_is_config;
         let new_is_special = new_is_history || new_is_config;

-        if current_is_uninitialized || (current_is_special && !new_is_special) {
-            self.active_view = new_view;
-        } else if !current_is_special && new_is_special {
-            self.previous_view = Some(std::mem::replace(&mut self.active_view, new_view));
-        } else {
-            if !new_is_special {
-                self.previous_view = None;
+        if current_is_uninitialized {
+            // Replace the uninitialized tab with the new view
+            if let Some(tab) = self.tabs.get_mut(self.active_tab_id) {
+                tab.view = new_view;
+                tab.agent = self.selected_agent.clone();
             }
-            self.active_view = new_view;
+        } else if new_is_special {
+            // History/Configuration are shown as overlays
+            self.overlay_view = Some(new_view);
+            self.overlay_previous_tab_id = Some(self.active_tab_id);
+        } else {
+            // New thread view - push as a new tab
+            self.overlay_view.take();
+            self.overlay_previous_tab_id.take();
+
+            let agent = self.selected_agent.clone();
+            let tab = AgentPanelTab::new(new_view, agent);
+            self.tabs.push(tab);
+            self.active_tab_id = self.tabs.len() - 1;
+            self.tab_bar_scroll_handle.scroll_to_item(self.active_tab_id);
         }

         if focus {
             self.focus_handle(cx).focus(window, cx);
         }
+        cx.notify();
+    }
+
+    fn set_active_tab_by_id(
+        &mut self,
+        tab_id: TabId,
+        window: &mut Window,
+        cx: &mut Context<Self>,
+    ) {
+        if tab_id < self.tabs.len() {
+            self.overlay_view = None;
+            self.overlay_previous_tab_id = None;
+            self.active_tab_id = tab_id;
+            self.tab_bar_scroll_handle.scroll_to_item(tab_id);
+            self.focus_handle(cx).focus(window, cx);
+            cx.notify();
+        }
+    }
+
+    fn remove_tab_by_id(
+        &mut self,
+        tab_id: TabId,
+        window: &mut Window,
+        cx: &mut Context<Self>,
+    ) {
+        if self.tabs.len() <= 1 {
+            // If there's only one tab, close the panel instead
+            if let Some(workspace) = self.workspace.upgrade() {
+                window.defer(cx, move |window, cx| {
+                    workspace.update(cx, |workspace, cx| {
+                        workspace.close_panel::<Self>(window, cx);
+                    });
+                });
+            }
+            return;
+        }
+
+        if tab_id < self.tabs.len() {
+            self.tabs.remove(tab_id);
+
+            // Adjust active tab ID if needed
+            if self.active_tab_id >= tab_id && self.active_tab_id > 0 {
+                self.active_tab_id = self.active_tab_id.saturating_sub(1);
+            }
+
+            self.tab_bar_scroll_handle.scroll_to_item(self.active_tab_id);
+            self.focus_handle(cx).focus(window, cx);
+            cx.notify();
+        }
+    }
+
+    fn activate_next_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
+        if self.tabs.len() <= 1 {
+            return;
+        }
+
+        let next_id = if self.active_tab_id + 1 >= self.tabs.len() {
+            0 // Wrap around to the first tab
+        } else {
+            self.active_tab_id + 1
+        };
+
+        self.set_active_tab_by_id(next_id, window, cx);
+    }
+
+    fn activate_previous_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
+        if self.tabs.len() <= 1 {
+            return;
+        }
+
+        let prev_id = if self.active_tab_id == 0 {
+            self.tabs.len() - 1 // Wrap around to the last tab
+        } else {
+            self.active_tab_id - 1
+        };
+
+        self.set_active_tab_by_id(prev_id, window, cx);
+    }
+
+    fn add_tab(
+        &mut self,
+        view: ActiveView,
+        agent: AgentType,
+        window: &mut Window,
+        cx: &mut Context<Self>,
+    ) {
+        let tab = AgentPanelTab::new(view, agent);
+        self.tabs.push(tab);
+
+        // Set the newly added tab as active
+        self.active_tab_id = self.tabs.len() - 1;
+
+        // Update the active view to match the new tab
+        // We'll just notify that the UI needs to update
+        cx.notify();
     }

     fn populate_recently_updated_menu_section(
@@ -1666,6 +1823,23 @@ impl AgentPanel {
         }
     }

+    pub fn close_active_thread_tab_or_dock(
+        &mut self,
+        _: &crate::CloseActiveThreadTabOrDock,
+        window: &mut Window,
+        cx: &mut Context<Self>,
+    ) {
+        if self.tabs.len() > 1 {
+            self.remove_tab_by_id(self.active_tab_id, window, cx);
+        } else if let Some(workspace) = self.workspace.upgrade() {
+            window.defer(cx, move |window, cx| {
+                workspace.update(cx, |workspace, cx| {
+                    workspace.close_panel::<Self>(window, cx);
+                });
+            });
+        }
+    }
+
     pub fn load_agent_thread(
         &mut self,
         thread: AgentSessionInfo,
@@ -1715,15 +1889,15 @@ impl AgentPanel {
             )
         });

-        self.set_active_view(ActiveView::AgentThread { thread_view }, true, window, cx);
+        self.set_active_view(ActiveView::ExternalAgentThread { thread_view }, true, window, cx);
     }
 }

 impl Focusable for AgentPanel {
     fn focus_handle(&self, cx: &App) -> FocusHandle {
-        match &self.active_view {
+        match &self.active_view() {
             ActiveView::Uninitialized => self.focus_handle.clone(),
-            ActiveView::AgentThread { thread_view, .. } => thread_view.focus_handle(cx),
+            ActiveView::ExternalAgentThread { thread_view, .. } => thread_view.focus_handle(cx),
             ActiveView::History { kind } => match kind {
                 HistoryKind::AgentThreads => self.acp_history.focus_handle(cx),
                 HistoryKind::TextThreads => self.text_thread_history.focus_handle(cx),
@@ -1794,7 +1968,7 @@ impl Panel for AgentPanel {
     }

     fn set_active(&mut self, active: bool, window: &mut Window, cx: &mut Context<Self>) {
-        if active && matches!(self.active_view, ActiveView::Uninitialized) {
+        if active && matches!(self.active_view(), ActiveView::Uninitialized) {
             let selected_agent = self.selected_agent.clone();
             self.new_agent_thread(selected_agent, window, cx);
         }
@@ -1838,8 +2012,8 @@ impl AgentPanel {
     fn render_title_view(&self, _window: &mut Window, cx: &Context<Self>) -> AnyElement {
         const LOADING_SUMMARY_PLACEHOLDER: &str = "Loading Summary…";

-        let content = match &self.active_view {
-            ActiveView::AgentThread { thread_view } => {
+        let content = match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view } => {
                 let is_generating_title = thread_view
                     .read(cx)
                     .as_native_thread(cx)
@@ -2003,13 +2177,13 @@ impl AgentPanel {

         let selected_agent = self.selected_agent.clone();

-        let text_thread_view = match &self.active_view {
+        let text_thread_view = match &self.active_view() {
             ActiveView::TextThread {
                 text_thread_editor, ..
             } => Some(text_thread_editor.clone()),
             _ => None,
         };
-        let text_thread_with_messages = match &self.active_view {
+        let text_thread_with_messages = match &self.active_view() {
             ActiveView::TextThread {
                 text_thread_editor, ..
             } => text_thread_editor
@@ -2021,12 +2195,12 @@ impl AgentPanel {
             _ => false,
         };

-        let thread_view = match &self.active_view {
-            ActiveView::AgentThread { thread_view } => Some(thread_view.clone()),
+        let thread_view = match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view } => Some(thread_view.clone()),
             _ => None,
         };
-        let thread_with_messages = match &self.active_view {
-            ActiveView::AgentThread { thread_view } => {
+        let thread_with_messages = match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view } => {
                 thread_view.read(cx).has_user_submitted_prompt(cx)
             }
             _ => false,
@@ -2189,8 +2363,8 @@ impl AgentPanel {
                 (None, self.selected_agent.label())
             };

-        let active_thread = match &self.active_view {
-            ActiveView::AgentThread { thread_view } => thread_view.read(cx).as_native_thread(cx),
+        let active_thread = match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view } => thread_view.read(cx).as_native_thread(cx),
             ActiveView::Uninitialized
             | ActiveView::TextThread { .. }
             | ActiveView::History { .. }
@@ -2548,7 +2722,7 @@ impl AgentPanel {
                     .size_full()
                     .gap(DynamicSpacing::Base04.rems(cx))
                     .pl(DynamicSpacing::Base04.rems(cx))
-                    .child(match &self.active_view {
+                    .child(match &self.active_view() {
                         ActiveView::History { .. } | ActiveView::Configuration => {
                             self.render_toolbar_back_button(cx).into_any_element()
                         }
@@ -2579,7 +2753,7 @@ impl AgentPanel {
             return false;
         }

-        match &self.active_view {
+        match &self.active_view() {
             ActiveView::TextThread { .. } => {
                 if LanguageModelRegistry::global(cx)
                     .read(cx)
@@ -2592,7 +2766,7 @@ impl AgentPanel {
                 }
             }
             ActiveView::Uninitialized
-            | ActiveView::AgentThread { .. }
+            | ActiveView::ExternalAgentThread { .. }
             | ActiveView::History { .. }
             | ActiveView::Configuration => return false,
         }
@@ -2620,11 +2794,11 @@ impl AgentPanel {
             return false;
         }

-        match &self.active_view {
+        match &self.active_view() {
             ActiveView::Uninitialized | ActiveView::History { .. } | ActiveView::Configuration => {
                 false
             }
-            ActiveView::AgentThread { thread_view, .. }
+            ActiveView::ExternalAgentThread { thread_view, .. }
                 if thread_view.read(cx).as_native_thread(cx).is_none() =>
             {
                 false
@@ -2654,7 +2828,7 @@ impl AgentPanel {
             return None;
         }

-        let text_thread_view = matches!(&self.active_view, ActiveView::TextThread { .. });
+        let text_thread_view = matches!(&self.active_view(), ActiveView::TextThread { .. });

         Some(
             div()
@@ -2771,7 +2945,7 @@ impl AgentPanel {
         cx: &mut Context<Self>,
     ) -> Div {
         let mut registrar = buffer_search::DivRegistrar::new(
-            |this, _, _cx| match &this.active_view {
+            |this, _, _cx| match &this.active_view() {
                 ActiveView::TextThread {
                     buffer_search_bar, ..
                 } => Some(buffer_search_bar.clone()),
@@ -2869,8 +3043,8 @@ impl AgentPanel {
         window: &mut Window,
         cx: &mut Context<Self>,
     ) {
-        match &self.active_view {
-            ActiveView::AgentThread { thread_view } => {
+        match &self.active_view() {
+            ActiveView::ExternalAgentThread { thread_view } => {
                 thread_view.update(cx, |thread_view, cx| {
                     thread_view.insert_dragged_files(paths, added_worktrees, window, cx);
                 });
@@ -2927,13 +3101,242 @@ impl AgentPanel {
     fn key_context(&self) -> KeyContext {
         let mut key_context = KeyContext::new_with_defaults();
         key_context.add("AgentPanel");
-        match &self.active_view {
-            ActiveView::AgentThread { .. } => key_context.add("acp_thread"),
+        match &self.active_view() {
+            ActiveView::ExternalAgentThread { .. } => key_context.add("acp_thread"),
             ActiveView::TextThread { .. } => key_context.add("text_thread"),
             ActiveView::Uninitialized | ActiveView::History { .. } | ActiveView::Configuration => {}
         }
         key_context
     }
+
+    fn render_tab_label(
+        &self,
+        tab: &ActiveView,
+        is_active: bool,
+        cx: &Context<Self>,
+    ) -> crate::agent_panel_tab::TabLabelRender {
+        let is_generating = match tab {
+            ActiveView::ExternalAgentThread { thread_view } => thread_view
+                .read(cx)
+                .as_active_thread()
+                .map_or(false, |t| {
+                    t.read(cx).thread.read(cx).status() == acp_thread::ThreadStatus::Generating
+                }),
+            ActiveView::TextThread {
+                text_thread_editor, ..
+            } => text_thread_editor
+                .read(cx)
+                .text_thread()
+                .read(cx)
+                .messages(cx)
+                .any(|msg| matches!(msg.status, assistant_text_thread::MessageStatus::Pending)),
+            ActiveView::History { .. } => false,
+            ActiveView::Configuration => false,
+            ActiveView::Uninitialized => false,
+        };
+
+        let title = match tab {
+            ActiveView::ExternalAgentThread { thread_view } => {
+                thread_view.read(cx).title(cx)
+            }
+            ActiveView::TextThread {
+                text_thread_editor, ..
+            } => {
+                let text_thread = text_thread_editor.read(cx).text_thread().read(cx);
+                match text_thread.summary() {
+                    TextThreadSummary::Pending => LOADING_SUMMARY_PLACEHOLDER.into(),
+                    TextThreadSummary::Content(summary) => {
+                        if summary.done {
+                            text_thread_editor.read(cx).title(cx).to_string().into()
+                        } else {
+                            LOADING_SUMMARY_PLACEHOLDER.into()
+                        }
+                    }
+                    TextThreadSummary::Error => {
+                        format!("⚠️ {}", text_thread_editor.read(cx).title(cx)).into()
+                    }
+                }
+            }
+            ActiveView::History { kind } => match kind {
+                HistoryKind::AgentThreads => "History".into(),
+                HistoryKind::TextThreads => "Text Thread History".into(),
+            },
+            ActiveView::Configuration => "Settings".into(),
+            ActiveView::Uninitialized => "Agent".into(),
+        };
+
+        let label = Label::new(title)
+            .truncate()
+            .when(!is_active, |label| label.color(Color::Muted))
+            .into_any_element();
+
+        crate::agent_panel_tab::TabLabelRender {
+            element: label,
+            tooltip: None,
+            is_generating,
+        }
+    }
+
+    fn render_tab_agent_icon(
+        &self,
+        _index: usize,
+        agent: &AgentType,
+        _agent_server_store: &Entity<project::AgentServerStore>,
+        cx: &Context<Self>,
+    ) -> impl IntoElement {
+        let icon = match agent {
+            AgentType::NativeAgent | AgentType::TextThread => None,
+            AgentType::Gemini => Some(IconName::AiGemini),
+            AgentType::ClaudeCode => Some(IconName::AiClaude),
+            AgentType::Codex => Some(IconName::AiOpenAi),
+            AgentType::Custom { .. } => Some(IconName::Sparkle),
+        };
+
+        if let Some(icon) = icon {
+            h_flex()
+                .child(Icon::new(icon).color(Color::Muted).size(IconSize::Small))
+                .into_any_element()
+        } else {
+            Empty.into_any_element()
+        }
+    }
+
+    fn render_tab_bar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
+        let agent_server_store = self.project.read(cx).agent_server_store().clone();
+
+        let end_slot = h_flex()
+            .gap(DynamicSpacing::Base02.rems(cx))
+            .pl(DynamicSpacing::Base04.rems(cx))
+            .pr(DynamicSpacing::Base06.rems(cx))
+            .child(self.render_recent_entries_menu(IconName::MenuAltTemp, Corner::TopRight, cx))
+            .child(self.render_panel_options_menu(window, cx));
+
+        let mut tab_bar = TabBar::new("agent-tab-bar")
+            .track_scroll(&self.tab_bar_scroll_handle)
+            .end_child(end_slot);
+
+        if let Some(overlay_view) = &self.overlay_view {
+            let crate::agent_panel_tab::TabLabelRender {
+                element: overlay_label,
+                ..
+            } = self.render_tab_label(overlay_view, true, cx);
+
+            let overlay_title = h_flex()
+                .flex_grow()
+                .h(Tab::content_height(cx))
+                .px(DynamicSpacing::Base04.px(cx))
+                .gap(DynamicSpacing::Base04.rems(cx))
+                .bg(cx.theme().colors().tab_bar_background)
+                .child(self.render_toolbar_back_button(cx).into_any_element())
+                .child(overlay_label)
+                .into_any_element();
+
+            return tab_bar.child(overlay_title).into_any_element();
+        }
+
+        if let Some(overlay_editor) = self.render_overlay_title_editor(cx) {
+            return tab_bar.child(overlay_editor).into_any_element();
+        }
+
+        let active_index = self.active_tab_id;
+        for (index, tab) in self.tabs.iter().enumerate() {
+            let is_active = index == active_index;
+            let position = if index == 0 {
+                TabPosition::First
+            } else if index == self.tabs.len() - 1 {
+                TabPosition::Last
+            } else {
+                let ordering = if index < active_index {
+                    Ordering::Less
+                } else if index > active_index {
+                    Ordering::Greater
+                } else {
+                    Ordering::Equal
+                };
+                TabPosition::Middle(ordering)
+            };
+
+            let crate::agent_panel_tab::TabLabelRender {
+                element: tab_label,
+                tooltip,
+                is_generating,
+            } = self.render_tab_label(tab.view(), is_active, cx);
+
+            let indicator = is_generating.then(|| ui::Indicator::dot().color(Color::Accent));
+            let agent_icon =
+                self.render_tab_agent_icon(index, tab.agent(), &agent_server_store, cx);
+            let start_slot = h_flex()
+                .gap(DynamicSpacing::Base04.rems(cx))
+                .children(indicator)
+                .child(agent_icon);
+
+            let mut tab_component = Tab::new(("agent-tab", index))
+                .position(position)
+                .close_side(TabCloseSide::End)
+                .toggle_state(is_active)
+                .on_click(cx.listener(move |this: &mut Self, _, window, cx| {
+                    if is_active {
+                        this.focus_title_editor(window, cx);
+                    } else {
+                        this.set_active_tab_by_id(index, window, cx);
+                    }
+                }))
+                .child(tab_label)
+                .start_slot(start_slot)
+                .end_slot(
+                    IconButton::new(("close-agent-tab", index), IconName::Close)
+                        .shape(IconButtonShape::Square)
+                        .icon_size(IconSize::Small)
+                        .visible_on_hover("")
+                        .on_click(cx.listener(move |this: &mut Self, _, window, cx| {
+                            this.remove_tab_by_id(index, window, cx);
+                        }))
+                        .tooltip(|_window, cx| cx.new(|_| Tooltip::new("Close Thread")).into()),
+                );
+
+            if let Some(tooltip_text) = tooltip {
+                tab_component = tab_component.tooltip(Tooltip::text(tooltip_text));
+            }
+            tab_bar = tab_bar.child(tab_component);
+        }
+        tab_bar.into_any_element()
+    }
+
+    fn render_overlay_title_editor(&self, cx: &Context<Self>) -> Option<AnyElement> {
+        if let Some(editor) = &self.overlay_title_editor {
+            Some(
+                h_flex()
+                    .id("overlay-title-editor")
+                    .flex_grow()
+                    .h(Tab::content_height(cx))
+                    .px(DynamicSpacing::Base04.px(cx))
+                    .child(editor.clone())
+                    .into_any_element(),
+            )
+        } else {
+            None
+        }
+    }
+
+    fn focus_title_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
+        if let Some(editor) = &self.overlay_title_editor {
+            editor.focus_handle(cx).focus(window, cx);
+        }
+    }
+
+    fn active_view(&self) -> &ActiveView {
+        if let Some(overlay_view) = &self.overlay_view {
+            overlay_view
+        } else if let Some(tab) = self.tabs.get(self.active_tab_id) {
+            &tab.view
+        } else {
+            // This should not happen in practice since we ensure there's always at least one tab
+            // But if it does, we need to return a reference somehow.
+            // Since ActiveView contains non-Sync fields, we can't make a static instance.
+            // Instead, we'll panic with a clearer message.
+            panic!("No active view available - AgentPanel should always have at least one tab");
+        }
+    }
 }

 impl Render for AgentPanel {
@@ -2952,6 +3355,7 @@ impl Render for AgentPanel {
             .size_full()
             .justify_between()
             .key_context(self.key_context())
+            .track_focus(&self.panel_focus_handle)
             .on_action(cx.listener(|this, action: &NewThread, window, cx| {
                 this.new_thread(action, window, cx);
             }))
@@ -2966,6 +3370,9 @@ impl Render for AgentPanel {
             .on_action(cx.listener(Self::go_back))
             .on_action(cx.listener(Self::toggle_navigation_menu))
             .on_action(cx.listener(Self::toggle_options_menu))
+            .on_action(cx.listener(|this, _: &crate::CloseActiveThreadTab, window, cx| {
+                this.remove_tab_by_id(this.active_tab_id, window, cx);
+            }))
             .on_action(cx.listener(Self::increase_font_size))
             .on_action(cx.listener(Self::decrease_font_size))
             .on_action(cx.listener(Self::reset_font_size))
@@ -2975,12 +3382,12 @@ impl Render for AgentPanel {
                     thread_view.update(cx, |thread_view, cx| thread_view.reauthenticate(window, cx))
                 }
             }))
-            .child(self.render_toolbar(window, cx))
+            .child(self.render_tab_bar(window, cx))
             .children(self.render_workspace_trust_message(cx))
             .children(self.render_onboarding(window, cx))
-            .map(|parent| match &self.active_view {
+            .map(|parent| match self.active_view() {
                 ActiveView::Uninitialized => parent,
-                ActiveView::AgentThread { thread_view, .. } => parent
+                ActiveView::ExternalAgentThread { thread_view, .. } => parent
                     .child(thread_view.clone())
                     .child(self.render_drag_target(cx)),
                 ActiveView::History { kind } => match kind {
@@ -3021,7 +3428,7 @@ impl Render for AgentPanel {
             })
             .children(self.render_trial_end_upsell(window, cx));

-        match self.active_view.which_font_size_used() {
+        match self.active_view().which_font_size_used() {
             WhichFontSize::AgentFont => {
                 WithRemSize::new(ThemeSettings::get_global(cx).agent_ui_font_size(cx))
                     .size_full()
diff --git a/crates/agent_ui/src/agent_panel_tab.rs b/crates/agent_ui/src/agent_panel_tab.rs
index d01b9734e8..281954fe7b 100644
--- a/crates/agent_ui/src/agent_panel_tab.rs
+++ b/crates/agent_ui/src/agent_panel_tab.rs
@@ -36,4 +36,4 @@ pub struct TabLabelRender {
 pub enum AgentPanelTabIdentity {
     AcpThread(acp::SessionId),
     TextThread(Arc<Path>),
-}
+}
\ No newline at end of file
diff --git a/crates/agent_ui/src/agent_ui.rs b/crates/agent_ui/src/agent_ui.rs
index e521cb33b1..878a25964b 100644
--- a/crates/agent_ui/src/agent_ui.rs
+++ b/crates/agent_ui/src/agent_ui.rs
@@ -3,6 +3,7 @@ mod agent_configuration;
 mod agent_diff;
 mod agent_model_selector;
 mod agent_panel;
+mod agent_panel_tab;
 mod agent_registry_ui;
 mod buffer_codegen;
 mod completion_provider;
@@ -143,6 +144,18 @@ actions!(
         OpenPermissionDropdown,
         /// Toggles thinking mode for models that support extended thinking.
         ToggleThinkingMode,
+        /// Activates the next tab in the agent panel.
+        ActivateNextTab,
+        /// Activates the previous tab in the agent panel.
+        ActivatePreviousTab,
+        /// Closes the currently active thread tab.
+        CloseActiveThreadTab,
+        /// Toggles the plan view in the thread.
+        TogglePlan,
+        /// Dismisses all OS-level agent notifications.
+        DismissOsNotifications,
+        /// Closes the currently active thread tab or docks the panel if it's the last tab.
+        CloseActiveThreadTabOrDock,
     ]
 );

@@ -281,6 +294,11 @@ pub fn init(
     context_server_configuration::init(language_registry.clone(), fs.clone(), cx);
     TextThreadEditor::init(cx);

+    // Register global action to dismiss all agent notifications
+    cx.on_action(|_: &DismissOsNotifications, cx| {
+        dismiss_all_agent_notifications(cx);
+    });
+
     register_slash_commands(cx);
     inline_assistant::init(fs.clone(), prompt_builder.clone(), cx);
     terminal_inline_assistant::init(fs.clone(), prompt_builder, cx);
@@ -334,6 +352,23 @@ pub fn init(
     .detach();
 }

+fn dismiss_all_agent_notifications(cx: &mut App) {
+    // Find all windows that contain AgentNotification and dismiss them
+    let agent_notification_windows: Vec<_> = cx
+        .windows()
+        .iter()
+        .filter_map(|window| window.downcast::<crate::ui::AgentNotification>())
+        .collect();
+
+    for window in agent_notification_windows {
+        window
+            .update(cx, |_, window, _| {
+                window.remove_window();
+            })
+            .ok();
+    }
+}
+
 fn update_command_palette_filter(cx: &mut App) {
     let disable_ai = DisableAiSettings::get_global(cx).disable_ai;
     let agent_enabled = AgentSettings::get_global(cx).enabled;
```