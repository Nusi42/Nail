//! Raw IMAP against the mock server's M1 extensions: IDLE push between
//! sessions, CONDSTORE / QRESYNC, STORE, MOVE, COPY, EXPUNGE, APPEND,
//! folder commands, XOAUTH2 / OAUTHBEARER, ID and the test helpers.

use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nail_api::FolderRole;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

use nail_testkit::{MailStore, MockCapabilities, MockImapConfig, MockImapServer};

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

    async fn login(server: &MockImapServer) -> Self {
        let mut conn = Self::open(server).await;
        let lines = conn.cmd("l1", "LOGIN sam@nail.example secret").await;
        assert!(lines.last().unwrap().starts_with("l1 OK"), "{lines:?}");
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

fn store() -> MailStore {
    let store = MailStore::new();
    store.add_mailbox("Archive", FolderRole::Archive);
    for subject in ["a", "b", "c"] {
        store.append(
            "INBOX",
            format!("Subject: {subject}\r\n\r\nbody").as_bytes(),
            &[] as &[&str],
        );
    }
    store
}

async fn server_with(config: MockImapConfig) -> MockImapServer {
    MockImapServer::start(config, store()).await.unwrap()
}

async fn server() -> MockImapServer {
    server_with(MockImapConfig::default()).await
}

fn has(lines: &[String], expected: &str) -> bool {
    lines.iter().any(|l| l.trim_end() == expected)
}

#[tokio::test]
async fn capabilities_follow_the_configuration() {
    let server = server_with(MockImapConfig {
        oauth_token: Some("t".to_owned()),
        capabilities: MockCapabilities {
            append_limit: Some(1000),
            ..MockCapabilities::default()
        },
        ..MockImapConfig::default()
    })
    .await;
    let mut conn = Conn::open(&server).await;
    let caps = conn.cmd("a1", "CAPABILITY").await[0].clone();
    for cap in [
        "IDLE",
        "CONDSTORE",
        "QRESYNC",
        "ENABLE",
        "MOVE",
        "UIDPLUS",
        "ID",
        "SASL-IR",
        "LITERAL+",
        "APPENDLIMIT=1000",
        "AUTH=XOAUTH2",
        "AUTH=OAUTHBEARER",
    ] {
        assert!(caps.split_whitespace().any(|c| c == cap), "{cap}: {caps}");
    }
    // COMPRESS is offered after login only.
    assert!(!caps.contains("COMPRESS"));
    let lines = conn.cmd("a2", "LOGIN sam@nail.example secret").await;
    assert!(lines[0].contains("[CAPABILITY ") && lines[0].contains("COMPRESS=DEFLATE"));

    let server = server_with(MockImapConfig {
        capabilities: MockCapabilities::imap4rev1(),
        ..MockImapConfig::default()
    })
    .await;
    let mut conn = Conn::login(&server).await;
    let caps = conn.cmd("a1", "CAPABILITY").await[0].clone();
    assert_eq!(
        caps.trim_end(),
        "* CAPABILITY IMAP4rev1 SPECIAL-USE AUTH=PLAIN"
    );
    for command in [
        "IDLE",
        "UID MOVE 1 Archive",
        "UID EXPUNGE 1",
        "ENABLE QRESYNC",
    ] {
        let lines = conn.cmd("x", command).await;
        assert!(
            lines.last().unwrap().starts_with("x BAD"),
            "{command}: {lines:?}"
        );
    }
}

#[tokio::test]
async fn idle_pushes_changes_made_by_another_session() {
    let server = server().await;
    let mut idler = Conn::login(&server).await;
    let mut other = Conn::login(&server).await;
    idler.cmd("a1", "SELECT INBOX").await;
    other.cmd("b1", "SELECT INBOX").await;

    idler.send("a2 IDLE\r\n").await;
    assert!(idler.line().await.starts_with("+ "));
    assert!(server.wait_for_idle(1, Duration::from_secs(5)).await);

    other.cmd("b2", "UID STORE 2 +FLAGS (\\Flagged)").await;
    assert_eq!(
        idler.line().await,
        "* 2 FETCH (UID 2 FLAGS (\\Flagged))\r\n"
    );

    server
        .store()
        .add_message("INBOX", b"Subject: d\r\n\r\nx", &[] as &[&str]);
    assert_eq!(idler.line().await, "* 4 EXISTS\r\n");

    other
        .cmd("b3", "UID STORE 1 +FLAGS.SILENT (\\Deleted)")
        .await;
    assert_eq!(
        idler.line().await,
        "* 1 FETCH (UID 1 FLAGS (\\Deleted))\r\n"
    );
    other.cmd("b4", "EXPUNGE").await;
    assert_eq!(idler.line().await, "* 1 EXPUNGE\r\n");

    idler.send("DONE\r\n").await;
    let lines = idler.until_tagged("a2").await;
    assert!(lines.last().unwrap().starts_with("a2 OK"), "{lines:?}");
    assert_eq!(server.idle_sessions(), 0);
    // The idler's view is current: sequence 1 is now UID 2.
    let lines = idler.cmd("a3", "FETCH 1 (UID)").await;
    assert_eq!(lines[0], "* 1 FETCH (UID 2)\r\n");
}

#[tokio::test]
async fn pending_changes_are_reported_at_the_next_command() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    conn.cmd("a1", "SELECT INBOX").await;
    server.store().add_flags("INBOX", 3, &["$Label1"]);
    server.store().remove_message("INBOX", 1);
    let lines = conn.cmd("a2", "NOOP").await;
    assert_eq!(
        lines,
        [
            "* 1 EXPUNGE\r\n",
            "* 2 FETCH (UID 3 FLAGS ($Label1))\r\n",
            "a2 OK NOOP completed\r\n"
        ]
    );
    // No expunge during a FETCH by sequence number; it waits.
    server.store().remove_message("INBOX", 2);
    let lines = conn.cmd("a3", "FETCH 1:* (UID)").await;
    assert!(!lines.iter().any(|l| l.contains("EXPUNGE")), "{lines:?}");
    let lines = conn.cmd("a4", "NOOP").await;
    assert_eq!(lines[0], "* 1 EXPUNGE\r\n");
}

