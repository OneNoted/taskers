use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread,
};

use anyhow::{Context, Result, anyhow};
use taskers_control::{ControlCommand, InMemoryController};
use taskers_domain::{AppModel, PaneId, PaneKind, PaneMetadataPatch, WorkspaceId};
use taskers_runtime::{CommandSpec, PtySession, SignalStreamParser};

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
    inner: Arc<Mutex<RuntimeManagerInner>>,
}

struct RuntimeManagerInner {
    panes: HashMap<PaneId, PaneRuntime>,
}

struct PaneRuntime {
    session: Arc<Mutex<PtySession>>,
    output: Arc<Mutex<String>>,
    process_id: Option<u32>,
}

impl RuntimeManager {
    pub fn new(controller: InMemoryController, enabled: bool) -> Self {
        Self {
            enabled,
            controller,
            inner: Arc::new(Mutex::new(RuntimeManagerInner {
                panes: HashMap::new(),
            })),
        }
    }

    pub fn sync_model(&self, model: &AppModel) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let model_pane_ids: std::collections::HashSet<PaneId> = model
            .workspaces
            .values()
            .flat_map(|ws| ws.panes.keys().copied())
            .collect();

        {
            let mut inner = self.inner.lock().expect("runtime manager mutex poisoned");
            inner.panes.retain(|id, _| model_pane_ids.contains(id));
        }

        for (workspace_id, workspace) in &model.workspaces {
            for pane in workspace.panes.values() {
                if pane.kind != PaneKind::Terminal {
                    continue;
                }

                let mut inner = self.inner.lock().expect("runtime manager mutex poisoned");
                if inner.panes.contains_key(&pane.id) {
                    continue;
                }

                let runtime = spawn_pane_runtime(
                    self.controller.clone(),
                    *workspace_id,
                    pane.id,
                    pane.metadata.cwd.as_deref().map(PathBuf::from),
                )
                .with_context(|| format!("failed to spawn shell runtime for pane {}", pane.id))?;
                inner.panes.insert(pane.id, runtime);
            }
        }

        Ok(())
    }

    pub fn snapshot(&self, pane_id: PaneId) -> Option<PaneRuntimeSnapshot> {
        if !self.enabled {
            return None;
        }

        let inner = self.inner.lock().expect("runtime manager mutex poisoned");
        let runtime = inner.panes.get(&pane_id)?;
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

    pub fn send_input(&self, pane_id: PaneId, input: &str) -> Result<()> {
        if !self.enabled {
            return Err(anyhow!("pane {pane_id} is using the Ghostty backend"));
        }

        let session = {
            let inner = self.inner.lock().expect("runtime manager mutex poisoned");
            inner
                .panes
                .get(&pane_id)
                .map(|runtime| Arc::clone(&runtime.session))
                .ok_or_else(|| anyhow!("pane {pane_id} has no live runtime"))?
        };

        let mut session = session.lock().expect("pty session mutex poisoned");
        session
            .write_all(input.as_bytes())
            .with_context(|| format!("failed to send input to pane {pane_id}"))?;
        Ok(())
    }
}

fn spawn_pane_runtime(
    controller: InMemoryController,
    workspace_id: WorkspaceId,
    pane_id: PaneId,
    cwd: Option<PathBuf>,
) -> Result<PaneRuntime> {
    let mut spec = CommandSpec::shell();
    spec.cwd = cwd;
    spec.env.insert("TERM".into(), "xterm-256color".into());
    spec.env
        .insert("TASKERS_PANE_ID".into(), pane_id.to_string());
    spec.env
        .insert("TASKERS_WORKSPACE_ID".into(), workspace_id.to_string());

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
                    let _ = controller.handle(ControlCommand::ClosePane {
                        workspace_id,
                        pane_id,
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
                            event: signal.clone().into_event("pty"),
                        });

                        if let Some(title) = signal.title {
                            let _ = controller.handle(ControlCommand::UpdatePaneMetadata {
                                pane_id,
                                patch: PaneMetadataPatch {
                                    title: Some(title),
                                    cwd: None,
                                    repo_name: None,
                                    git_branch: None,
                                    ports: None,
                                    agent_kind: None,
                                },
                            });
                        }
                    }
                }
                Err(_) => {
                    let _ = controller.handle(ControlCommand::ClosePane {
                        workspace_id,
                        pane_id,
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
                            if byte == 0x07 {
                                index += 1;
                                break;
                            }
                            if byte == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\'
                            {
                                index += 2;
                                break;
                            }
                            index += 1;
                        }
                    }
                    _ => {
                        index += 1;
                    }
                }
            }
            _ => {
                result.push(bytes[index] as char);
                index += 1;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::sanitize_terminal_output;

    #[test]
    fn strips_common_terminal_escape_sequences() {
        let cleaned = sanitize_terminal_output(
            "\u{1b}[32mgreen\u{1b}[0m text \u{1b}]777;taskers;kind=completed;message=done\u{7}",
        );
        assert_eq!(cleaned, "green text ");
    }
}
