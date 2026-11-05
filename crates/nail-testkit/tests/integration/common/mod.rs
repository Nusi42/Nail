//! A raw IMAP connection to the mock server, shared by the raw protocol
//! tests.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use nail_testkit::MockImapServer;

pub(crate) struct Conn {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Conn {
    pub(crate) async fn open(server: &MockImapServer) -> Self {
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

    pub(crate) async fn login(server: &MockImapServer) -> Self {
        let mut conn = Self::open(server).await;
        let lines = conn.cmd("l1", "LOGIN sam@nail.example secret").await;
        assert!(lines.last().unwrap().starts_with("l1 OK"), "{lines:?}");
        conn
    }

    pub(crate) async fn line(&mut self) -> String {
        let mut line = String::new();
        tokio::time::timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
            .await
            .expect("server answered in time")
            .unwrap();
        line
    }

    pub(crate) async fn send(&mut self, text: &str) {
        self.writer.write_all(text.as_bytes()).await.unwrap();
    }

    pub(crate) async fn cmd(&mut self, tag: &str, command: &str) -> Vec<String> {
        self.send(&format!("{tag} {command}\r\n")).await;
        self.until_tagged(tag).await
    }

    pub(crate) async fn until_tagged(&mut self, tag: &str) -> Vec<String> {
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
