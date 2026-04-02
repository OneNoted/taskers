use std::{
    fs,
    future::Future,
    io,
    os::unix::{fs::PermissionsExt, net::UnixListener as StdUnixListener},
    path::Path,
};

use serde_json::{from_slice, to_vec};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

use crate::{
    RequestFrame,
    controller::InMemoryController,
    protocol::{ControlCommand, ControlError, ControlResponse, ResponseFrame},
};

pub fn bind_socket(path: impl AsRef<Path>) -> io::Result<UnixListener> {
    let path = path.as_ref();
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    let listener = StdUnixListener::bind(path)?;
    set_private_socket_permissions(path)?;
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
    serve_with_handler(
        listener,
        move |command| {
            let controller = controller.clone();
            async move {
                controller
                    .handle(command)
                    .map_err(|error| ControlError::internal(error.to_string()))
            }
        },
        shutdown,
    )
    .await
}

pub async fn serve_with_handler<S, H, F>(
    listener: UnixListener,
    handler: H,
    shutdown: S,
) -> io::Result<()>
where
    S: Future<Output = ()> + Send,
    H: Fn(ControlCommand) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = Result<ControlResponse, ControlError>> + Send + 'static,
{
    tokio::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let handler = handler.clone();
                tokio::spawn(async move {
                    let _ = handle_connection_with_handler(stream, handler).await;
                });
            }
        }
    }

    Ok(())
}

async fn handle_connection_with_handler<H, F>(stream: UnixStream, handler: H) -> io::Result<()>
where
    H: Fn(ControlCommand) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = Result<ControlResponse, ControlError>> + Send + 'static,
{
    ensure_peer_is_owner(&stream)?;
    let (read_half, mut write_half) = stream.into_split();
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    reader.read_line(&mut line).await?;

    let request: RequestFrame = from_slice(line.trim_end().as_bytes()).map_err(invalid_data)?;
    let result = handler(request.command).await;
    let response = ResponseFrame {
        request_id: request.request_id,
        response: result,
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

fn set_private_socket_permissions(path: &Path) -> io::Result<()> {
    let permissions = fs::Permissions::from_mode(0o600);
    fs::set_permissions(path, permissions)
}

fn ensure_peer_is_owner(stream: &UnixStream) -> io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = stream;
        Ok(())
    }

    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;

        let expected_uid = unsafe { libc::geteuid() };
        let mut credentials = libc::ucred {
            pid: 0,
            uid: 0,
            gid: 0,
        };
        let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
        let result = unsafe {
            libc::getsockopt(
                stream.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_PEERCRED,
                (&mut credentials as *mut libc::ucred).cast(),
                &mut len,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        if credentials.uid != expected_uid {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "rejecting control client from uid {} (expected {})",
                    credentials.uid, expected_uid
                ),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;
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

    #[tokio::test]
    async fn bound_socket_is_private() {
        let tempdir = tempdir().expect("tempdir");
        let socket_path = PathBuf::from(tempdir.path()).join("taskers.sock");
        let listener = bind_socket(&socket_path).expect("listener");

        let mode = std::fs::metadata(&socket_path)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);

        drop(listener);
    }
}
