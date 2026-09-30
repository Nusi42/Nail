//! The `nail` binary end to end: `serve --seed demo` answers HTTP, and
//! `config schema` prints JSON.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn http_get(addr: &str, path: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

#[test]
fn serve_prints_its_url_and_serves_the_ui() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ui = tempfile::tempdir().unwrap();
    std::fs::write(ui.path().join("index.html"), "<!doctype html>nail-test-ui").unwrap();

    let child = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args(["serve", "--port", "0", "--seed", "demo", "--no-watch"])
        .arg("--data-dir")
        .arg(data.path())
        .arg("--config-dir")
        .arg(config.path())
        .arg("--ui-dir")
        .arg(ui.path())
        .env("NAIL_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut child = Killed(child);

    let mut line = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let url = line.trim();
    let addr = url
        .strip_prefix("http://")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap_or_else(|| panic!("unexpected first line {url:?}"));

    let index = http_get(addr, "/");
    assert!(index.starts_with("HTTP/1.1 200"), "{index}");
    assert!(index.contains("nail-test-ui"));
    let spa = http_get(addr, "/some/route");
    assert!(spa.contains("nail-test-ui"));
    let blob = http_get(addr, "/api/blob/not-a-hash");
    assert!(blob.starts_with("HTTP/1.1 400"), "{blob}");

    // The demo seed landed in the data directory.
    assert!(data.path().join("nail.sqlite3").is_file());
    assert!(data.path().join("blobs").is_dir());
}

#[test]
fn config_schema_prints_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args(["config", "schema", "layout"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["title"], "Nail layout.kdl");
}

#[test]
fn bad_arguments_fail() {
    let output = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args(["serve", "--seed", "nothing"])
        .output()
        .unwrap();
    assert!(!output.status.success());
}

/// Reads lines until each of `needles` was in one (any order, 30 s).
fn wait_lines(lines: &std::sync::mpsc::Receiver<String>, needles: &[&str]) {
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let mut missing: Vec<&str> = needles.to_vec();
    let mut seen = Vec::new();
    while !missing.is_empty() {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        match lines.recv_timeout(left) {
            Ok(line) => {
                missing.retain(|n| !line.contains(n));
                seen.push(line);
            }
            Err(_) => panic!("no lines with {missing:?}; saw {seen:#?}"),
        }
    }
}

#[test]
fn plugin_dev_loads_a_folder_into_serve_and_follows_reloads() {
    let data = tempfile::tempdir().unwrap();
    let config = tempfile::tempdir().unwrap();
    let ui = tempfile::tempdir().unwrap();
    std::fs::write(ui.path().join("index.html"), "<!doctype html>ui").unwrap();
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(
        folder.path().join("manifest.json"),
        r#"{"id":"console-demo","name":"Console demo","version":"1.0.0","apiVersion":"1","main":"index.js","permissions":["storage"]}"#,
    )
    .unwrap();
    let js =
        |text: &str| format!("export default {{ activate() {{ nail.log.info({text:?}); }} }};");
    std::fs::write(folder.path().join("index.js"), js("hello one")).unwrap();

    let serve = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args([
            "serve",
            "--port",
            "0",
            "--seed",
            "demo",
            "--no-watch",
            "--no-mcp",
        ])
        .arg("--data-dir")
        .arg(data.path())
        .arg("--config-dir")
        .arg(config.path())
        .arg("--ui-dir")
        .arg(ui.path())
        .env("NAIL_LOG", "warn")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut serve = Killed(serve);
    let mut line = String::new();
    BufReader::new(serve.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    assert!(line.starts_with("http://127.0.0.1:"), "{line}");
    // `serve` announced itself in its data directory.
    assert!(data.path().join("ui-endpoint.json").is_file());

    // Found through --data-dir (the endpoint file).
    let console = Command::new(env!("CARGO_BIN_EXE_nail"))
        .args(["plugin", "dev"])
        .arg(folder.path())
        .arg("--data-dir")
        .arg(data.path())
        .env("NAIL_LOG", "warn")
        .env("NAIL_LOCALE", "en")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let mut console = Killed(console);
    let stdout = console.0.stdout.take().unwrap();
    let (tx, lines) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    wait_lines(&lines, &["hello one", "Loaded Console demo 1.0.0"]);
    // Hot reload: the new code runs and says so in the console.
    std::fs::write(folder.path().join("index.js"), js("hello two")).unwrap();
    wait_lines(&lines, &["hello two"]);
}
