use std::{
    collections::{BTreeMap, HashMap},
    env, fs,
    io::{self, BufRead, BufReader, Read, Write},
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
};

use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};

use crate::{CommandSpec, PtySession, SignalStreamParser};

const MAX_TRANSCRIPT_BYTES: usize = 262_144;
const OUTPUT_CHUNK_BYTES: usize = 8192;
const ATTACH_ENV_KEYS: &[&str] = &[
    "HOME",
    "PATH",
    "SHELL",
    "TERM_PROGRAM",
    "TASKERS_AGENT_SESSION_ID",
    "TASKERS_CTL_PATH",
    "TASKERS_DISABLE_SHELL_INTEGRATION",
    "TASKERS_EMBEDDED",
    "TASKERS_PANE_ID",
    "TASKERS_REAL_SHELL",
    "TASKERS_SHELL_INTEGRATION_DIR",
    "TASKERS_SOCKET",
    "TASKERS_SURFACE_ID",
    "TASKERS_TERMINAL_SESSION_ID",
    "TASKERS_TERMINAL_SOCKET",
    "TASKERS_USER_BASHRC",
    "TASKERS_USER_ZDOTDIR",
    "TASKERS_WORKSPACE_ID",
    "TASKERS_TTY_NAME",
    "ZDOTDIR",
];

#[derive(Debug, Clone)]
pub struct TerminalSessionClient {
    socket_path: PathBuf,
}

