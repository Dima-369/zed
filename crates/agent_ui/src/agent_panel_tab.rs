use std::path::Path;
use std::sync::Arc;

use crate::agent_panel::{ActiveView, AgentType};
use agent_client_protocol as acp;
use gpui::{AnyElement, SharedString};

pub type TabId = usize;

pub struct AgentPanelTab {
    pub view: ActiveView,
    pub agent: AgentType,
}

impl AgentPanelTab {
    pub fn new(view: ActiveView, agent: AgentType) -> Self {
        Self { view, agent }
    }

    pub fn view(&self) -> &ActiveView {
        &self.view
    }

    pub fn agent(&self) -> &AgentType {
        &self.agent
    }
}

pub struct TabLabelRender {
    pub element: AnyElement,
    pub tooltip: Option<SharedString>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum AgentPanelTabIdentity {
    AcpThread(acp::SessionId),
    TextThread(Arc<Path>),
}
