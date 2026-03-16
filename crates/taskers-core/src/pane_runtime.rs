use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use anyhow::{Context, Result, anyhow};
use taskers_control::{ControlCommand, InMemoryController};
use taskers_domain::{AppModel, PaneId, PaneKind, SurfaceId, WorkspaceId};
use taskers_runtime::{CommandSpec, PtySession, ShellLaunchSpec, SignalStreamParser};

const MAX_OUTPUT_CHARS: usize = 24_000;

#[derive(Debug, Clone)]
pub struct PaneRuntimeSnapshot {
    pub output: String,
    pub process_id: Option<u32>,
}

#[derive(Clone)]
pub struct RuntimeManager {
    enabled: bool,
    controller: InMemoryController,
    shell_launch: ShellLaunchSpec,
    inner: Arc<Mutex<RuntimeManagerInner>>,
}

struct RuntimeManagerInner {
    surfaces: HashMap<SurfaceId, PaneRuntime>,
}

struct PaneRuntime {
    session: Arc<Mutex<PtySession>>,
    output: Arc<Mutex<String>>,
    process_id: Option<u32>,
}

impl RuntimeManager {
    pub fn new(
        controller: InMemoryController,
        enabled: bool,
        shell_launch: ShellLaunchSpec,
    ) -> Self {
        Self {
            enabled,
            controller,
            shell_launch,
            inner: Arc::new(Mutex::new(RuntimeManagerInner {
                surfaces: HashMap::new(),
            })),
        }
    }

    pub fn sync_model(&self, model: &AppModel) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let model_surface_ids: std::collections::HashSet<SurfaceId> = model
            .workspaces
            .values()
            .flat_map(|ws| {
                ws.panes
                    .values()
                    .flat_map(|pane| pane.surface_ids())
                    .collect::<Vec<_>>()
            })
            .collect();

        {
            let mut inner = self.inner.lock().expect("runtime manager mutex poisoned");
            inner
                .surfaces
                .retain(|id, _| model_surface_ids.contains(id));
        }

        for (workspace_id, workspace) in &model.workspaces {
            for pane in workspace.panes.values() {
                for surface in pane.surfaces.values() {
                    if surface.kind != PaneKind::Terminal {
                        continue;
                    }

                    let mut inner = self.inner.lock().expect("runtime manager mutex poisoned");
                    if inner.surfaces.contains_key(&surface.id) {
                        continue;
                    }

                    let runtime = spawn_surface_runtime(
                        self.controller.clone(),
                        self.shell_launch.clone(),
                        *workspace_id,
                        pane.id,
                        surface.id,
                        surface.metadata.cwd.as_deref().map(PathBuf::from),
                    )
                    .with_context(|| {
                        format!("failed to spawn shell runtime for surface {}", surface.id)
                    })?;
                    inner.surfaces.insert(surface.id, runtime);
                }
            }
        }

        Ok(())
    }

    pub fn snapshot(&self, surface_id: SurfaceId) -> Option<PaneRuntimeSnapshot> {
        if !self.enabled {
            return None;
        }

        let inner = self.inner.lock().expect("runtime manager mutex poisoned");
        let runtime = inner.surfaces.get(&surface_id)?;
        let output = runtime
            .output
            .lock()
            .expect("pane output mutex poisoned")
            .clone();

        Some(PaneRuntimeSnapshot {
            output,
            process_id: runtime.process_id,
        })
    }

    pub fn send_input(&self, surface_id: SurfaceId, input: &str) -> Result<()> {
        if !self.enabled {
            return Err(anyhow!("surface {surface_id} is using the Ghostty backend"));
        }

        let session = {
            let inner = self.inner.lock().expect("runtime manager mutex poisoned");
            inner
                .surfaces
                .get(&surface_id)
                .map(|runtime| Arc::clone(&runtime.session))
                .ok_or_else(|| anyhow!("surface {surface_id} has no live runtime"))?
        };

        let mut session = session.lock().expect("pty session mutex poisoned");
        session
            .write_all(input.as_bytes())
            .with_context(|| format!("failed to send input to surface {surface_id}"))?;
        Ok(())
    }
}

