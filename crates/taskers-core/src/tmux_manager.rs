use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Result;
use taskers_domain::{AppModel, PaneKind, SessionId, SurfaceId};
use taskers_runtime::TerminalSessionClient;

#[derive(Clone)]
pub struct TerminalSessionManager {
    client: Option<TerminalSessionClient>,
    inner: Arc<Mutex<HashMap<SurfaceId, SessionId>>>,
}

impl TerminalSessionManager {
    pub fn new(client: Option<TerminalSessionClient>) -> Self {
        Self {
            client,
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn sync_model(&self, model: &AppModel) -> Result<()> {
        let current = model_terminal_sessions(model);
        let removed = {
            let mut inner = self
                .inner
                .lock()
                .expect("terminal session manager mutex poisoned");
            let removed = inner
                .iter()
                .filter_map(|(surface_id, session_id)| {
                    (!current.contains_key(surface_id)).then_some(*session_id)
                })
                .collect::<Vec<_>>();
            *inner = current;
            removed
        };

        let Some(client) = self.client.as_ref() else {
            return Ok(());
        };
        for session_id in removed {
            client.terminate_session(&session_id.to_string())?;
        }
        Ok(())
    }
}

fn model_terminal_sessions(model: &AppModel) -> HashMap<SurfaceId, SessionId> {
    model
        .workspaces
        .values()
        .flat_map(|workspace| workspace.panes.values())
        .flat_map(|pane| pane.surfaces.values())
        .filter(|surface| surface.kind == PaneKind::Terminal)
        .map(|surface| (surface.id, surface.session_id))
        .collect()
}
