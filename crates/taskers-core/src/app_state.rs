use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use taskers_control::{ControlCommand, ControlResponse, InMemoryController};
use taskers_domain::{AppModel, PaneId, WorkspaceId};
use taskers_ghostty::{BackendChoice, SurfaceDescriptor};
use taskers_runtime::ShellLaunchSpec;

use crate::{pane_runtime::RuntimeManager, session_store};

#[derive(Clone)]
pub struct AppState {
    controller: InMemoryController,
    runtime: RuntimeManager,
    session_path: PathBuf,
    shell_launch: ShellLaunchSpec,
}

impl AppState {
    pub fn new(
        model: AppModel,
        session_path: PathBuf,
        backend: BackendChoice,
        shell_launch: ShellLaunchSpec,
    ) -> Result<Self> {
        let controller = InMemoryController::new(model.clone());
        let runtime = RuntimeManager::new(
            controller.clone(),
            backend != BackendChoice::Ghostty,
            shell_launch.clone(),
        );
        runtime.sync_model(&model)?;

        let state = Self {
            controller,
            runtime,
            session_path,
            shell_launch,
        };
        state.persist_snapshot()?;
        Ok(state)
    }

    pub fn controller(&self) -> InMemoryController {
        self.controller.clone()
    }

    pub fn runtime(&self) -> RuntimeManager {
        self.runtime.clone()
    }

    pub fn shell_launch(&self) -> &ShellLaunchSpec {
        &self.shell_launch
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

    pub fn surface_descriptor_for_pane(
        &self,
        workspace_id: WorkspaceId,
        pane_id: PaneId,
    ) -> Result<SurfaceDescriptor> {
        let model = self.snapshot_model();
        let workspace = model
            .workspaces
            .get(&workspace_id)
            .ok_or_else(|| anyhow!("workspace {workspace_id} is not present"))?;
        let pane = workspace
            .panes
            .get(&pane_id)
            .ok_or_else(|| anyhow!("pane {pane_id} is not present"))?;
        let surface = pane
            .active_surface()
            .ok_or_else(|| anyhow!("pane {pane_id} has no active surface"))?;

        let mut env = self.shell_launch.env.clone();
        env.insert("TASKERS_PANE_ID".into(), pane.id.to_string());
        env.insert("TASKERS_WORKSPACE_ID".into(), workspace_id.to_string());
        env.insert("TASKERS_SURFACE_ID".into(), surface.id.to_string());

        Ok(SurfaceDescriptor {
            cols: 120,
            rows: 40,
            cwd: surface.metadata.cwd.clone(),
            title: surface.metadata.title.clone(),
            command_argv: self.shell_launch.program_and_args(),
            env,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use taskers_domain::AppModel;
    use taskers_ghostty::BackendChoice;
    use taskers_runtime::ShellLaunchSpec;

    use super::AppState;

    #[test]
    fn surface_descriptor_includes_shell_launch_and_surface_metadata() {
        let model = AppModel::new("Main");
        let workspace = model.active_workspace_id().expect("workspace");
        let pane = model.active_workspace().expect("workspace").active_pane;

        let mut shell_launch = ShellLaunchSpec::fallback();
        shell_launch.program = PathBuf::from("/bin/zsh");
        shell_launch.args = vec!["-i".into()];
        shell_launch
            .env
            .insert("TASKERS_SOCKET".into(), "/tmp/taskers.sock".into());

        let app_state = AppState::new(
            model,
            PathBuf::from("/tmp/taskers-session.json"),
            BackendChoice::Mock,
            shell_launch,
        )
        .expect("app state");

        let descriptor = app_state
            .surface_descriptor_for_pane(workspace, pane)
            .expect("descriptor");

        assert_eq!(descriptor.command_argv, vec!["/bin/zsh", "-i"]);
        assert_eq!(
            descriptor.env.get("TASKERS_WORKSPACE_ID"),
            Some(&workspace.to_string())
        );
        assert_eq!(descriptor.env.get("TASKERS_PANE_ID"), Some(&pane.to_string()));
        assert!(descriptor.env.contains_key("TASKERS_SURFACE_ID"));
    }
}
