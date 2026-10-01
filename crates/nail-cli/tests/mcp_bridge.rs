//! `nail mcp --client <name>` end to end: the binary forwards JSON-RPC
//! lines between its stdin/stdout and an MCP server on 127.0.0.1.

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;

use nail_api::compose::Draft;
use nail_api::mcp::McpGrant;
use nail_api::{
    Account, AccountId, ApiError, DraftId, Event, Folder, FolderId, MessageBody, MessageId,
    MessageSummary, Request, Response, SearchQuery, ThreadId, UiContext, VirtualAccountConfig,
};
use nail_mcp::{
    AttachmentData, DataResult, McpData, McpServer, MessagePage, SearchOutcome, ServerOptions,
    ThreadData,
};

/// No data at all: every read is "not found".
struct Empty;

fn none<T>() -> DataResult<T> {
    Err(ApiError::not_found("nothing here"))
}

impl McpData for Empty {
    async fn accounts(&self) -> DataResult<Vec<Account>> {
        Ok(vec![])
    }
    async fn virtual_accounts(&self) -> DataResult<Vec<VirtualAccountConfig>> {
        Ok(vec![])
    }
    async fn folders(&self, _: &AccountId) -> DataResult<Vec<Folder>> {
        none()
    }
    async fn messages(&self, _: &FolderId, _: u32, _: u32) -> DataResult<MessagePage> {
        none()
    }
    async fn message_summary(&self, _: &MessageId) -> DataResult<MessageSummary> {
        none()
    }
    async fn message_body(&self, _: &MessageId) -> DataResult<MessageBody> {
        none()
    }
    async fn raw_message(&self, _: &MessageId) -> DataResult<Vec<u8>> {
        none()
    }
    async fn attachment(&self, _: &MessageId, _: u32) -> DataResult<AttachmentData> {
        none()
    }
    async fn is_encrypted(&self, _: &MessageId) -> DataResult<bool> {
        Ok(false)
    }
    async fn thread(&self, _: &ThreadId) -> DataResult<ThreadData> {
        none()
    }
    async fn search(&self, _: SearchQuery) -> DataResult<SearchOutcome> {
        none()
    }
    async fn rules_text(&self) -> DataResult<String> {
        Ok(String::new())
    }
    async fn ui_context(&self) -> DataResult<UiContext> {
        Ok(UiContext::default())
    }
    async fn identities(&self) -> DataResult<Vec<nail_api::Identity>> {
        Ok(Vec::new())
    }
    async fn draft(&self, _: &DraftId) -> DataResult<Draft> {
        none()
    }
    fn emit(&self, _: Event) {}
}

#[tokio::test(flavor = "multi_thread")]
async fn nail_mcp_bridges_stdio_to_the_server() {
    let store = nail_store::Store::open_in_memory().unwrap();
    let server = McpServer::start(store.clone(), Arc::new(Empty), ServerOptions::default())
        .await
        .unwrap();
    let Response::McpClientCreated { token, .. } = nail_mcp::admin::handle(
        &store,
        None,
        Request::CreateMcpClient {
            name: "cli".into(),
            grant: McpGrant::default(),
        },
        |_| {},
    )
    .await
    .unwrap() else {
        panic!("created expected");
    };
    let url = server.url().to_owned();

    // The child process talks blocking stdio; run it off the runtime.
    let lines = tokio::task::spawn_blocking(move || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_nail"))
            .args(["mcp", "--client", "cli", "--url", &url])
            .env("NAIL_MCP_TOKEN", &token)
            .env("NAIL_LOG", "warn")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let mut read_line = || {
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            serde_json::from_str::<serde_json::Value>(&line).unwrap()
        };
        writeln!(
            stdin,
            r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"test","version":"1"}}}}}}"#
        )
        .unwrap();
        let initialized = read_line();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#).unwrap();
        writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
        let tools = read_line();
        drop(stdin);
        let status = child.wait().unwrap();
        (initialized, tools, status.success())
    })
    .await
    .unwrap();

    let (initialized, tools, exited_cleanly) = lines;
    assert_eq!(initialized["id"], 1);
    assert_eq!(initialized["result"]["serverInfo"]["name"], "nail");
    assert_eq!(tools["id"], 2);
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 9); // 5 mail + 4 PIM (M3a)
    assert!(exited_cleanly, "closing stdin ends the bridge");
}

