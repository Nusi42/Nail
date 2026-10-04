//! Sync policies (headers only / recent N days), on-demand bodies and
//! server-side search (M2c WP53).
//!
//! The `nail-testkit` mock serves header blocks and `HEADER` searches; the
//! keys it does not implement (`SINCE`, `TEXT`, `CHARSET`) are checked
//! against a small scripted server that records the commands it gets.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};

use crate::common::{login, modern_server};
use nail_proto_imap::{
    BodyFetch, Credentials, ImapClient, ImapConfig, ImapDate, SearchKey, Security,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

fn header_block(raw: &[u8]) -> &[u8] {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(raw.len(), |p| p + 4);
    &raw[..end]
}

#[tokio::test]
async fn headers_only_fetches_header_blocks_and_bodies_come_on_demand() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let plan = client.plan_fetch("INBOX", None).await.unwrap();
    let uids: Vec<u32> = plan.batches.concat();
    assert_eq!(uids, [1, 2, 3]);

    let headers = client
        .fetch_batch_with(&uids, BodyFetch::HeadersOnly)
        .await
        .unwrap();
    assert_eq!(headers.len(), 3);
    for message in &headers {
        assert!(message.headers_only);
        let full = server
            .store()
            .mailbox("INBOX")
            .unwrap()
            .message(message.uid)
            .unwrap()
            .raw
            .clone();
        assert_eq!(message.raw, header_block(&full), "uid {}", message.uid);
        assert_eq!(
            message.size,
            Some(u32::try_from(full.len()).unwrap()),
            "the size is the full message's"
        );
        assert!(message.envelope.is_some());
    }
    assert!(headers[0].flags.seen, "flags come with the headers");

    let full = client
        .fetch_batch_with(&uids, BodyFetch::Full)
        .await
        .unwrap();
    assert!(full.iter().all(|m| !m.headers_only));

    // Opening message 2 fetches its body, from another folder selection.
    client.examine("Archive").await.unwrap();
    let body = client.fetch_body("INBOX", 2).await.unwrap().unwrap();
    assert!(!body.headers_only);
    assert!(String::from_utf8_lossy(&body.raw).contains("Hello two."));
    assert!(client.fetch_body("INBOX", 99).await.unwrap().is_none());
    client.logout().await.unwrap();
}

#[tokio::test]
async fn header_searches_run_on_the_server() {
    let server = modern_server().await;
    let mut client = login(&server).await;
    let found = client
        .search_messages("INBOX", &SearchKey::Header("Subject".into(), "two".into()))
        .await
        .unwrap();
    assert_eq!(found, [2]);
    let unseen_flagged = client
        .search_messages(
            "INBOX",
            &SearchKey::And(vec![SearchKey::Unseen, SearchKey::Flagged]),
        )
        .await
        .unwrap();
    assert_eq!(unseen_flagged, [2]);
    assert!(
        client
            .search_messages("INBOX", &SearchKey::Keyword("bad word".into()))
            .await
            .is_err()
    );
    client.logout().await.unwrap();
}

