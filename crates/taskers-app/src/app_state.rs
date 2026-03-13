use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use taskers_control::{ControlCommand, ControlResponse, InMemoryController};
use taskers_domain::AppModel;
use taskers_ghostty::BackendChoice;

use crate::{pane_runtime::RuntimeManager, session_store};

#[derive(Clone)]
pub struct AppState {
    controller: InMemoryController,
    runtime: RuntimeManager,
    session_path: PathBuf,
}

impl AppState {
    pub fn new(model: AppModel, session_path: PathBuf, backend: BackendChoice) -> Result<Self> {
        let controller = InMemoryController::new(model.clone());
        let runtime = RuntimeManager::new(
            controller.clone(),
            backend != BackendChoice::Ghostty,
        );
        runtime.sync_model(&model)?;

        let state = Self {
            controller,
            runtime,
            session_path,
        };
        state.persist_snapshot()?;
        Ok(state)
    }

    pub fn controller(&self) -> InMemoryController {
        self.controller.clone()
    }

    pub fn session_path(&self) -> &Path {
        &self.session_path
    }

    pub fn runtime(&self) -> RuntimeManager {
        self.runtime.clone()
    }

    pub fn snapshot_model(&self) -> AppModel {
        self.controller.snapshot().model
    }

    pub fn dispatch(&self, command: ControlCommand) -> Result<ControlResponse> {
        let response = self
            .controller
            .handle(command)
            .map_err(|error| anyhow!(error.to_string()))?;
        self.runtime.sync_model(&self.snapshot_model())?;
        self.persist_snapshot()?;
        Ok(response)
    }

    pub fn persist_snapshot(&self) -> Result<()> {
        let model = self.snapshot_model();
        self.persist_model(&model)
    }

    pub fn persist_model(&self, model: &AppModel) -> Result<()> {
        session_store::save_session(&self.session_path, model).with_context(|| {
            format!(
                "failed to save taskers session to {}",
                self.session_path.display()
            )
        })
    }
}