fn spawn_surface_runtime(
    controller: InMemoryController,
    shell_launch: ShellLaunchSpec,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    surface_id: SurfaceId,
    cwd: Option<PathBuf>,
) -> Result<PaneRuntime> {
    let mut spec = CommandSpec::new(shell_launch.program.display().to_string());
    spec.args = shell_launch.args;
    spec.cwd = cwd;
    spec.env.extend(shell_launch.env);
    spec.env
        .entry("TERM".into())
        .or_insert_with(|| "xterm-256color".into());
    spec.env
        .insert("TASKERS_PANE_ID".into(), pane_id.to_string());
    spec.env
        .insert("TASKERS_WORKSPACE_ID".into(), workspace_id.to_string());
    spec.env
        .insert("TASKERS_SURFACE_ID".into(), surface_id.to_string());

    let spawned = PtySession::spawn(&spec)?;
    let process_id = spawned.session.process_id();
    let output = Arc::new(Mutex::new(String::new()));
    let session = Arc::new(Mutex::new(spawned.session));

    let reader_output = Arc::clone(&output);
    thread::spawn(move || {
        let mut reader = spawned.reader;
        let mut buffer = [0u8; 4096];
        let mut signal_parser = SignalStreamParser::default();

        loop {
            match reader.read_into(&mut buffer) {
                Ok(0) => {
                    let _ = controller.handle(ControlCommand::CloseSurface {
                        workspace_id,
                        pane_id,
                        surface_id,
                    });
                    break;
                }
                Ok(bytes_read) => {
                    let chunk = String::from_utf8_lossy(&buffer[..bytes_read]).to_string();
                    let clean = sanitize_terminal_output(&chunk);
                    if !clean.is_empty() {
                        append_output(&reader_output, &clean);
                    }

                    for signal in signal_parser.push(&chunk) {
                        let _ = controller.handle(ControlCommand::EmitSignal {
                            workspace_id,
                            pane_id,
                            surface_id: Some(surface_id),
                            event: signal.clone().into_event("pty"),
                        });
                    }
                }
                Err(_) => {
                    let _ = controller.handle(ControlCommand::CloseSurface {
                        workspace_id,
                        pane_id,
                        surface_id,
                    });
                    break;
                }
            }
        }
    });

    Ok(PaneRuntime {
        session,
        output,
        process_id,
    })
}

fn append_output(output: &Arc<Mutex<String>>, chunk: &str) {
    let mut output = output.lock().expect("pane output mutex poisoned");
    output.push_str(chunk);

    if output.chars().count() > MAX_OUTPUT_CHARS {
        let trimmed = output
            .chars()
            .rev()
            .take(MAX_OUTPUT_CHARS)
            .collect::<Vec<_>>();
        *output = trimmed.into_iter().rev().collect();
    }
}

fn sanitize_terminal_output(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut result = String::with_capacity(input.len());
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'\r' => {
                index += 1;
            }
            0x1b => {
                index += 1;
                if index >= bytes.len() {
                    break;
                }

                match bytes[index] {
                    b'[' => {
                        index += 1;
                        while index < bytes.len() {
                            let byte = bytes[index];
                            index += 1;
                            if (0x40..=0x7e).contains(&byte) {
                                break;
                            }
                        }
                    }
                    b']' => {
                        index += 1;
                        while index < bytes.len() {
                            let byte = bytes[index];
                            index += 1;
                            if byte == 0x07 {
                                break;
                            }
                            if byte == 0x1b && bytes.get(index) == Some(&b'\\') {
                                index += 1;
                                break;
                            }
                        }
                    }
                    _ => {
                        index += 1;
                    }
                }
            }
            byte if byte.is_ascii_control() && byte != b'\n' && byte != b'\t' => {
                index += 1;
            }
            _ => {
                let start = index;
                index += 1;
                while index < bytes.len()
                    && bytes[index] != 0x1b
                    && bytes[index] != b'\r'
                    && (!bytes[index].is_ascii_control()
                        || bytes[index] == b'\n'
                        || bytes[index] == b'\t')
                {
                    index += 1;
                }
                result.push_str(&input[start..index]);
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_output;

    #[test]
    fn strips_control_sequences_from_terminal_output() {
        let input = "\u{1b}[31mhello\u{1b}[0m\r\nworld\u{1b}]2;title\u{7}";
        assert_eq!(sanitize_terminal_output(input), "hello\nworld");
    }
}