/// A scripted IMAP server: answers LOGIN, EXAMINE, the searches and fetches
/// of the tests, and records every command (literals included).
async fn scripted_server(capabilities: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log = Arc::new(Mutex::new(Vec::new()));
    let seen = log.clone();
    tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        let (read, mut write) = socket.into_split();
        let mut read = BufReader::new(read);
        write
            .write_all(format!("* OK [CAPABILITY {capabilities}] ready\r\n").as_bytes())
            .await
            .unwrap();
        loop {
            // One command, with its literals.
            let mut command = String::new();
            loop {
                let mut line = String::new();
                if read.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                let line = line.trim_end_matches("\r\n").to_owned();
                let literal = line
                    .strip_suffix('}')
                    .and_then(|l| l.rsplit_once('{'))
                    .map(|(head, n)| (head.to_owned(), n.to_owned()));
                if let Some((head, n)) = literal {
                    let sync = !n.ends_with('+');
                    let len: usize = n.trim_end_matches('+').parse().unwrap();
                    if sync {
                        write.write_all(b"+ go ahead\r\n").await.unwrap();
                    }
                    let mut bytes = vec![0; len];
                    read.read_exact(&mut bytes).await.unwrap();
                    command.push_str(&head);
                    write!(command, "{{{len}}}").unwrap();
                    command.push_str(&String::from_utf8_lossy(&bytes));
                } else {
                    command.push_str(&line);
                    break;
                }
            }
            let (tag, rest) = command.split_once(' ').unwrap();
            seen.lock().unwrap().push(rest.to_owned());
            let upper = rest.to_ascii_uppercase();
            let reply = if upper.starts_with("CAPABILITY") {
                format!("* CAPABILITY {capabilities}\r\n{tag} OK done\r\n")
            } else if upper.starts_with("LOGIN") {
                format!("{tag} OK [CAPABILITY {capabilities}] logged in\r\n")
            } else if upper.starts_with("EXAMINE") {
                format!(
                    "* 3 EXISTS\r\n* OK [UIDVALIDITY 7] ok\r\n* OK [UIDNEXT 4] ok\r\n{tag} OK [READ-ONLY] done\r\n"
                )
            } else if upper.starts_with("UID SEARCH UID") {
                // Messages 2 and 3 are recent.
                format!("* SEARCH 2 3\r\n{tag} OK done\r\n")
            } else if upper.starts_with("UID SEARCH") {
                format!("* SEARCH 3\r\n{tag} OK done\r\n")
            } else if upper.starts_with("UID FETCH") {
                let headers = upper.contains("BODY.PEEK[HEADER]");
                let set = rest.split(' ').nth(2).unwrap().to_owned();
                let uids: Vec<u32> = set
                    .split(',')
                    .flat_map(|part| match part.split_once(':') {
                        Some((a, b)) => {
                            (a.parse::<u32>().unwrap()..=b.parse::<u32>().unwrap()).collect()
                        }
                        None => vec![part.parse().unwrap()],
                    })
                    .collect();
                let mut out = String::new();
                for uid in uids {
                    let (section, data) = if headers {
                        ("BODY[HEADER]", format!("Subject: {uid}\r\n\r\n"))
                    } else {
                        ("BODY[]", format!("Subject: {uid}\r\n\r\nbody {uid}\r\n"))
                    };
                    write!(
                        out,
                        "* {uid} FETCH (UID {uid} FLAGS () RFC822.SIZE 30 {section} {{{}}}\r\n{data})\r\n",
                        data.len()
                    ).unwrap();
                }
                write!(out, "{tag} OK done\r\n").unwrap();
                out
            } else if upper.starts_with("LOGOUT") {
                format!("* BYE bye\r\n{tag} OK done\r\n")
            } else {
                format!("{tag} OK done\r\n")
            };
            write.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (port, log)
}

async fn scripted_client(port: u16) -> ImapClient {
    let config = ImapConfig::new("127.0.0.1")
        .with_port(port)
        .with_security(Security::InsecurePlaintext)
        .with_timeout(crate::common::TIMEOUT)
        .with_compress(false)
        .with_client_id(None);
    ImapClient::connect(&config, &Credentials::new("sam", "secret"))
        .await
        .unwrap()
}

#[tokio::test]
async fn recent_days_fetch_bodies_since_a_day_and_headers_before() {
    let (port, log) = scripted_server("IMAP4rev1 LITERAL+").await;
    let mut client = scripted_client(port).await;
    client.examine("INBOX").await.unwrap();
    let day = ImapDate {
        year: 2026,
        month: 9,
        day: 1,
    };
    let messages = client
        .fetch_batch_with(&[3, 1, 2], BodyFetch::Since(day))
        .await
        .unwrap();
    let summary: Vec<(u32, bool)> = messages.iter().map(|m| (m.uid, m.headers_only)).collect();
    assert_eq!(summary, [(1, true), (2, false), (3, false)]);
    assert_eq!(messages[0].raw, b"Subject: 1\r\n\r\n");
    let commands = log.lock().unwrap().clone();
    assert!(
        commands.contains(&"UID SEARCH UID 1:3 SINCE 1-Sep-2026".to_owned()),
        "{commands:#?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("UID FETCH 2:3 ") && c.ends_with("BODY.PEEK[])")),
        "{commands:#?}"
    );
    assert!(
        commands
            .iter()
            .any(|c| c.starts_with("UID FETCH 1 ") && c.ends_with("BODY.PEEK[HEADER])")),
        "{commands:#?}"
    );
}

#[tokio::test]
async fn non_ascii_searches_send_utf8_literals() {
    for capabilities in ["IMAP4rev1 LITERAL+", "IMAP4rev1"] {
        let (port, log) = scripted_server(capabilities).await;
        let mut client = scripted_client(port).await;
        let key = SearchKey::And(vec![
            SearchKey::Text("Grüße".into()),
            SearchKey::From("ada".into()),
        ]);
        let found = client.search_messages("INBOX", &key).await.unwrap();
        assert_eq!(found, [3]);
        let commands = log.lock().unwrap().clone();
        assert!(
            commands.contains(&"UID SEARCH CHARSET UTF-8 TEXT {7}Grüße FROM \"ada\"".to_owned()),
            "{capabilities}: {commands:#?}"
        );
        assert!(commands.iter().any(|c| c.starts_with("EXAMINE")));
    }
}