impl TerminalSessionClient {
    pub fn new(socket_path: PathBuf) -> Self {
        Self { socket_path }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn ping(&self) -> Result<()> {
        let mut stream = self.connect()?;
        write_request(&mut stream, &SessionRequest::Ping)?;
        match read_event(&mut BufReader::new(stream))? {
            SessionEvent::Pong => Ok(()),
            other => bail!("unexpected ping response: {other:?}"),
        }
    }

    pub fn has_session(&self, session_id: &str) -> Result<bool> {
        let mut stream = self.connect()?;
        write_request(
            &mut stream,
            &SessionRequest::HasSession {
                session_id: session_id.into(),
            },
        )?;
        match read_event(&mut BufReader::new(stream))? {
            SessionEvent::Exists { exists } => Ok(exists),
            SessionEvent::Error { message } => Err(anyhow!(message)),
            other => bail!("unexpected session lookup response: {other:?}"),
        }
    }

    pub fn terminate_session(&self, session_id: &str) -> Result<()> {
        let mut stream = self.connect()?;
        write_request(
            &mut stream,
            &SessionRequest::Terminate {
                session_id: session_id.into(),
            },
        )?;
        match read_event(&mut BufReader::new(stream))? {
            SessionEvent::Ack => Ok(()),
            SessionEvent::Error { message } => Err(anyhow!(message)),
            other => bail!("unexpected terminate response: {other:?}"),
        }
    }

    pub fn attach_or_create(&self, session_id: &str, shell_args: &[String]) -> Result<()> {
        let mut stream = self.connect()?;
        let attach = SessionRequest::Attach {
            session_id: session_id.into(),
            cols: 120,
            rows: 40,
            cwd: env::current_dir()
                .ok()
                .map(|path| path.display().to_string()),
            shell_args: shell_args.to_vec(),
            env: collect_attach_env(),
        };
        write_request(&mut stream, &attach)?;

        let read_stream = stream
            .try_clone()
            .context("failed to clone session socket")?;
        let reader = thread::spawn(move || -> Result<()> {
            let mut reader = BufReader::new(read_stream);
            let mut stdout = io::stdout().lock();
            loop {
                match read_event(&mut reader)? {
                    SessionEvent::Attached => {}
                    SessionEvent::Output { data_b64 } => {
                        let bytes = BASE64
                            .decode(data_b64)
                            .context("failed to decode sidecar output")?;
                        stdout
                            .write_all(&bytes)
                            .context("failed to write sidecar output")?;
                        stdout.flush().ok();
                    }
                    SessionEvent::Closed => return Ok(()),
                    SessionEvent::Error { message } => return Err(anyhow!(message)),
                    other => bail!("unexpected attach event: {other:?}"),
                }
            }
        });

        let mut write_stream = stream;
        let writer = thread::spawn(move || -> Result<()> {
            let mut stdin = io::stdin().lock();
            let mut buffer = [0u8; 4096];
            loop {
                let bytes_read = stdin.read(&mut buffer).context("failed to read stdin")?;
                if bytes_read == 0 {
                    write_request(&mut write_stream, &SessionRequest::Detach)?;
                    return Ok(());
                }
                write_request(
                    &mut write_stream,
                    &SessionRequest::Input {
                        data_b64: BASE64.encode(&buffer[..bytes_read]),
                    },
                )?;
            }
        });

        let write_result = writer
            .join()
            .map_err(|_| anyhow!("session writer panicked"))?;
        let read_result = reader
            .join()
            .map_err(|_| anyhow!("session reader panicked"))?;
        write_result?;
        read_result
    }

    fn connect(&self) -> Result<UnixStream> {
        UnixStream::connect(&self.socket_path).with_context(|| {
            format!(
                "failed to connect to terminal session socket {}",
                self.socket_path.display()
            )
        })
    }
}

#[derive(Clone)]
pub struct TerminalSessionDaemon {
    sessions: Arc<Mutex<HashMap<String, SessionState>>>,
    next_client_id: Arc<AtomicU64>,
}

struct SessionState {
    pty: Arc<Mutex<PtySession>>,
    transcript: Vec<u8>,
    clients: HashMap<u64, mpsc::Sender<SessionEvent>>,
}

impl TerminalSessionDaemon {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_client_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn serve(&self, socket_path: &Path) -> Result<()> {
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create terminal session socket directory {}",
                    parent.display()
                )
            })?;
        }
        if socket_path.exists() {
            fs::remove_file(socket_path).with_context(|| {
                format!("failed to remove stale socket {}", socket_path.display())
            })?;
        }

        let listener = UnixListener::bind(socket_path)
            .with_context(|| format!("failed to bind {}", socket_path.display()))?;
        for stream in listener.incoming() {
            let stream = match stream {
                Ok(stream) => stream,
                Err(error) => {
                    eprintln!("terminal session accept failed: {error}");
                    continue;
                }
            };
            let daemon = self.clone();
            thread::spawn(move || {
                if let Err(error) = daemon.handle_connection(stream) {
                    eprintln!("terminal session connection failed: {error:?}");
                }
            });
        }
        Ok(())
    }

    fn handle_connection(&self, mut stream: UnixStream) -> Result<()> {
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .context("failed to clone terminal session stream")?,
        );
        let request = read_request(&mut reader)?;
        match request {
            SessionRequest::Ping => {
                write_event(&mut stream, &SessionEvent::Pong)?;
            }
            SessionRequest::HasSession { session_id } => {
                let exists = self
                    .sessions
                    .lock()
                    .expect("session daemon mutex poisoned")
                    .contains_key(&session_id);
                write_event(&mut stream, &SessionEvent::Exists { exists })?;
            }
            SessionRequest::Terminate { session_id } => {
                let state = self
                    .sessions
                    .lock()
                    .expect("session daemon mutex poisoned")
                    .remove(&session_id);
                if let Some(state) = state {
                    state
                        .pty
                        .lock()
                        .expect("pty session mutex poisoned")
                        .kill()
                        .ok();
                }
                write_event(&mut stream, &SessionEvent::Ack)?;
            }
            SessionRequest::Attach {
                session_id,
                cols,
                rows,
                cwd,
                shell_args,
                env,
            } => {
                self.attach_client(stream, session_id, cols, rows, cwd, shell_args, env)?;
            }
            other => bail!("unexpected initial request: {other:?}"),
        }
        Ok(())
    }

    fn attach_client(
        &self,
        stream: UnixStream,
        session_id: String,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        shell_args: Vec<String>,
        env: BTreeMap<String, String>,
    ) -> Result<()> {
        self.ensure_session(&session_id, cols, rows, cwd, shell_args, env)?;

        let client_id = self.next_client_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel::<SessionEvent>();
        let transcript = {
            let mut sessions = self.sessions.lock().expect("session daemon mutex poisoned");
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| anyhow!("session {session_id} was not created"))?;
            session.clients.insert(client_id, tx.clone());
            session.transcript.clone()
        };

        tx.send(SessionEvent::Attached).ok();
        queue_output(&tx, &transcript);

        let writer_stream = stream
            .try_clone()
            .context("failed to clone client stream")?;
        let writer = thread::spawn(move || -> Result<()> {
            let mut writer = writer_stream;
            while let Ok(event) = rx.recv() {
                write_event(&mut writer, &event)?;
            }
            Ok(())
        });

        let mut reader = BufReader::new(stream);
        loop {
            match read_request(&mut reader) {
                Ok(SessionRequest::Input { data_b64 }) => {
                    let bytes = BASE64
                        .decode(data_b64)
                        .context("failed to decode session input")?;
                    let pty = {
                        let sessions = self.sessions.lock().expect("session daemon mutex poisoned");
                        sessions
                            .get(&session_id)
                            .map(|session| Arc::clone(&session.pty))
                            .ok_or_else(|| anyhow!("session {session_id} vanished"))?
                    };
                    pty.lock()
                        .expect("pty session mutex poisoned")
                        .write_all(&bytes)
                        .context("failed to write session input")?;
                }
                Ok(SessionRequest::Resize { cols, rows }) => {
                    let pty = {
                        let sessions = self.sessions.lock().expect("session daemon mutex poisoned");
                        sessions
                            .get(&session_id)
                            .map(|session| Arc::clone(&session.pty))
                            .ok_or_else(|| anyhow!("session {session_id} vanished"))?
                    };
                    pty.lock()
                        .expect("pty session mutex poisoned")
                        .resize(cols, rows)
                        .context("failed to resize session")?;
                }
                Ok(SessionRequest::Detach) => break,
                Ok(other) => bail!("unexpected attach request: {other:?}"),
                Err(error) => {
                    if is_unexpected_eof(&error) {
                        break;
                    }
                    return Err(error);
                }
            }
        }

        {
            let mut sessions = self.sessions.lock().expect("session daemon mutex poisoned");
            if let Some(session) = sessions.get_mut(&session_id) {
                session.clients.remove(&client_id);
            }
        }
        drop(tx);
        writer
            .join()
            .map_err(|_| anyhow!("client writer panicked"))??;
        Ok(())
    }

    fn ensure_session(
        &self,
        session_id: &str,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        shell_args: Vec<String>,
        env: BTreeMap<String, String>,
    ) -> Result<()> {
        if self
            .sessions
            .lock()
            .expect("session daemon mutex poisoned")
            .contains_key(session_id)
        {
            return Ok(());
        }

        let spec = build_command_spec(cols, rows, cwd, shell_args, env)?;
        let spawned = PtySession::spawn(&spec)?;
        let pty = Arc::new(Mutex::new(spawned.session));
        let reader = spawned.reader;
        self.sessions
            .lock()
            .expect("session daemon mutex poisoned")
            .insert(
                session_id.into(),
                SessionState {
                    pty: Arc::clone(&pty),
                    transcript: Vec::new(),
                    clients: HashMap::new(),
                },
            );
        self.spawn_reader(session_id.to_string(), reader);
        Ok(())
    }

    fn spawn_reader(&self, session_id: String, mut reader: crate::PtyReader) {
        let sessions = Arc::clone(&self.sessions);
        thread::spawn(move || {
            let mut buffer = [0u8; 4096];
            let mut signal_parser = SignalStreamParser::default();
            loop {
                let bytes_read = match reader.read_into(&mut buffer) {
                    Ok(0) => break,
                    Ok(bytes_read) => bytes_read,
                    Err(_) => break,
                };
                let chunk = &buffer[..bytes_read];
                let mut clients = Vec::new();
                {
                    let mut all_sessions = sessions.lock().expect("session daemon mutex poisoned");
                    let Some(session) = all_sessions.get_mut(&session_id) else {
                        break;
                    };
                    session.transcript.extend_from_slice(chunk);
                    trim_transcript(&mut session.transcript);
                    clients.extend(session.clients.values().cloned());
                }

                for _event in signal_parser.push_events(&String::from_utf8_lossy(chunk)) {}
                for client in clients {
                    queue_output(&client, chunk);
                }
            }

            let clients = {
                let mut all_sessions = sessions.lock().expect("session daemon mutex poisoned");
                all_sessions
                    .remove(&session_id)
                    .map(|session| session.clients.into_values().collect::<Vec<_>>())
                    .unwrap_or_default()
            };
            for client in clients {
                client.send(SessionEvent::Closed).ok();
            }
        });
    }
}