#[tokio::test]
async fn dropped_idle_connections_close_without_bye() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    conn.cmd("a1", "EXAMINE INBOX").await;
    conn.send("a2 IDLE\r\n").await;
    assert!(conn.line().await.starts_with("+ "));
    assert!(server.wait_for_idle(1, Duration::from_secs(5)).await);
    assert_eq!(server.drop_idle_connections(), 1);
    assert_eq!(conn.line().await, "", "closed without a response");
    assert_eq!(server.command_count("IDLE"), 1);
}

#[tokio::test]
async fn condstore_and_qresync() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    let lines = conn.cmd("a1", "ENABLE QRESYNC").await;
    assert_eq!(lines[0], "* ENABLED QRESYNC\r\n");
    let lines = conn.cmd("a2", "SELECT INBOX (CONDSTORE)").await;
    let highest = server.store().highest_modseq("INBOX").unwrap();
    assert!(has(
        &lines,
        &format!("* OK [HIGHESTMODSEQ {highest}] highest")
    ));
    let validity = server.store().mailbox("INBOX").unwrap().uid_validity;

    // Changes after `highest`: a flag on 2, message 3 expunged.
    server.store().add_flags("INBOX", 2, &["\\Seen"]);
    server.store().remove_message("INBOX", 3);
    let lines = conn
        .cmd(
            "a3",
            &format!("EXAMINE INBOX (QRESYNC ({validity} {highest}))"),
        )
        .await;
    assert!(has(&lines, "* VANISHED (EARLIER) 3"), "{lines:?}");
    let modseq_2 = server
        .store()
        .mailbox("INBOX")
        .unwrap()
        .message(2)
        .unwrap()
        .modseq;
    assert!(
        has(
            &lines,
            &format!("* 2 FETCH (UID 2 FLAGS (\\Seen) MODSEQ ({modseq_2}))")
        ),
        "{lines:?}"
    );

    let lines = conn
        .cmd(
            "a4",
            &format!("UID FETCH 1:* (FLAGS) (CHANGEDSINCE {highest} VANISHED)"),
        )
        .await;
    assert_eq!(lines[0], "* VANISHED (EARLIER) 3\r\n");
    assert_eq!(
        lines[1],
        format!("* 2 FETCH (UID 2 FLAGS (\\Seen) MODSEQ ({modseq_2}))\r\n")
    );
    assert_eq!(lines.len(), 3);

    // A wrong UIDVALIDITY gets no QRESYNC data.
    let lines = conn
        .cmd("a5", &format!("EXAMINE INBOX (QRESYNC (1 {highest}))"))
        .await;
    assert!(!lines.iter().any(|l| l.contains("VANISHED")));

    let lines = conn
        .cmd(
            "a6",
            "STATUS INBOX (MESSAGES UIDNEXT UIDVALIDITY UNSEEN HIGHESTMODSEQ)",
        )
        .await;
    assert!(lines[0].starts_with("* STATUS \"INBOX\" (MESSAGES 2 UIDNEXT 4 "));
}

