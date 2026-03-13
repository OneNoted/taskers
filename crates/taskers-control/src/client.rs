use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{from_slice, to_vec};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
};

use crate::protocol::{ControlCommand, RequestFrame, ResponseFrame};

#[derive(Debug, Clone)]
pub struct ControlClient {
    socket_path: PathBuf,
}

impl ControlClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub async fn send(&self, command: ControlCommand) -> Result<ResponseFrame> {
        let mut stream = UnixStream::connect(&self.socket_path).await?;
        let request = RequestFrame::new(command);
        let payload = to_vec(&request)?;

        stream.write_all(&payload).await?;
        stream.write_all(b"\n").await?;
        stream.flush().await?;

        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        reader.read_line(&mut response).await?;

        Ok(from_slice(response.trim_end().as_bytes())?)
    }
}
