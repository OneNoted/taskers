use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::PathBuf,
};

use anyhow::Result;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: BTreeMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

impl CommandSpec {
    pub fn shell() -> Self {
        Self {
            program: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()),
            args: Vec::new(),
            cwd: None,
            env: BTreeMap::new(),
            cols: 120,
            rows: 40,
        }
    }
}

pub struct PtySession {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
}

pub struct PtyReader {
    reader: Box<dyn Read + Send>,
}

pub struct SpawnedPty {
    pub session: PtySession,
    pub reader: PtyReader,
}

impl PtySession {
    pub fn spawn(spec: &CommandSpec) -> Result<SpawnedPty> {
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(PtySize {
            rows: spec.rows,
            cols: spec.cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let mut command = CommandBuilder::new(&spec.program);
        for arg in &spec.args {
            command.arg(arg);
        }
        if let Some(cwd) = &spec.cwd {
            command.cwd(cwd);
        }
        for (key, value) in &spec.env {
            command.env(key, value);
        }

        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        let writer = pair.master.take_writer()?;
        let reader = pair.master.try_clone_reader()?;

        Ok(SpawnedPty {
            session: Self {
                child,
                master: pair.master,
                writer,
            },
            reader: PtyReader { reader },
        })
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    pub fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)
    }
}

impl PtyReader {
    pub fn read_into(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.reader.read(buffer)
    }
}