#[tokio::test]
async fn store_modes_and_unchangedsince() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    conn.cmd("a1", "SELECT INBOX").await;
    let lines = conn.cmd("a2", "UID STORE 1:2 +FLAGS ($Work \\Seen)").await;
    assert_eq!(lines[0], "* 1 FETCH (UID 1 FLAGS ($Work \\Seen))\r\n");
    let lines = conn.cmd("a3", "STORE 1 -FLAGS.SILENT ($Work)").await;
    assert_eq!(lines.len(), 1, "silent: {lines:?}");
    let lines = conn.cmd("a4", "UID STORE 2 FLAGS (\\Answered)").await;
    assert_eq!(lines[0], "* 2 FETCH (UID 2 FLAGS (\\Answered))\r\n");
    assert_eq!(server.store().flags("INBOX", 1).unwrap(), ["\\Seen"]);

    let modseq = server.store().highest_modseq("INBOX").unwrap();
    server.store().add_flags("INBOX", 3, &["\\Flagged"]);
    let lines = conn
        .cmd(
            "a5",
            &format!("UID STORE 1,3 (UNCHANGEDSINCE {modseq}) +FLAGS (\\Draft)"),
        )
        .await;
    assert!(
        lines.last().unwrap().starts_with("a5 OK [MODIFIED 3]"),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.contains("UID 1 FLAGS (\\Seen \\Draft) MODSEQ ("))
    );
    let lines = conn.cmd("a6", "EXAMINE INBOX").await;
    assert!(lines.last().unwrap().starts_with("a6 OK"));
    let lines = conn.cmd("a7", "UID STORE 1 +FLAGS (\\Seen)").await;
    assert!(lines.last().unwrap().starts_with("a7 NO"), "read-only");
}

#[tokio::test]
async fn move_copy_expunge_with_uidplus() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    conn.cmd("a1", "ENABLE QRESYNC").await;
    conn.cmd("a2", "SELECT INBOX").await;
    let validity = server.store().mailbox("Archive").unwrap().uid_validity;

    let lines = conn.cmd("a3", "UID COPY 1:2 Archive").await;
    assert_eq!(
        lines.last().unwrap().trim_end(),
        format!("a3 OK [COPYUID {validity} 1,2 1,2] COPY completed")
    );
    let lines = conn.cmd("a4", "UID MOVE 3 Archive").await;
    assert_eq!(
        lines[0].trim_end(),
        format!("* OK [COPYUID {validity} 3 3] moved")
    );
    assert_eq!(lines[1], "* VANISHED 3\r\n");
    let lines = conn.cmd("a5", "UID MOVE 1 Nowhere").await;
    assert!(lines[0].starts_with("a5 NO [TRYCREATE]"));

    conn.cmd("a6", "UID STORE 1:2 +FLAGS.SILENT (\\Deleted)")
        .await;
    let lines = conn.cmd("a7", "UID EXPUNGE 2").await;
    assert_eq!(lines[0], "* VANISHED 2\r\n");
    assert_eq!(
        server
            .store()
            .mailbox("INBOX")
            .unwrap()
            .messages
            .iter()
            .map(|m| m.uid)
            .collect::<Vec<_>>(),
        [1]
    );
    let lines = conn.cmd("a8", "UID SEARCH DELETED").await;
    assert_eq!(lines[0], "* SEARCH 1\r\n");
    // CLOSE expunges silently.
    let lines = conn.cmd("a9", "CLOSE").await;
    assert_eq!(lines.len(), 1);
    assert!(server.store().mailbox("INBOX").unwrap().messages.is_empty());
}

