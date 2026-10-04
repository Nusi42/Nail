//! The mock IMAP server the benchmark syncs from.
//!
//! [`MockMailServer`] serves a [`Corpus`] from `nail-testkit`'s
//! [`MockImapServer`], either **in this process** (tests, quick runs) or
//! in a **child process** (`nail-bench mock-imap`), so the memory the
//! server needs for the corpus does not count against the core's peak
//! working set. The child prints one JSON line ([`MockReady`]) and then
//! reads commands from stdin: `deliver <n>` appends `n` new Inbox messages
//! ([`Corpus::arrivals`]) and answers `delivered <n>`; `quit` (or a closed
//! stdin) stops it.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use nail_testkit::{MailStore, MockImapConfig, MockImapServer};
use serde::{Deserialize, Serialize};

use crate::corpus::Corpus;
use crate::{BenchError, CorpusSpec};

/// What the child prints once the server listens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MockReady {
    /// The port on 127.0.0.1.
    pub port: u16,
    /// The accepted user name.
    pub username: String,
    /// The accepted password.
    pub password: String,
    /// Messages served.
    pub messages: u64,
    /// Raw bytes served.
    pub bytes: u64,
}

/// Fills a [`MailStore`] with a corpus; answers it and the bytes stored.
pub fn mail_store(corpus: &Corpus) -> (MailStore, u64) {
    let store = MailStore::new();
    for folder in corpus.folders() {
        store.add_mailbox(&folder.name, folder.role.api());
    }
    let mut bytes = 0u64;
    for m in corpus.messages() {
        bytes += m.raw.len() as u64;
        store.append_with_date(
            &corpus.folders()[m.folder].name,
            &m.raw,
            &m.flags,
            Some(&m.internal_date()),
        );
    }
    (store, bytes)
}

/// Appends arrivals `first..first + count` to the Inbox.
pub fn deliver(store: &MailStore, corpus: &Corpus, first: u64, count: u64) {
    let inbox = &corpus.folders()[0].name;
    for m in corpus.arrivals(first, count) {
        store.append_with_date(inbox, &m.raw, &m.flags, Some(&m.internal_date()));
    }
}

/// A running mock server, in this process or a child.
#[derive(Debug)]
pub enum MockMailServer {
    /// In this process.
    InProcess {
        /// The server.
        server: MockImapServer,
        /// The corpus (for arrivals).
        corpus: Box<Corpus>,
        /// What it serves.
        ready: MockReady,
        /// Arrivals delivered so far.
        delivered: u64,
    },
    /// A `nail-bench mock-imap` child.
    Child {
        /// The process.
        child: Child,
        /// Its stdin (commands).
        stdin: ChildStdin,
        /// Its stdout (answers).
        stdout: BufReader<ChildStdout>,
        /// What it serves.
        ready: MockReady,
    },
}

impl MockMailServer {
    /// Starts a server in this process.
    pub async fn start_in_process(spec: &CorpusSpec) -> Result<Self, BenchError> {
        let corpus = Corpus::new(spec.clone());
        let (store, bytes) = mail_store(&corpus);
        let config = MockImapConfig::default();
        let server = MockImapServer::start(config.clone(), store)
            .await
            .map_err(|e| BenchError::Mock(e.to_string()))?;
        let ready = MockReady {
            port: server.port(),
            username: config.username,
            password: config.password,
            messages: spec.messages,
            bytes,
        };
        Ok(Self::InProcess {
            server,
            corpus: Box::new(corpus),
            ready,
            delivered: 0,
        })
    }