fn build_command_spec(
    cols: u16,
    rows: u16,
    cwd: Option<String>,
    mut shell_args: Vec<String>,
    mut env_map: BTreeMap<String, String>,
) -> Result<CommandSpec> {
    let integration_dir = env_map
        .get("TASKERS_SHELL_INTEGRATION_DIR")
        .cloned()
        .ok_or_else(|| anyhow!("missing TASKERS_SHELL_INTEGRATION_DIR"))?;
    env_map.insert("TASKERS_SESSION_CHILD".into(), "1".into());
    let wrapper_path = PathBuf::from(integration_dir).join("taskers-shell-wrapper.sh");
    let mut args = vec![wrapper_path.display().to_string()];
    args.append(&mut shell_args);

    Ok(CommandSpec {
        program: "sh".into(),
        args,
        cwd: cwd.map(PathBuf::from),
        env: env_map,
        cols,
        rows,
    })
}

fn collect_attach_env() -> BTreeMap<String, String> {
    let mut env_map = BTreeMap::new();
    for key in ATTACH_ENV_KEYS {
        if let Ok(value) = env::var(key) {
            env_map.insert((*key).into(), value);
        }
    }
    env_map
}

fn trim_transcript(transcript: &mut Vec<u8>) {
    if transcript.len() <= MAX_TRANSCRIPT_BYTES {
        return;
    }
    let drop_len = transcript.len() - MAX_TRANSCRIPT_BYTES;
    transcript.drain(..drop_len);
}

