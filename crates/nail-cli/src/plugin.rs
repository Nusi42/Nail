//! `nail plugin …` (M2d, ADR 0050; wired in WP64, ADR 0055):
//! `nail plugin dev <folder>` loads an unpacked plug-in into a running
//! Nail with hot reload and prints its log (its own lines, the host's
//! lines, every API call with its outcome) as a console.
//!
//! It talks to the running `nail serve` over its local WebSocket
//! (`/api/ws` on 127.0.0.1; the URL comes from `--url` or from
//! `<data-dir>/ui-endpoint.json`, which `nail serve` writes — and, M4b, the
//! desktop app when its "nail plugin dev" endpoint is on, with a token the
//! command sends as `Authorization: Bearer`; ADR 0087). The plug-in
//! is loaded with `Request::LoadDevPlugin`, every permission its manifest
//! declares granted for all accounts; then the command tails
//! `Event::PluginLogLine` for that plug-in until Ctrl+C. The plug-in stays
//! loaded (and reloading on changes) in Nail after the console closes. In
//! the desktop app, the plug-in manager's "Load a development folder…"
//! does the same.

use std::io::Write;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use futures_util::{SinkExt, StreamExt};
use nail_api::plugins::admin::PluginLogEntry;
use nail_api::plugins::{PluginGrant, PluginManifest};
use nail_api::{Event, LocalizedMessage, PluginId, Request, Response, RpcRequest, ServerMessage};
use nail_config::l10n::Catalog;
use tokio_tungstenite::tungstenite::Message;

use crate::messages::{self, Localized};

/// The file `nail serve` writes into its data directory with its URL.
pub use nail_api::desktop::UI_ENDPOINT_FILE;
use nail_api::desktop::UiEndpointFile;

/// `nail plugin …`
#[derive(Debug, Subcommand)]
pub enum PluginCommand {
    /// Load an unpacked plug-in folder (with manifest.json) into the
    /// running Nail, reload it when its files change and show its log.
    Dev(PluginDevArgs),
}

/// `nail plugin dev …`
#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct PluginDevArgs {
    /// The plug-in folder (holding manifest.json).
    #[arg(value_name = "FOLDER")]
    pub folder: PathBuf,
    /// The running Nail's URL (as `nail serve` prints it). Default: read
    /// from the data directory (ui-endpoint.json).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// Data directory of the running Nail. Default: %LOCALAPPDATA%\Nail.
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
}

/// Runs a `nail plugin` command.
pub async fn run(command: PluginCommand) -> anyhow::Result<()> {
    match command {
        PluginCommand::Dev(args) => dev(args).await,
    }
}

/// Writes `<data-dir>/ui-endpoint.json` for `nail plugin dev`; removed
/// when dropped.
#[derive(Debug)]
pub struct UiEndpoint(PathBuf);

impl UiEndpoint {
    /// Writes the file.
    pub fn write(data_dir: &Path, url: &str) -> std::io::Result<Self> {
        let path = data_dir.join(UI_ENDPOINT_FILE);
        let body = UiEndpointFile {
            url: url.to_owned(),
            pid: std::process::id(),
            token: None,
        };
        std::fs::write(&path, serde_json::to_string(&body)?)?;
        Ok(Self(path))
    }
}

impl Drop for UiEndpoint {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// Where to connect: the URL and, for the desktop app's endpoint, its
/// token.
fn resolve_url(args: &PluginDevArgs) -> anyhow::Result<(String, Option<String>)> {
    if let Some(url) = &args.url {
        return Ok((url.clone(), None));
    }
    let data_dir = args
        .data_dir
        .clone()
        .or_else(nail_core::paths::default_data_dir)
        .localized(|| LocalizedMessage::new("cli-plugin-dev-no-nail"))?;
    let text = std::fs::read_to_string(data_dir.join(UI_ENDPOINT_FILE))
        .localized(|| LocalizedMessage::new("cli-plugin-dev-no-nail"))?;
    let file: UiEndpointFile = serde_json::from_str(&text)
        .localized(|| LocalizedMessage::new("cli-plugin-dev-no-nail"))?;
    Ok((file.url, file.token))
}

/// `ws://host:port/api/ws` of a UI URL.
pub fn socket_url(url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url).ok()?;
    let scheme = match parsed.scheme() {
        "http" => "ws",
        "https" => "wss",
        _ => return None,
    };
    parsed.set_scheme(scheme).ok()?;
    parsed.set_path("/api/ws");
    parsed.set_query(None);
    parsed.set_fragment(None);
    Some(parsed.to_string())
}

/// Opens the WebSocket of the Nail at `url`. Failures carry Nail's own
/// text; only the network library's reason is an English `detail`.
async fn connect(
    url: &str,
    token: Option<&str>,
) -> anyhow::Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
> {
    let socket = socket_url(url)
        .localized(|| LocalizedMessage::new("cli-plugin-dev-not-http").arg("url", url))?;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut request = socket.as_str().into_client_request()?;
    if let Some(token) = token {
        request
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse()?);
    }
    match tokio_tungstenite::connect_async(request).await {
        Ok((ws, _)) => Ok(ws),
        Err(err) => Err(messages::error(
            LocalizedMessage::new("cli-plugin-dev-connect")
                .arg("url", url)
                .arg("detail", err.to_string()),
        )),
    }
}

