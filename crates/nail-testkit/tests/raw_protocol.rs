//! Talks raw IMAP to the mock server to check wire details that the client
//! tests do not exercise (literals, SASL-IR, non-peek fetch, SELECT).
#![cfg(test)]

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use nail_testkit::{MailStore, MockImapConfig, MockImapServer};

struct Conn {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Conn {
    async fn open(server: &MockImapServer) -> Self {
        let stream = TcpStream::connect(server.addr()).await.unwrap();
        let (read, writer) = stream.into_split();
        let mut conn = Self {
            reader: BufReader::new(read),
            writer,
        };
        let greeting = conn.line().await;
        assert!(greeting.starts_with("* OK"), "{greeting}");
        conn
    }

    async fn line(&mut self) -> String {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
            .await
            .expect("server answered in time")
            .unwrap();
        line
    }

    async fn send(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).await.unwrap();
    }

    /// Sends a command and returns all lines up to and including the tagged one.
    async fn cmd(&mut self, tag: &str, command: &str) -> Vec<String> {
        self.send(&format!("{tag} {command}\r\n")).await;
        self.until_tagged(tag).await
    }

    async fn until_tagged(&mut self, tag: &str) -> Vec<String> {
        let mut lines = Vec::new();
        loop {
            let line = self.line().await;
            assert!(!line.is_empty(), "connection closed");
            let done = line.starts_with(&format!("{tag} "));
            lines.push(line);
            if done {
                return lines;
            }
        }
    }
}

async fn server() -> MockImapServer {
    MockImapServer::start(MockImapConfig::default(), MailStore::demo())
        .await
        .unwrap()
}

#[tokio::test]
async fn login_with_synchronizing_literal() {
    let server = server().await;
    let mut conn = Conn::open(&server).await;
    conn.send("a1 LOGIN {16}\r\n").await;
    assert!(conn.line().await.starts_with("+ "));
    conn.send("sam@nail.example \"secret\"\r\n").await;
    let lines = conn.until_tagged("a1").await;
    assert!(lines.last().unwrap().starts_with("a1 OK"), "{lines:?}");
}

#[tokio::test]
async fn authenticate_plain_with_initial_response_and_errors() {
    let server = server().await;
    let mut conn = Conn::open(&server).await;
    // base64("\0sam@nail.example\0wrong")
    let lines = conn
        .cmd("a1", "AUTHENTICATE PLAIN AHNhbUBuYWlsLmV4YW1wbGUAd3Jvbmc=")
        .await;
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with("a1 NO [AUTHENTICATIONFAILED]"),
        "{lines:?}"
    );
    let lines = conn.cmd("a2", "SELECT INBOX").await;
    assert!(
        lines.last().unwrap().starts_with("a2 BAD"),
        "not authenticated: {lines:?}"
    );
    // base64("\0sam@nail.example\0secret")
    let lines = conn
        .cmd("a3", "AUTHENTICATE PLAIN AHNhbUBuYWlsLmV4YW1wbGUAc2VjcmV0")
        .await;
    assert!(lines.last().unwrap().starts_with("a3 OK"), "{lines:?}");
    let lines = conn.cmd("a4", "FROBNICATE").await;
    assert!(lines.last().unwrap().starts_with("a4 BAD"));
}

#[tokio::test]
async fn select_fetch_body_marks_seen_but_examine_does_not() {
    let server = server().await;
    let mut conn = Conn::open(&server).await;
    conn.cmd("a1", "LOGIN sam@nail.example secret").await;

    let lines = conn.cmd("a2", "EXAMINE INBOX").await;
    assert!(lines.iter().any(|l| l.starts_with("* OK [UIDVALIDITY ")));
    assert!(lines.last().unwrap().starts_with("a2 OK [READ-ONLY]"));
    // UID 2 (the newsletter) is unseen in the corpus.
    let lines = conn.cmd("a3", "UID FETCH 2 (FLAGS RFC822.SIZE)").await;
    assert!(
        lines[0].starts_with("* 2 FETCH (UID 2 FLAGS () RFC822.SIZE "),
        "{lines:?}"
    );
    let lines = conn.cmd("a4", "UID FETCH 2 BODY[HEADER]").await;
    assert!(lines[0].contains("BODY[HEADER] {"), "{lines:?}");
    assert!(
        !lines.iter().any(|l| l.contains("\\Seen")),
        "EXAMINE is read-only"
    );

    let lines = conn.cmd("a5", "SELECT INBOX").await;
    assert!(lines.last().unwrap().starts_with("a5 OK [READ-WRITE]"));
    let lines = conn.cmd("a6", "FETCH 2 BODY[TEXT]").await;
    assert!(
        lines.iter().any(|l| l.contains("FLAGS (\\Seen)")),
        "{lines:?}"
    );
    let seen = server.store().mailbox("INBOX").unwrap().messages[1]
        .flags
        .clone();
    assert_eq!(seen, ["\\Seen"]);

    let lines = conn.cmd("a7", "UID SEARCH UNSEEN").await;
    assert!(lines[0].starts_with("* SEARCH"));
    assert!(!lines[0].split_whitespace().any(|t| t == "2"));
    let lines = conn.cmd("a8", "LOGOUT").await;
    assert!(lines[0].starts_with("* BYE"));
}

#[tokio::test]
async fn list_patterns_and_special_use() {
    let server = server().await;
    let mut conn = Conn::open(&server).await;
    conn.cmd("a1", "LOGIN sam@nail.example secret").await;
    let lines = conn.cmd("a2", "LIST \"\" \"\"").await;
    assert_eq!(lines[0], "* LIST (\\Noselect) \"/\" \"\"\r\n");
    let lines = conn.cmd("a3", "LIST \"\" \"%\"").await;
    let has = |line: &str| lines.iter().any(|l| l == line);
    assert!(
        has("* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent\"\r\n"),
        "{lines:?}"
    );
    assert!(has(
        "* LIST (\\HasNoChildren \\Archive) \"/\" \"Archive\"\r\n"
    ));
    assert!(has("* LIST (\\HasNoChildren) \"/\" \"INBOX\"\r\n"));
    let lines = conn.cmd("a4", "LIST \"\" \"S*\"").await;
    assert_eq!(lines.len(), 2);
    assert_eq!(server.connection_count(), 1);
}

#[tokio::test]
async fn injected_failures() {
    let server = server().await;
    let mut conn = Conn::open(&server).await;
    conn.cmd("a1", "LOGIN sam@nail.example secret").await;
    server.fail_on(Some("noop"));
    let lines = conn.cmd("a2", "NOOP").await;
    assert!(lines[0].starts_with("a2 NO [SERVERBUG]"));
    server.fail_on(None);
    server.disconnect_on(Some("NOOP"));
    conn.send("a3 NOOP\r\n").await;
    assert_eq!(conn.line().await, "", "connection closed without answer");
}