fn queue_output(sender: &mpsc::Sender<SessionEvent>, bytes: &[u8]) {
    for chunk in bytes.chunks(OUTPUT_CHUNK_BYTES) {
        sender
            .send(SessionEvent::Output {
                data_b64: BASE64.encode(chunk),
            })
            .ok();
    }
}

fn write_request(stream: &mut UnixStream, request: &SessionRequest) -> Result<()> {
    let data = serde_json::to_vec(request).context("failed to serialize session request")?;
    stream
        .write_all(&data)
        .context("failed to write session request")?;
    stream
        .write_all(b"\n")
        .context("failed to terminate session request")?;
    stream.flush().ok();
    Ok(())
}

fn write_event(stream: &mut UnixStream, event: &SessionEvent) -> Result<()> {
    let data = serde_json::to_vec(event).context("failed to serialize session event")?;
    stream
        .write_all(&data)
        .context("failed to write session event")?;
    stream
        .write_all(b"\n")
        .context("failed to terminate session event")?;
    stream.flush().ok();
    Ok(())
}

fn read_request(reader: &mut BufReader<UnixStream>) -> Result<SessionRequest> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .context("failed to read session request")?;
    if bytes == 0 {
        bail!("unexpected EOF while reading session request");
    }
    serde_json::from_str(line.trim_end()).context("failed to parse session request")
}

fn read_event(reader: &mut BufReader<UnixStream>) -> Result<SessionEvent> {
    let mut line = String::new();
    let bytes = reader
        .read_line(&mut line)
        .context("failed to read session event")?;
    if bytes == 0 {
        bail!("unexpected EOF while reading session event");
    }
    serde_json::from_str(line.trim_end()).context("failed to parse session event")
}

fn is_unexpected_eof(error: &anyhow::Error) -> bool {
    error
        .to_string()
        .contains("unexpected EOF while reading session request")
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionRequest {
    Ping,
    HasSession {
        session_id: String,
    },
    Terminate {
        session_id: String,
    },
    Attach {
        session_id: String,
        cols: u16,
        rows: u16,
        cwd: Option<String>,
        shell_args: Vec<String>,
        env: BTreeMap<String, String>,
    },
    Input {
        data_b64: String,
    },
    Resize {
        cols: u16,
        rows: u16,
    },
    Detach,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum SessionEvent {
    Pong,
    Exists { exists: bool },
    Ack,
    Attached,
    Output { data_b64: String },
    Closed,
    Error { message: String },
}

#[cfg(test)]
mod tests {
    use super::collect_attach_env;

    #[test]
    fn collect_attach_env_preserves_terminal_session_identity() {
        unsafe {
            std::env::set_var("TASKERS_TERMINAL_SESSION_ID", "session-123");
        }
        let env = collect_attach_env();
        assert_eq!(
            env.get("TASKERS_TERMINAL_SESSION_ID").map(String::as_str),
            Some("session-123")
        );
        unsafe {
            std::env::remove_var("TASKERS_TERMINAL_SESSION_ID");
        }
    }
}