/// Every permission the manifest declares, for all accounts.
pub fn declared_grants(manifest: &PluginManifest) -> Vec<PluginGrant> {
    manifest
        .permissions
        .iter()
        .cloned()
        .map(PluginGrant::all)
        .collect()
}

/// One console line of a log entry.
pub fn console_line(catalog: &Catalog, entry: &PluginLogEntry) -> String {
    let text = entry
        .localized
        .as_deref()
        .map_or_else(|| entry.message.clone(), |m| catalog.render(m));
    let level = serde_json::to_value(entry.level)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    let source = serde_json::to_value(entry.source)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default();
    catalog.render(
        &LocalizedMessage::new("cli-plugin-dev-line")
            .arg("level", level)
            .arg("source", source)
            .arg("text", text),
    )
}

async fn dev(args: PluginDevArgs) -> anyhow::Result<()> {
    let catalog = messages::catalog();
    let folder = std::path::absolute(&args.folder).localized(|| {
        LocalizedMessage::new("cli-plugin-dev-folder")
            .arg("folder", args.folder.display().to_string())
    })?;
    let folder = folder.canonicalize().unwrap_or(folder);
    // Grants: whatever the manifest declares. An unreadable or invalid
    // manifest goes to Nail anyway, which reports its problems.
    let manifest = std::fs::read(folder.join("manifest.json"))
        .ok()
        .and_then(|bytes| PluginManifest::parse(&bytes).ok());
    let grants = manifest.as_ref().map(declared_grants).unwrap_or_default();
    let (url, token) = resolve_url(&args)?;
    let mut ws = connect(&url, token.as_deref()).await?;
    let request = RpcRequest {
        id: 1,
        request: Request::LoadDevPlugin {
            folder: folder.display().to_string(),
            grants,
        },
    };
    ws.send(Message::Text(serde_json::to_string(&request)?.into()))
        .await?;

    // Lines of the first start arrive before the answer.
    let mut plugin: Option<PluginId> = manifest.as_ref().map(|m| m.id.clone());
    let mut out = std::io::stdout();
    loop {
        let frame = tokio::select! {
            frame = ws.next() => frame,
            _ = tokio::signal::ctrl_c() => break,
        };
        let text = match frame {
            Some(Ok(Message::Text(text))) => text,
            Some(Ok(Message::Close(_))) | None => {
                return Err(messages::error(LocalizedMessage::new(
                    "cli-plugin-dev-closed",
                )));
            }
            Some(Ok(_)) => continue,
            Some(Err(err)) => {
                return Err(messages::error(
                    LocalizedMessage::new("cli-plugin-dev-connect")
                        .arg("url", url.as_str())
                        .arg("detail", err.to_string()),
                ));
            }
        };
        let Ok(message) = serde_json::from_str::<ServerMessage>(text.as_str()) else {
            continue;
        };
        match message {
            ServerMessage::Response { id: 1, response } => match response {
                Response::Plugin { plugin: info } => {
                    let line = catalog.render(
                        &LocalizedMessage::new("cli-plugin-dev-loaded")
                            .arg("plugin", info.manifest.name.as_str())
                            .arg("version", info.manifest.version.as_str())
                            .arg("folder", folder.display().to_string()),
                    );
                    writeln!(out, "{line}")?;
                    if let Some(error) = info.localized.as_deref() {
                        writeln!(out, "{}", catalog.render(error))?;
                    }
                    out.flush()?;
                    plugin = Some(info.id);
                }
                Response::Error {
                    message, localized, ..
                } => {
                    let detail = localized.as_deref().map_or(message, |m| catalog.render(m));
                    return Err(messages::error(
                        LocalizedMessage::new("cli-plugin-dev-refused").arg("detail", detail),
                    ));
                }
                _ => {}
            },
            ServerMessage::Event {
                event: Event::PluginLogLine { entry },
            } if plugin.as_ref() == Some(&entry.plugin_id) => {
                writeln!(out, "{}", console_line(&catalog, &entry))?;
                out.flush()?;
            }
            _ => {}
        }
    }
    let _ = ws.close(None).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use nail_api::plugins::PluginPermission;
    use nail_api::plugins::admin::{PluginLogLevel, PluginLogSource};

    #[test]
    fn dev_takes_a_folder_and_where_nail_runs() {
        let cli = crate::cli::Cli::try_parse_from([
            "nail",
            "plugin",
            "dev",
            "C:/dev/follow-up",
            "--url",
            "http://127.0.0.1:4780/",
        ])
        .unwrap();
        match cli.command {
            crate::cli::Command::Plugin {
                command: PluginCommand::Dev(args),
            } => {
                assert_eq!(args.folder, PathBuf::from("C:/dev/follow-up"));
                assert_eq!(args.url.as_deref(), Some("http://127.0.0.1:4780/"));
            }
            other => panic!("{other:?}"),
        }
        assert!(crate::cli::Cli::try_parse_from(["nail", "plugin", "dev"]).is_err());
    }

    #[test]
    fn socket_urls() {
        assert_eq!(
            socket_url("http://127.0.0.1:4780/").as_deref(),
            Some("ws://127.0.0.1:4780/api/ws")
        );
        assert_eq!(
            socket_url("https://nail.example:8443/x?y").as_deref(),
            Some("wss://nail.example:8443/api/ws")
        );
        assert_eq!(socket_url("ftp://x"), None);
    }

    /// M2d review: no English written by Nail as `detail`, and the real
    /// reason instead of "no answer".
    #[tokio::test]
    async fn connect_failures_are_localized_with_the_library_reason() {
        let catalog = Catalog::english();
        let err = connect("ftp://127.0.0.1/", None).await.unwrap_err();
        let text = messages::render_error(&err, &catalog);
        assert_eq!(
            text.lines().next().unwrap(),
            catalog.render(
                &LocalizedMessage::new("cli-plugin-dev-not-http").arg("url", "ftp://127.0.0.1/")
            ),
            "{text}"
        );
        // A port nothing listens on.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}/");
        let err = connect(&url, None).await.unwrap_err();
        let cli = err
            .downcast_ref::<messages::CliError>()
            .expect("a CliError");
        assert_eq!(cli.message.id, "cli-plugin-dev-connect");
        let detail = cli.message.args["detail"].to_string();
        assert_ne!(detail, "no answer");
        assert!(!detail.is_empty());
    }

    #[test]
    fn the_endpoint_file_is_written_read_and_removed() {
        let dir = tempfile::tempdir().unwrap();
        let args = PluginDevArgs {
            folder: "x".into(),
            url: None,
            data_dir: Some(dir.path().to_path_buf()),
        };
        let err = resolve_url(&args).unwrap_err();
        let text = messages::render_error(&err, &Catalog::english());
        assert!(text.contains("nail serve"), "{text}");
        {
            let _endpoint = UiEndpoint::write(dir.path(), "http://127.0.0.1:5000/").unwrap();
            assert_eq!(
                resolve_url(&args).unwrap(),
                ("http://127.0.0.1:5000/".to_owned(), None)
            );
        }
        assert!(!dir.path().join(UI_ENDPOINT_FILE).exists());
        // M4b: the desktop app's endpoint file carries a token.
        std::fs::write(
            dir.path().join(UI_ENDPOINT_FILE),
            r#"{"url":"http://127.0.0.1:5001/","pid":1,"token":"s3cret"}"#,
        )
        .unwrap();
        assert_eq!(
            resolve_url(&args).unwrap(),
            (
                "http://127.0.0.1:5001/".to_owned(),
                Some("s3cret".to_owned())
            )
        );
    }

    /// M4b: the token goes out as a bearer header on the upgrade.
    #[tokio::test]
    async fn the_desktop_token_is_sent_on_the_upgrade() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = vec![0u8; 4096];
            let n = socket.read(&mut buffer).await.unwrap();
            let _ = socket.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
            String::from_utf8_lossy(&buffer[..n]).into_owned()
        });
        assert!(connect(&url, Some("s3cret")).await.is_err());
        let head = server.await.unwrap();
        assert!(
            head.to_ascii_lowercase()
                .contains("authorization: bearer s3cret"),
            "{head}"
        );
    }

    #[test]
    fn grants_cover_every_declared_permission() {
        let manifest = PluginManifest::parse(
            br#"{"id":"p","name":"P","version":"1.0.0","apiVersion":"1","main":"i.js",
                "permissions":["mail.read","storage","network:api.example.com"]}"#,
        )
        .unwrap();
        let grants = declared_grants(&manifest);
        assert_eq!(grants.len(), 3);
        assert!(grants.iter().all(|g| g.accounts.is_none()));
        assert_eq!(grants[0].permission, PluginPermission::MailRead);
    }

    #[test]
    fn console_lines_use_the_localized_text() {
        let entry = |localized: Option<LocalizedMessage>| PluginLogEntry {
            seq: 1,
            plugin_id: PluginId::new("p"),
            at: "2026-09-30T08:00:00.000Z".into(),
            level: PluginLogLevel::Warn,
            source: PluginLogSource::Api,
            message: "english".into(),
            localized: localized.map(Box::new),
        };
        let catalog = Catalog::english();
        let plain = console_line(&catalog, &entry(None));
        assert!(plain.contains("warn") && plain.contains("api") && plain.contains("english"));
        let localized = console_line(
            &catalog,
            &entry(Some(
                LocalizedMessage::new("plugin-stopped-cpu-budget").arg("plugin", "P"),
            )),
        );
        assert!(localized.contains("P was stopped"), "{localized}");
    }

    #[tokio::test]
    async fn an_unreachable_nail_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let err = run(PluginCommand::Dev(PluginDevArgs {
            folder: dir.path().to_path_buf(),
            url: Some("http://127.0.0.1:9/".into()),
            data_dir: None,
        }))
        .await
        .unwrap_err();
        let text = messages::render_error(&err, &Catalog::english());
        assert!(text.contains("127.0.0.1:9"), "{text}");
    }
}
