//! The endpoint file: where the running app's MCP server listens.
//!
//! The server writes `mcp-endpoint.json` into the data directory when it
//! starts and removes it when it stops; `nail mcp --client <name>` reads it
//! to find the loopback URL (ADR 0025). The file holds no secret:
//! the bridge authenticates with the client's bearer token.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Name of the endpoint file in the data directory.
pub const ENDPOINT_FILE: &str = "mcp-endpoint.json";

/// Contents of the endpoint file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Endpoint {
    /// Streamable HTTP URL, `http://127.0.0.1:<port>/mcp`.
    pub url: String,
    /// Process id of the server (diagnostics).
    pub pid: u32,
}

/// The endpoint file of a data directory.
pub fn endpoint_path(data_dir: &Path) -> PathBuf {
    data_dir.join(ENDPOINT_FILE)
}

/// Writes the endpoint file (atomically: temporary file, then rename).
pub fn write_endpoint(path: &Path, endpoint: &Endpoint) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(endpoint)?)?;
    std::fs::rename(&temp, path)
}

/// Reads the endpoint file.
pub fn read_endpoint(path: &Path) -> std::io::Result<Endpoint> {
    let bytes = std::fs::read(path)?;
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

/// Removes the endpoint file if it still names `url` (another instance may
/// have replaced it).
pub fn remove_endpoint(path: &Path, url: &str) {
    if read_endpoint(path).is_ok_and(|e| e.url == url) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_files_round_trip_and_are_removed_by_their_owner() {
        let dir = tempfile::tempdir().unwrap();
        let path = endpoint_path(dir.path());
        let endpoint = Endpoint {
            url: "http://127.0.0.1:1234/mcp".into(),
            pid: 7,
        };
        write_endpoint(&path, &endpoint).unwrap();
        assert_eq!(read_endpoint(&path).unwrap(), endpoint);
        remove_endpoint(&path, "http://127.0.0.1:9999/mcp");
        assert!(path.exists(), "someone else's file stays");
        remove_endpoint(&path, &endpoint.url);
        assert!(!path.exists());
        assert!(read_endpoint(&path).is_err());
    }
}
