use std::{future::Future, io, os::unix::net::UnixListener as StdUnixListener, path::Path};

use serde_json::{from_slice, to_vec};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::{RequestFrame, controller::InMemoryController, protocol::ResponseFrame};

pub fn bind_socket(path: impl AsRef<Path>) -> io::Result<UnixListener> {
    let path = path.as_ref();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = StdUnixListener::bind(path)?;
    listener.set_nonblocking(true)?;
    UnixListener::from_std(listener)
}

pub async fn serve<S>(
    listener: UnixListener,
    controller: InMemoryController,
    shutdown: S,
) -> io::Result<()>
where
    S: Future<Output = ()> + Send,
{
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let controller = controller.clone();
                tokio::spawn(async move {
                    let _ = handle_connection(stream, controller).await;
                });
            }
        }
    }

    Ok(())
}

async fn handle_connection(stream: UnixStream, controller: InMemoryController) -> io::Result<()> {
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let request: RequestFrame = from_slice(line.trim_end().as_bytes()).map_err(invalid_data)?;
    let response = ResponseFrame {
        request_id: request.request_id,
        response: controller
            .handle(request.command)
            .map_err(|error| error.to_string()),
    };
    let payload = to_vec(&response).map_err(invalid_data)?;
    write_half.write_all(&payload).await?;
    write_half.write_all(b"\n").await?;
    write_half.flush().await?;

    Ok(())
}

fn invalid_data(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}

#[cfg(test)]
mod tests {
    use std::{future::pending, path::PathBuf};

    use tempfile::tempdir;
    use tokio::sync::oneshot;

    use taskers_domain::AppModel;

    use crate::{
        client::ControlClient,
        controller::InMemoryController,
        protocol::{ControlCommand, ControlQuery, ControlResponse},
    };

    use super::{bind_socket, serve};

    #[tokio::test]
    async fn client_and_server_roundtrip() {
        let tempdir = tempdir().expect("tempdir");
        let socket_path = PathBuf::from(tempdir.path()).join("taskers.sock");
        let listener = bind_socket(&socket_path).expect("listener");
        let controller = InMemoryController::new(AppModel::new("Main"));
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let server = tokio::spawn(serve(listener, controller.clone(), async move {
            let _ = shutdown_rx.await;
        }));

        let client = ControlClient::new(&socket_path);
        let created = client
            .send(ControlCommand::CreateWorkspace {
                label: "Docs".into(),
            })
            .await
            .expect("create workspace request");
        assert!(matches!(
            created.response,
            Ok(ControlResponse::WorkspaceCreated { .. })
        ));

        let status = client
            .send(ControlCommand::QueryStatus {
                query: ControlQuery::All,
            })
            .await
            .expect("query request");
        match status.response {
            Ok(ControlResponse::Status { session }) => {
                assert_eq!(session.model.workspaces.len(), 2);
            }
            other => panic!("unexpected response: {other:?}"),
        }

        shutdown_tx.send(()).expect("shutdown");
        server.await.expect("server task").expect("serve cleanly");
        drop(pending::<()>());
    }
}