#[tokio::test]
async fn append_with_flags_date_and_limits() {
    let server = server_with(MockImapConfig {
        capabilities: MockCapabilities {
            append_limit: Some(20),
            ..MockCapabilities::default()
        },
        ..MockImapConfig::default()
    })
    .await;
    let mut conn = Conn::login(&server).await;
    let validity = server.store().mailbox("Archive").unwrap().uid_validity;
    let lines = conn
        .cmd(
            "a1",
            "APPEND Archive (\\Seen $Sent) \"28-Sep-2026 08:15:00 +0200\" {12+}\r\nSubject: x\r\n",
        )
        .await;
    assert_eq!(
        lines[0].trim_end(),
        format!("a1 OK [APPENDUID {validity} 1] APPEND completed")
    );
    let archive = server.store().mailbox("Archive").unwrap();
    assert_eq!(archive.messages[0].flags, ["\\Seen", "$Sent"]);
    assert_eq!(
        archive.messages[0].internal_date,
        "28-Sep-2026 08:15:00 +0200"
    );

    // A synchronizing literal above the limit is refused before upload.
    conn.send("a2 APPEND Archive {100}\r\n").await;
    assert!(conn.line().await.starts_with("a2 NO [TOOBIG]"));
    let lines = conn.cmd("a3", "APPEND Nowhere {1+}\r\nx").await;
    assert!(lines[0].starts_with("a3 NO [TRYCREATE]"));
}

#[tokio::test]
async fn folder_commands() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    assert!(conn.cmd("a1", "CREATE Work/").await[0].starts_with("a1 OK"));
    assert!(conn.cmd("a2", "CREATE Work/Old").await[0].starts_with("a2 OK"));
    assert!(conn.cmd("a3", "CREATE Work").await[0].starts_with("a3 NO [ALREADYEXISTS]"));
    assert!(conn.cmd("a4", "RENAME Work Jobs").await[0].starts_with("a4 OK"));
    assert!(server.store().mailbox("Jobs/Old").is_some());
    assert!(conn.cmd("a5", "RENAME Nope X").await[0].starts_with("a5 NO [NONEXISTENT]"));
    assert!(conn.cmd("a6", "DELETE Jobs/Old").await[0].starts_with("a6 OK"));
    assert!(conn.cmd("a7", "DELETE Jobs/Old").await[0].starts_with("a7 NO [NONEXISTENT]"));
    assert!(conn.cmd("a8", "DELETE INBOX").await[0].starts_with("a8 NO"));
    assert!(conn.cmd("a9", "SUBSCRIBE Jobs").await[0].starts_with("a9 OK"));
}