#[test]
fn nail_mcp_without_a_token_explains_itself() {
    let output = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args(["mcp", "--client", "x", "--url", "http://127.0.0.1:9/mcp"])
        .env_remove("NAIL_MCP_TOKEN")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty(), "stdout is reserved for JSON-RPC");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("NAIL_MCP_TOKEN"), "{stderr}");
}

/// A free loopback port (released again; the child binds it right after).
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// M2c: without a running Nail, `nail mcp` starts the core headless on the
/// fixed port of the MCP settings, holds the data directory lock while it
/// runs and releases everything when the client disconnects.
#[tokio::test(flavor = "multi_thread")]
async fn nail_mcp_starts_nail_headless_when_it_is_not_running() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let port = free_port();
    let token = {
        let store = nail_store::Store::open(data.path().join(nail_store::DB_FILE_NAME)).unwrap();
        store
            .set_setting(
                nail_store::MCP_SETTINGS_KEY,
                &nail_api::mcp::McpSettings { port: Some(port) },
            )
            .unwrap();
        let Response::McpClientCreated { token, .. } = nail_mcp::admin::handle(
            &store,
            None,
            Request::CreateMcpClient {
                name: "cli".into(),
                grant: McpGrant::default(),
            },
            |_| {},
        )
        .await
        .unwrap() else {
            panic!("created expected");
        };
        token
    };
    let data_dir = data.path().to_path_buf();
    let config_dir = config.path().to_path_buf();

    let (initialized, tools, locked, endpoint, stderr, success) =
        tokio::task::spawn_blocking(move || {
            let mut child = Command::new(env!("CARGO_BIN_EXE_nail"))
                .args(["mcp", "--client", "cli", "--data-dir"])
                .arg(&data_dir)
                .arg("--config-dir")
                .arg(&config_dir)
                .env("NAIL_MCP_TOKEN", &token)
                .env("NAIL_LOG", "warn")
                .env("NAIL_LOCALE", "en")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdin = child.stdin.take().unwrap();
            let mut stdout = BufReader::new(child.stdout.take().unwrap());
            let mut read_line = || {
                let mut line = String::new();
                stdout.read_line(&mut line).unwrap();
                serde_json::from_str::<serde_json::Value>(&line).unwrap()
            };
            writeln!(
                stdin,
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"2025-06-18","capabilities":{{}},"clientInfo":{{"name":"test","version":"1"}}}}}}"#
            )
            .unwrap();
            let initialized = read_line();
            writeln!(stdin, r#"{{"jsonrpc":"2.0","method":"notifications/initialized"}}"#)
                .unwrap();
            writeln!(stdin, r#"{{"jsonrpc":"2.0","id":2,"method":"tools/list"}}"#).unwrap();
            let tools = read_line();
            // While it runs, the data directory is taken and announced.
            let locked = nail_store::DataDirLock::acquire(&data_dir, "test")
                .err()
                .map(|e| e.to_string());
            let endpoint =
                nail_mcp::endpoint::read_endpoint(&nail_mcp::endpoint::endpoint_path(&data_dir))
                    .ok();
            drop(stdin);
            let output = child.wait_with_output().unwrap();
            (
                initialized,
                tools,
                locked,
                endpoint,
                String::from_utf8_lossy(&output.stderr).into_owned(),
                output.status.success(),
            )
        })
        .await
        .unwrap();

    assert_eq!(initialized["result"]["serverInfo"]["name"], "nail");
    assert_eq!(tools["result"]["tools"].as_array().unwrap().len(), 9); // 5 mail + 4 PIM (M3a)
    let locked = locked.expect("the headless core holds the lock");
    assert!(locked.contains("mcp"), "{locked}");
    let endpoint = endpoint.expect("the endpoint file is written");
    assert_eq!(endpoint.url, format!("http://127.0.0.1:{port}/mcp"));
    assert!(stderr.contains("without a window"), "{stderr}");
    assert!(success, "closing stdin stops the headless core: {stderr}");
    // Everything is released.
    assert!(nail_store::DataDirLock::acquire(data.path(), "test").is_ok());
    assert!(!nail_mcp::endpoint::endpoint_path(data.path()).exists());
}

#[test]
fn nail_mcp_no_start_does_not_start_nail() {
    let data = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args(["mcp", "--client", "x", "--no-start", "--data-dir"])
        .arg(data.path())
        .env("NAIL_MCP_TOKEN", "t")
        .env("NAIL_LOCALE", "en")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not running"), "{stderr}");
    assert!(
        !data.path().join(nail_store::DB_FILE_NAME).exists(),
        "no store was created"
    );
}
