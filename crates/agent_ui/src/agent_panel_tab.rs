use std::path::Path;
use std::sync::Arc;

use crate::agent_panel::ActiveView;
use agent_client_protocol as acp;
use gpui::AnyElement;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TabId(pub usize);

pub struct AgentPanelTab {
    pub id: TabId,
    pub view: ActiveView,
}

impl AgentPanelTab {
    pub fn new(id: TabId, view: ActiveView) -> Self {
        Self {
            id,
            view,
        }
    }
}

pub struct TabLabelRender {
    pub label: String,
    pub icon: Option<AnyElement>,
    pub indicator: Option<AnyElement>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum AgentPanelTabIdentity {
    AcpThread(acp::SessionId),
    TextThread(Arc<Path>),
}