#[tokio::test]
async fn oauth_mechanisms_and_error_challenges() {
    let server = server_with(MockImapConfig {
        oauth_token: Some("good".to_owned()),
        ..MockImapConfig::default()
    })
    .await;
    let xoauth2 = |token: &str| {
        BASE64.encode(format!(
            "user=sam@nail.example\x01auth=Bearer {token}\x01\x01"
        ))
    };
    let mut conn = Conn::open(&server).await;
    conn.send(&format!("a1 AUTHENTICATE XOAUTH2 {}\r\n", xoauth2("bad")))
        .await;
    let challenge = conn.line().await;
    let json = BASE64.decode(challenge[2..].trim()).unwrap();
    assert!(
        String::from_utf8(json)
            .unwrap()
            .contains("\"status\":\"401\"")
    );
    conn.send("\r\n").await;
    assert!(
        conn.line()
            .await
            .starts_with("a1 NO [AUTHENTICATIONFAILED]")
    );
    let lines = conn
        .cmd("a2", &format!("AUTHENTICATE XOAUTH2 {}", xoauth2("good")))
        .await;
    assert!(lines[0].starts_with("a2 OK"), "{lines:?}");

    let mut conn = Conn::open(&server).await;
    conn.send("a1 AUTHENTICATE OAUTHBEARER\r\n").await;
    assert_eq!(conn.line().await, "+ \r\n");
    let bearer =
        BASE64.encode("n,a=sam@nail.example,\x01host=h\x01port=1\x01auth=Bearer good\x01\x01");
    conn.send(&format!("{bearer}\r\n")).await;
    assert!(conn.line().await.starts_with("a1 OK"));
    assert_eq!(
        server.commands(),
        [
            "AUTHENTICATE XOAUTH2",
            "AUTHENTICATE XOAUTH2",
            "AUTHENTICATE OAUTHBEARER"
        ]
    );

    // Token rotation at runtime.
    server.set_oauth_token(Some("rotated"));
    let mut conn = Conn::open(&server).await;
    conn.send(&format!("a1 AUTHENTICATE XOAUTH2 {}\r\n", xoauth2("good")))
        .await;
    assert!(conn.line().await.starts_with("+ "), "old token refused");
    conn.send("\r\n").await;
    assert!(conn.line().await.starts_with("a1 NO"));
    let lines = conn
        .cmd(
            "a2",
            &format!("AUTHENTICATE XOAUTH2 {}", xoauth2("rotated")),
        )
        .await;
    assert!(lines[0].starts_with("a2 OK"), "{lines:?}");
}

#[tokio::test]
async fn sasl_ir_can_be_switched_off() {
    let server = server_with(MockImapConfig {
        capabilities: MockCapabilities::imap4rev1(),
        ..MockImapConfig::default()
    })
    .await;
    let mut conn = Conn::open(&server).await;
    let lines = conn
        .cmd("a1", "AUTHENTICATE PLAIN AHNhbUBuYWlsLmV4YW1wbGUAc2VjcmV0")
        .await;
    assert!(lines[0].starts_with("a1 BAD"), "{lines:?}");
    conn.send("a2 AUTHENTICATE PLAIN\r\n").await;
    assert_eq!(conn.line().await, "+ \r\n");
    conn.send("AHNhbUBuYWlsLmV4YW1wbGUAc2VjcmV0\r\n").await;
    assert!(conn.line().await.starts_with("a2 OK"));
    // Without LITERAL+, a non-synchronizing literal is still read (lenient),
    // but synchronizing literals get a continuation.
    conn.send("a3 LOGIN {3}\r\n").await;
    assert!(conn.line().await.starts_with("+ "));
}

#[tokio::test]
async fn id_is_recorded_and_answered() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    let lines = conn
        .cmd("a1", "ID (\"name\" \"Nail\" \"version\" NIL)")
        .await;
    assert!(lines[0].starts_with("* ID (\"name\" \"Nail mock IMAP\""));
    assert_eq!(
        server.client_ids(),
        [vec![
            ("name".to_owned(), Some("Nail".to_owned())),
            ("version".to_owned(), None)
        ]]
    );
}

#[tokio::test]
async fn slow_responses_are_injected() {
    let server = server().await;
    let mut conn = Conn::login(&server).await;
    server.delay_responses(Some("NOOP"), Duration::from_millis(300));
    let started = std::time::Instant::now();
    conn.cmd("a1", "NOOP").await;
    assert!(started.elapsed() >= Duration::from_millis(300));
    let started = std::time::Instant::now();
    conn.cmd("a2", "CAPABILITY").await;
    assert!(started.elapsed() < Duration::from_millis(300));
    server.clear_delay();
}