    /// Starts `exe mock-imap --spec <json>` and waits for its ready line.
    pub fn start_child(exe: &Path, spec: &CorpusSpec) -> Result<Self, BenchError> {
        let spec_json = serde_json::to_string(spec).map_err(|e| BenchError::Mock(e.to_string()))?;
        let mut child = Command::new(exe)
            .args(["mock-imap", "--spec", &spec_json])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| BenchError::Mock(format!("{}: {e}", exe.display())))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| BenchError::Mock("no stdin".into()))?;
        let mut stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| BenchError::Mock("no stdout".into()))?,
        );
        let mut line = String::new();
        stdout
            .read_line(&mut line)
            .map_err(|e| BenchError::Mock(e.to_string()))?;
        let ready: MockReady = serde_json::from_str(line.trim())
            .map_err(|e| BenchError::Mock(format!("mock-imap said {line:?}: {e}")))?;
        Ok(Self::Child {
            child,
            stdin,
            stdout,
            ready,
        })
    }

    /// What the server serves.
    pub fn ready(&self) -> &MockReady {
        match self {
            Self::InProcess { ready, .. } | Self::Child { ready, .. } => ready,
        }
    }

    /// Delivers `count` new messages to the Inbox.
    pub fn deliver(&mut self, count: u64) -> Result<(), BenchError> {
        match self {
            Self::InProcess {
                server,
                corpus,
                delivered,
                ..
            } => {
                deliver(server.store(), corpus, *delivered, count);
                *delivered += count;
                Ok(())
            }
            Self::Child { stdin, stdout, .. } => {
                writeln!(stdin, "deliver {count}").map_err(|e| BenchError::Mock(e.to_string()))?;
                stdin.flush().map_err(|e| BenchError::Mock(e.to_string()))?;
                let mut line = String::new();
                stdout
                    .read_line(&mut line)
                    .map_err(|e| BenchError::Mock(e.to_string()))?;
                if line.trim() == format!("delivered {count}") {
                    Ok(())
                } else {
                    Err(BenchError::Mock(format!("deliver answered {line:?}")))
                }
            }
        }
    }

    /// Stops the server.
    pub async fn stop(self) {
        match self {
            Self::InProcess { server, .. } => server.shutdown().await,
            Self::Child {
                mut child,
                mut stdin,
                ..
            } => {
                drop(writeln!(stdin, "quit"));
                drop(stdin);
                if child.wait().is_err() {
                    drop(child.kill());
                }
            }
        }
    }
}

/// The child side: serves `spec` and answers commands until `quit`.
pub async fn serve_child(spec: CorpusSpec) -> Result<(), BenchError> {
    let issues = spec.issues();
    if !issues.is_empty() {
        return Err(BenchError::Spec(issues.join("; ")));
    }
    let server = MockMailServer::start_in_process(&spec).await?;
    let ready = server.ready().clone();
    println!(
        "{}",
        serde_json::to_string(&ready).map_err(|e| BenchError::Mock(e.to_string()))?
    );
    let mut server = server;
    // Commands are answered one by one as they come: a thread reads stdin
    // and hands the lines over a channel.
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        for line in stdin.lock().lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    while let Some(line) = rx.recv().await {
        let mut words = line.split_whitespace();
        match (
            words.next(),
            words.next().and_then(|n| n.parse::<u64>().ok()),
        ) {
            (Some("deliver"), Some(n)) => {
                server.deliver(n)?;
                println!("delivered {n}");
                std::io::stdout()
                    .flush()
                    .map_err(|e| BenchError::Mock(e.to_string()))?;
            }
            (Some("quit"), _) => break,
            _ => {
                println!("unknown {line}");
            }
        }
    }
    server.stop().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_in_process_server_serves_the_corpus_and_arrivals() {
        let spec = CorpusSpec {
            messages: 120,
            folders: 6,
            ..CorpusSpec::small()
        };
        let mut server = MockMailServer::start_in_process(&spec).await.unwrap();
        assert_eq!(server.ready().messages, 120);
        assert!(server.ready().bytes > 0);
        let MockMailServer::InProcess { server: imap, .. } = &server else {
            panic!("in process");
        };
        let total: usize = imap
            .store()
            .mailboxes()
            .iter()
            .map(|m| m.messages.len())
            .sum();
        assert_eq!(total, 120);
        let inbox_before = imap.store().mailbox("INBOX").unwrap().messages.len();
        server.deliver(7).unwrap();
        server.deliver(3).unwrap();
        let MockMailServer::InProcess { server: imap, .. } = &server else {
            panic!("in process");
        };
        assert_eq!(
            imap.store().mailbox("INBOX").unwrap().messages.len(),
            inbox_before + 10
        );
        server.stop().await;
    }
}
