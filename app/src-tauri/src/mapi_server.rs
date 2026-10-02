//! The app's side of Simple MAPI (M4b WP113, ADR 0087): a named pipe that
//! `NailMapi.dll` (WP111) sends each `MAPISendMail(W)` call to.
//!
//! * Name: `\\.\pipe\nail-mapi-<user SID>` (`nail_api::mapi::mapi_pipe_name`;
//!   `NAIL_MAPI_PIPE` overrides it for tests).
//! * Created with `FILE_FLAG_FIRST_PIPE_INSTANCE` (another process that
//!   already owns the name — a squatter — makes the server refuse to start
//!   instead of sharing it), remote clients rejected, and a protected DACL
//!   granting only the current user access ([`crate::win::pipe_sddl`]).
//!   No network sockets.
//! * Frames: 4-byte little-endian length + JSON (`MapiPipeRequest` →
//!   `MapiPipeResponse`), at most `MAX_MAPI_FRAME_BYTES`; another protocol
//!   version or a malformed frame is answered with `MapiResult::Failure`.
//! * Each request goes to the handler (the core's `Service::submit_mapi`:
//!   files read before answering, a draft saved, the composer opened —
//!   never sent).

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use nail_api::mapi::{
    MAPI_PROTOCOL_VERSION, MapiMessage, MapiPipeRequest, MapiPipeResponse, MapiResult,
    decode_frame, encode_frame, frame_len,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Handles one message; returns what `MAPISendMail` returns.
pub type MapiHandler = Arc<dyn Fn(MapiMessage) -> BoxFuture<'static, MapiResult> + Send + Sync>;

/// How long a client may take to send its request.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How long the server waits for the client to read the answer and close
/// its end before it disconnects the instance.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

/// Waits until the client closed its end (it has read the answer), at most
/// [`DRAIN_TIMEOUT`]. `flush` does nothing on a named pipe, and
/// `DisconnectNamedPipe` discards what the client has not read yet — so
/// disconnecting right after the write lost answers the core had already
/// acted on (WP115). Anything more the client sends is ignored.
pub async fn wait_for_client_close<S: AsyncRead + Unpin>(stream: &mut S) {
    let drain = async {
        let mut buf = [0u8; 64];
        loop {
            match stream.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    };
    let _ = tokio::time::timeout(DRAIN_TIMEOUT, drain).await;
}

/// The pipe name: `NAIL_MAPI_PIPE` when set, else the user's.
pub fn pipe_name(env: Option<String>, user_sid: Option<&str>) -> Option<String> {
    env.filter(|name| name.starts_with(r"\\.\pipe\"))
        .or_else(|| user_sid.map(nail_api::mapi::mapi_pipe_name))
}

/// Reads one request from a connection, asks the handler and writes the
/// answer.
pub async fn serve_connection<S>(
    stream: &mut S,
    handler: &MapiHandler,
) -> Result<MapiResult, String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = tokio::time::timeout(READ_TIMEOUT, read_request(stream))
        .await
        .map_err(|_| "the client sent nothing in time".to_owned())
        .and_then(|r| r);
    let result = match request {
        Ok(request) if request.version == MAPI_PROTOCOL_VERSION => handler(request.message).await,
        Ok(request) => {
            tracing::warn!(
                version = request.version,
                "unknown Simple MAPI protocol version"
            );
            MapiResult::Failure
        }
        Err(err) => {
            tracing::warn!(%err, "invalid Simple MAPI request");
            MapiResult::Failure
        }
    };
    let frame = encode_frame(&MapiPipeResponse { result }).map_err(|e| e.to_string())?;
    stream.write_all(&frame).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())?;
    Ok(result)
}

async fn read_request<S: AsyncRead + Unpin>(stream: &mut S) -> Result<MapiPipeRequest, String> {
    let mut header = [0u8; 4];
    stream
        .read_exact(&mut header)
        .await
        .map_err(|e| e.to_string())?;
    let len = frame_len(header).map_err(|e| e.to_string())?;
    let mut body = vec![0u8; len];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|e| e.to_string())?;
    decode_frame(&body).map_err(|e| e.to_string())
}

/// The running pipe server; stopped when dropped.
#[derive(Debug)]
pub struct MapiServer {
    name: String,
    task: tokio::task::JoinHandle<()>,
}

impl MapiServer {
    /// The pipe's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for MapiServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(windows)]
mod imp {
    use super::{MapiHandler, MapiServer, serve_connection, wait_for_client_close};
    use std::sync::Arc;
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    use crate::win::SecurityAttributes;

    #[allow(unsafe_code)]
    fn create(
        name: &str,
        first: bool,
        security: &SecurityAttributes,
    ) -> std::io::Result<NamedPipeServer> {
        let mut options = ServerOptions::new();
        options
            .first_pipe_instance(first)
            .reject_remote_clients(true);
        // SAFETY: `security` points at a valid SECURITY_ATTRIBUTES whose
        // descriptor lives as long as `security`, which outlives the call.
        unsafe { options.create_with_security_attributes_raw(name, security.as_ptr()) }
    }

    impl MapiServer {
        /// Creates the pipe (first instance, the given SDDL) and serves it
        /// on `runtime`.
        pub fn start(
            name: &str,
            sddl: &str,
            handler: MapiHandler,
            runtime: &tokio::runtime::Handle,
        ) -> Result<Self, String> {
            let security = Arc::new(SecurityAttributes::from_sddl(sddl)?);
            let _guard = runtime.enter();
            let first = create(name, true, &security)
                .map_err(|e| format!("cannot create the Simple MAPI pipe {name}: {e}"))?;
            let pipe = name.to_owned();
            let task = runtime.spawn(async move {
                let mut server = first;
                loop {
                    if let Err(err) = server.connect().await {
                        tracing::warn!(%err, "a Simple MAPI client failed to connect");
                    }
                    let next = match create(&pipe, false, &security) {
                        Ok(next) => next,
                        Err(err) => {
                            tracing::error!(%err, "cannot create the next Simple MAPI pipe instance");
                            break;
                        }
                    };
                    let mut connected = std::mem::replace(&mut server, next);
                    let handler = Arc::clone(&handler);
                    tokio::spawn(async move {
                        match serve_connection(&mut connected, &handler).await {
                            Ok(result) => {
                                tracing::info!(?result, "answered a Simple MAPI request");
                                wait_for_client_close(&mut connected).await;
                            }
                            Err(err) => tracing::warn!(%err, "Simple MAPI connection failed"),
                        }
                        let _ = connected.disconnect();
                    });
                }
            });
            Ok(Self {
                name: name.to_owned(),
                task,
            })
        }
    }
}

#[cfg(not(windows))]
impl MapiServer {
    /// Named pipes are Windows only.
    pub fn start(
        _name: &str,
        _sddl: &str,
        _handler: MapiHandler,
        _runtime: &tokio::runtime::Handle,
    ) -> Result<Self, String> {
        Err("Simple MAPI needs Windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::mapi::{MapiAttachment, MapiRecipient, MapiRecipientClass, MapiUi, read_frame};
    use pretty_assertions::assert_eq;
    use std::sync::Mutex;

    fn message() -> MapiMessage {
        MapiMessage {
            subject: Some("Report".into()),
            body: Some("Attached.".into()),
            recipients: vec![MapiRecipient {
                class: MapiRecipientClass::To,
                name: Some("Ada".into()),
                address: Some("SMTP:ada@example.org".into()),
            }],
            attachments: vec![MapiAttachment {
                path: r"C:\Temp\report.pdf".into(),
                file_name: None,
            }],
            ui: MapiUi::None,
            program: Some("WINWORD.EXE".into()),
        }
    }

    fn recording(result: MapiResult) -> (MapiHandler, Arc<Mutex<Vec<MapiMessage>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let handler: MapiHandler = Arc::new(move |message| {
            sink.lock().unwrap().push(message);
            Box::pin(async move { result })
        });
        (handler, seen)
    }

    #[test]
    fn the_pipe_is_the_users_unless_overridden() {
        assert_eq!(
            pipe_name(None, Some("S-1-5-21-1")).as_deref(),
            Some(r"\\.\pipe\nail-mapi-S-1-5-21-1")
        );
        assert_eq!(
            pipe_name(Some(r"\\.\pipe\nail-mapi-test".into()), Some("S-1")).as_deref(),
            Some(r"\\.\pipe\nail-mapi-test")
        );
        // Only pipe names are accepted from the environment.
        assert_eq!(
            pipe_name(Some(r"C:\x".into()), Some("S-1")).as_deref(),
            Some(r"\\.\pipe\nail-mapi-S-1")
        );
        assert_eq!(pipe_name(None, None), None);
    }

    #[tokio::test]
    async fn a_request_is_handled_and_answered() {
        let (handler, seen) = recording(MapiResult::Success);
        let (mut client, mut server) = tokio::io::duplex(64 * 1024);
        let request = MapiPipeRequest {
            version: MAPI_PROTOCOL_VERSION,
            message: message(),
        };
        client
            .write_all(&encode_frame(&request).unwrap())
            .await
            .unwrap();
        let result = serve_connection(&mut server, &handler).await.unwrap();
        assert_eq!(result, MapiResult::Success);
        let mut answer = vec![0u8; 64];
        let n = client.read(&mut answer).await.unwrap();
        let response: MapiPipeResponse = read_frame(&mut &answer[..n]).unwrap();
        assert_eq!(response.result, MapiResult::Success);
        assert_eq!(*seen.lock().unwrap(), [message()]);
    }

    #[tokio::test]
    async fn other_versions_and_garbage_fail_without_reaching_the_core() {
        let (handler, seen) = recording(MapiResult::Success);
        for frame in [
            encode_frame(&MapiPipeRequest {
                version: 99,
                message: message(),
            })
            .unwrap(),
            {
                let mut f = 5u32.to_le_bytes().to_vec();
                f.extend_from_slice(b"{bad}");
                f
            },
            // Announces more than the limit.
            ((nail_api::mapi::MAX_MAPI_FRAME_BYTES + 1) as u32)
                .to_le_bytes()
                .to_vec(),
        ] {
            let (mut client, mut server) = tokio::io::duplex(64 * 1024);
            client.write_all(&frame).await.unwrap();
            let result = serve_connection(&mut server, &handler).await.unwrap();
            assert_eq!(result, MapiResult::Failure);
        }
        assert!(seen.lock().unwrap().is_empty());
    }

    #[cfg(windows)]
    fn test_pipe(tag: &str) -> String {
        format!(
            r"\\.\pipe\nail-mapi-test-{tag}-{}-{}",
            std::process::id(),
            crate::win::random_token().unwrap()
        )
    }

    /// The real pipe: the DLL's side connects, sends a frame, reads the
    /// answer; the handler (the core) saw the message.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_named_pipe_serves_the_dll_protocol() {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = test_pipe("serve");
        let sid = crate::win::current_user_sid().unwrap();
        let (handler, seen) = recording(MapiResult::AttachmentTooLarge);
        let server = MapiServer::start(
            &name,
            &crate::win::pipe_sddl(&sid),
            handler,
            &tokio::runtime::Handle::current(),
        )
        .unwrap();
        assert_eq!(server.name(), name);
        for _ in 0..2 {
            let mut client = ClientOptions::new().open(&name).unwrap();
            let request = MapiPipeRequest {
                version: MAPI_PROTOCOL_VERSION,
                message: message(),
            };
            client
                .write_all(&encode_frame(&request).unwrap())
                .await
                .unwrap();
            let mut header = [0u8; 4];
            client.read_exact(&mut header).await.unwrap();
            let mut body = vec![0u8; frame_len(header).unwrap()];
            client.read_exact(&mut body).await.unwrap();
            let response: MapiPipeResponse = decode_frame(&body).unwrap();
            assert_eq!(response.result, MapiResult::AttachmentTooLarge);
        }
        assert_eq!(seen.lock().unwrap().len(), 2, "one instance after another");
    }

    /// Regression (WP115): the server disconnected right after writing, so a
    /// client that read its answer a moment later got nothing (the DLL then
    /// reported a failure although the draft was saved).
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn a_slow_client_still_gets_its_answer() {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = test_pipe("slow");
        let sid = crate::win::current_user_sid().unwrap();
        let (handler, _) = recording(MapiResult::Success);
        let _server = MapiServer::start(
            &name,
            &crate::win::pipe_sddl(&sid),
            handler,
            &tokio::runtime::Handle::current(),
        )
        .unwrap();
        let mut client = ClientOptions::new().open(&name).unwrap();
        let request = MapiPipeRequest {
            version: MAPI_PROTOCOL_VERSION,
            message: message(),
        };
        client
            .write_all(&encode_frame(&request).unwrap())
            .await
            .unwrap();
        // The server has answered long before the client reads.
        tokio::time::sleep(Duration::from_millis(500)).await;
        let mut header = [0u8; 4];
        client.read_exact(&mut header).await.unwrap();
        let mut body = vec![0u8; frame_len(header).unwrap()];
        client.read_exact(&mut body).await.unwrap();
        let response: MapiPipeResponse = decode_frame(&body).unwrap();
        assert_eq!(response.result, MapiResult::Success);
    }

    #[tokio::test]
    async fn waiting_for_the_client_ends_when_it_closes() {
        let (client, mut server) = tokio::io::duplex(64);
        drop(client);
        tokio::time::timeout(Duration::from_secs(1), wait_for_client_close(&mut server))
            .await
            .unwrap();
    }

    /// A second server on the same name (a squatter, or a second Nail)
    /// cannot share the pipe.
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_first_instance_owns_the_name() {
        let name = test_pipe("first");
        let sddl = crate::win::pipe_sddl(&crate::win::current_user_sid().unwrap());
        let runtime = tokio::runtime::Handle::current();
        let (handler, _) = recording(MapiResult::Success);
        let _first = MapiServer::start(&name, &sddl, Arc::clone(&handler), &runtime).unwrap();
        let second = MapiServer::start(&name, &sddl, handler, &runtime);
        assert!(second.is_err());
    }

    /// The ACL: a descriptor that grants the current user nothing locks
    /// the client out (the real one grants only the current user).
    #[cfg(windows)]
    #[tokio::test(flavor = "multi_thread")]
    async fn the_acl_decides_who_may_connect() {
        use tokio::net::windows::named_pipe::ClientOptions;
        let name = test_pipe("acl");
        let (handler, _) = recording(MapiResult::Success);
        // Only LocalSystem (S-1-5-18) may connect.
        let _server = MapiServer::start(
            &name,
            "D:P(A;;GA;;;S-1-5-18)",
            handler,
            &tokio::runtime::Handle::current(),
        )
        .unwrap();
        let err = ClientOptions::new().open(&name).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{err}");
    }
}
