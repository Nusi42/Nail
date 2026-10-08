//! One client connection of the mock IMAP server.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::io;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nail_api::FolderRole;
use nail_net::io::{LineRead, read_line_limited};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::watch;

use super::command::{self, FetchItem, Section, SequenceSet, Token};
use super::deflate::DeflateIo;
use super::encode::{number_list, number_set, push_envelope, push_literal, push_string};
use super::{Control, MessageFault, MockImapConfig, MockTls};
use crate::mailstore::{FlagUpdate, MailStore, Mailbox, MailboxError, StoredFlags, StoredMessage};
use crate::tls::TestTls;

/// Upper bound for one command (literals included), to stop runaway clients.
const MAX_COMMAND_LEN: usize = 64 << 20;
const SYSTEM_FLAGS: &str = "\\Answered \\Flagged \\Deleted \\Seen \\Draft";

trait Io: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Io for T {}

/// State shared by all sessions of one server.
#[derive(Clone)]
pub(crate) struct Shared {
    pub(crate) store: MailStore,
    pub(crate) config: Arc<MockImapConfig>,
    pub(crate) control: Arc<Control>,
    pub(crate) tls: Option<TestTls>,
}

/// What happens to the connection after a command.
enum Flow {
    Continue,
    Close,
    StartTls,
    Compress,
}

/// One command read from the client.
enum Input {
    /// The complete command, literals inlined as `{n}\r\n<bytes>`.
    Line(Vec<u8>),
    /// A synchronizing `APPEND` literal above `APPENDLIMIT`: answered with
    /// `NO [TOOBIG]` instead of a continuation.
    TooBig(String),
}

/// Status and text of the tagged completion.
type Reply = (&'static str, String);

fn ok(text: impl Into<String>) -> Reply {
    ("OK", text.into())
}

fn no(text: impl Into<String>) -> Reply {
    ("NO", text.into())
}

fn bad(text: impl Into<String>) -> Reply {
    ("BAD", text.into())
}

/// A session's view of its selected mailbox: what the client has been told.
struct View {
    mailbox: String,
    read_only: bool,
    uid_validity: u32,
    messages: Vec<ViewMessage>,
}

struct ViewMessage {
    uid: u32,
    flags: Vec<String>,
    modseq: u64,
}

impl View {
    fn snapshot(mailbox: &Mailbox, read_only: bool) -> Self {
        Self {
            mailbox: mailbox.name.clone(),
            read_only,
            uid_validity: mailbox.uid_validity,
            messages: mailbox
                .messages
                .iter()
                .map(|m| ViewMessage {
                    uid: m.uid,
                    flags: m.flags.clone(),
                    modseq: m.modseq,
                })
                .collect(),
        }
    }

    fn max_uid(&self) -> u32 {
        self.messages.last().map_or(0, |m| m.uid)
    }

    fn seq_of(&self, uid: u32) -> Option<u32> {
        self.messages
            .iter()
            .position(|m| m.uid == uid)
            .map(seq_number)
    }

    /// `(sequence number, UID)` of the messages matching `set`.
    fn matching(&self, set: &SequenceSet, uid: bool) -> Vec<(u32, u32)> {
        let max = if uid {
            self.max_uid()
        } else {
            u32::try_from(self.messages.len()).unwrap_or(u32::MAX)
        };
        self.messages
            .iter()
            .enumerate()
            .filter(|(i, m)| set.contains(if uid { m.uid } else { seq_number(*i) }, max))
            .map(|(i, m)| (seq_number(i), m.uid))
            .collect()
    }
}

fn seq_number(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent IMAP session states (authenticated, TLS, compression, enabled extensions)"
)]
struct Session {
    shared: Shared,
    authenticated: bool,
    tls_active: bool,
    compressed: bool,
    /// CONDSTORE is enabled: flag updates carry `MODSEQ`.
    condstore: bool,
    /// QRESYNC is enabled: expunges are reported as `VANISHED`.
    qresync: bool,
    view: Option<View>,
    changes: watch::Receiver<u64>,
    stream: BufReader<Box<dyn Io>>,
}

/// Serves one connection until logout, disconnect or server shutdown.
pub(crate) async fn run(stream: TcpStream, shared: Shared, mut shutdown: watch::Receiver<bool>) {
    tokio::select! {
        result = Box::pin(serve(stream, shared)) => {
            if let Err(err) = result {
                tracing::debug!(%err, "mock IMAP session ended with error");
            }
        }
        _ = shutdown.changed() => {}
    }
}

async fn serve(stream: TcpStream, shared: Shared) -> io::Result<()> {
    let implicit = shared.config.tls == MockTls::Implicit;
    let stream: Box<dyn Io> = match (&shared.tls, implicit) {
        (Some(tls), true) => Box::new(tls.acceptor.accept(stream).await?),
        _ => Box::new(stream),
    };
    let changes = shared.store.subscribe();
    let mut session = Session {
        shared,
        authenticated: false,
        tls_active: implicit,
        compressed: false,
        condstore: false,
        qresync: false,
        view: None,
        changes,
        stream: BufReader::new(stream),
    };
    session
        .send(b"* OK Nail mock IMAP4rev1 server ready\r\n")
        .await?;

    loop {
        let line = match session.read_command().await? {
            None => return Ok(()),
            Some(Input::Line(line)) => line,
            Some(Input::TooBig(tag)) => {
                session
                    .tagged(&tag, "NO", "[TOOBIG] message exceeds APPENDLIMIT")
                    .await?;
                continue;
            }
        };
        match session.handle(&line).await? {
            Flow::Continue => {}
            Flow::Close => {
                // Close gracefully (TLS close_notify, FIN), so the client
                // reads the final responses instead of a reset.
                drop(
                    tokio::time::timeout(
                        std::time::Duration::from_secs(1),
                        session.stream.get_mut().shutdown(),
                    )
                    .await,
                );
                return Ok(());
            }
            Flow::StartTls => {
                let Some(tls) = session.shared.tls.clone() else {
                    return Ok(());
                };
                // Anything the client pipelined after STARTTLS is discarded
                // (as required, to prevent command injection).
                let plain = session.take_stream();
                let upgraded = tls.acceptor.accept(plain.into_inner()).await?;
                session.stream = BufReader::new(Box::new(upgraded));
                session.tls_active = true;
            }
            Flow::Compress => {
                let plain = session.take_stream();
                if !plain.buffer().is_empty() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "data pipelined after COMPRESS",
                    ));
                }
                session.stream = BufReader::new(Box::new(DeflateIo::new(plain.into_inner())));
                session.compressed = true;
                session
                    .shared
                    .control
                    .compressed
                    .fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

impl Session {
    fn take_stream(&mut self) -> BufReader<Box<dyn Io>> {
        std::mem::replace(
            &mut self.stream,
            BufReader::new(Box::new(tokio::io::duplex(1).0)),
        )
    }

    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        if bytes.is_empty() {
            return Ok(());
        }
        let stream = self.stream.get_mut();
        stream.write_all(bytes).await?;
        stream.flush().await
    }

    async fn tagged(&mut self, tag: &str, status: &str, text: &str) -> io::Result<()> {
        self.send(format!("{tag} {status} {text}\r\n").as_bytes())
            .await
    }

    /// Reads one line (without CRLF); `None` at EOF.
    async fn read_line(&mut self, limit: usize) -> io::Result<Option<Vec<u8>>> {
        let limit = u64::try_from(limit).unwrap_or(u64::MAX);
        match read_line_limited(&mut self.stream, limit).await? {
            LineRead::Eof => Ok(None),
            LineRead::TooLong => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "command too long",
            )),
            LineRead::Line(mut line) => {
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                Ok(Some(line))
            }
        }
    }

    /// Reads one command including any literals; `None` at EOF.
    async fn read_command(&mut self) -> io::Result<Option<Input>> {
        let mut command = Vec::new();
        loop {
            let limit = MAX_COMMAND_LEN - command.len().min(MAX_COMMAND_LEN);
            let Some(line) = self.read_line(limit).await? else {
                return Ok(None);
            };
            let literal = command::trailing_literal(&line);
            command.extend_from_slice(&line);
            let Some((len, synchronizing)) = literal else {
                return Ok(Some(Input::Line(command)));
            };
            if command.len() + len > MAX_COMMAND_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "literal too long",
                ));
            }
            if synchronizing {
                if let Some(tag) = self.oversized_append(&command, len) {
                    return Ok(Some(Input::TooBig(tag)));
                }
                self.send(b"+ Ready for literal data\r\n").await?;
            }
            let mut data = vec![0; len];
            self.stream.read_exact(&mut data).await?;
            command.extend_from_slice(b"\r\n");
            command.extend_from_slice(&data);
        }
    }

    /// The tag of an `APPEND` whose announced literal exceeds `APPENDLIMIT`.
    fn oversized_append(&self, command: &[u8], len: usize) -> Option<String> {
        let limit = self.shared.config.capabilities.append_limit?;
        if (len as u64) <= limit {
            return None;
        }
        let text = String::from_utf8_lossy(command);
        let mut words = text.split(' ');
        let tag = words.next()?;
        words
            .next()
            .is_some_and(|verb| verb.eq_ignore_ascii_case("APPEND"))
            .then(|| tag.to_owned())
    }

    fn oauth_offered(&self) -> bool {
        self.shared.config.oauth_token.is_some()
    }

    fn capabilities(&self) -> String {
        let caps = &self.shared.config.capabilities;
        let mut list = vec!["IMAP4rev1".to_owned()];
        let optional = [
            (caps.literal_plus, "LITERAL+"),
            (caps.special_use, "SPECIAL-USE"),
            (caps.idle, "IDLE"),
            (caps.condstore, "CONDSTORE"),
            (caps.qresync_offered(), "QRESYNC"),
            (caps.enable, "ENABLE"),
            (caps.move_, "MOVE"),
            (caps.uidplus, "UIDPLUS"),
            (caps.id, "ID"),
            (caps.sasl_ir, "SASL-IR"),
            (
                caps.compress && self.authenticated && !self.compressed,
                "COMPRESS=DEFLATE",
            ),
        ];
        list.extend(
            optional
                .iter()
                .filter(|(on, _)| *on)
                .map(|(_, name)| (*name).to_owned()),
        );
        if let Some(limit) = caps.append_limit.filter(|_| caps.advertise_append_limit) {
            list.push(format!("APPENDLIMIT={limit}"));
        }
        if self.login_disabled() {
            list.push("STARTTLS".to_owned());
            list.push("LOGINDISABLED".to_owned());
        } else {
            list.push("AUTH=PLAIN".to_owned());
            if self.oauth_offered() {
                list.push("AUTH=XOAUTH2".to_owned());
                list.push("AUTH=OAUTHBEARER".to_owned());
            }
        }
        list.join(" ")
    }

    async fn handle(&mut self, line: &[u8]) -> io::Result<Flow> {
        if self.shared.control.offline.load(Ordering::SeqCst) {
            tracing::debug!("mock IMAP offline: closing the session");
            return Ok(Flow::Close);
        }
        if self.shared.control.should_disconnect_line(line) {
            tracing::debug!("mock IMAP: injected disconnect (after N commands)");
            return Ok(Flow::Close);
        }
        let tokens = match command::tokenize(line) {
            Ok(tokens) => tokens,
            Err(err) => {
                self.send(format!("* BAD {err}\r\n").as_bytes()).await?;
                return Ok(Flow::Continue);
            }
        };
        let mut args = tokens.into_iter();
        let (Some(Token::Atom(tag)), Some(Token::Atom(name))) = (args.next(), args.next()) else {
            self.send(b"* BAD missing tag or command\r\n").await?;
            return Ok(Flow::Continue);
        };
        let mut name = name.to_ascii_uppercase();
        let mut uid = false;
        let mut args: Vec<Token> = args.collect();
        if name == "UID" {
            let sub = if let Some(Token::Atom(sub)) = args.first() {
                sub.to_ascii_uppercase()
            } else {
                self.tagged(&tag, "BAD", "UID needs a command").await?;
                return Ok(Flow::Continue);
            };
            args.remove(0);
            uid = true;
            name = sub;
        }
        let full_name = if uid {
            format!("UID {name}")
        } else {
            name.clone()
        };
        let recorded = if name == "AUTHENTICATE" {
            let mechanism = args.first().and_then(Token::as_text).unwrap_or_default();
            format!("AUTHENTICATE {}", mechanism.to_ascii_uppercase())
        } else {
            full_name.clone()
        };
        self.shared.control.record_command(recorded);
        if self.shared.control.should_disconnect(&full_name) {
            tracing::debug!(command = %full_name, "mock IMAP: injected disconnect");
            return Ok(Flow::Close);
        }
        if let Some(delay) = self.shared.control.delay_for(&full_name) {
            tokio::time::sleep(delay).await;
        }
        if self.shared.control.should_fail(&full_name) {
            self.tagged(&tag, "NO", "[SERVERBUG] injected failure")
                .await?;
            return Ok(Flow::Continue);
        }

        // Commands that manage the connection or the selection themselves.
        match name.as_str() {
            "LOGOUT" => {
                self.send(b"* BYE Nail mock server logging out\r\n").await?;
                self.tagged(&tag, "OK", "LOGOUT completed").await?;
                return Ok(Flow::Close);
            }
            "STARTTLS" => return self.starttls(&tag).await,
            "AUTHENTICATE" => return Box::pin(self.authenticate(&tag, &args)).await,
            "IDLE" if self.authenticated => return Box::pin(self.idle(&tag)).await,
            "COMPRESS" if self.authenticated => return self.compress(&tag, &args).await,
            _ => {}
        }

        // Report what other sessions changed before and after the command.
        // EXPUNGE must not be sent during FETCH / STORE / SEARCH / COPY /
        // MOVE by sequence number, as it would shift the numbers.
        let syncs = self.authenticated
            && self.view.is_some()
            && !matches!(name.as_str(), "SELECT" | "EXAMINE" | "CLOSE" | "UNSELECT");
        let allow_expunge = uid
            || !matches!(
                name.as_str(),
                "FETCH" | "STORE" | "SEARCH" | "COPY" | "MOVE"
            );
        if syncs {
            let out = self.sync_view(allow_expunge);
            self.send(&out).await?;
        }

        let (status, text) = match name.as_str() {
            "CAPABILITY" => {
                let caps = self.capabilities();
                self.send(format!("* CAPABILITY {caps}\r\n").as_bytes())
                    .await?;
                ok("CAPABILITY completed")
            }
            "NOOP" => ok("NOOP completed"),
            "CHECK" if self.view.is_some() => ok("CHECK completed"),
            "LOGIN" => self.login(&args),
            _ if !self.authenticated => bad("command not allowed before authentication"),
            "ENABLE" => self.enable(&args).await?,
            "ID" if self.shared.config.capabilities.id => self.id(&args).await?,
            "LIST" => self.list(&args).await?,
            "STATUS" => self.status(&args).await?,
            "SELECT" => self.select(&args, false).await?,
            "EXAMINE" => self.select(&args, true).await?,
            "CREATE" => self.create(&args),
            "DELETE" => self.delete(&args),
            "RENAME" => self.rename(&args),
            "SUBSCRIBE" | "UNSUBSCRIBE" => ok(format!("{name} completed")),
            "APPEND" => self.append(&args),
            "CLOSE" | "UNSELECT" if self.view.is_some() => {
                if let Some(view) = self.view.take()
                    && name == "CLOSE"
                    && !view.read_only
                {
                    self.shared.store.expunge_deleted(&view.mailbox, |_| true);
                }
                ok(format!("{name} completed"))
            }
            _ if self.view.is_none() => bad("no mailbox selected"),
            "FETCH" => self.fetch(&args, uid).await?,
            "SEARCH" => self.search(&args, uid).await?,
            "STORE" => match self.writable() {
                Ok(mailbox) => self.store(mailbox, &args, uid).await?,
                Err(refused) => refused,
            },
            "COPY" => self.copy(&args, uid, false).await?,
            "MOVE" if self.shared.config.capabilities.move_ => self.copy(&args, uid, true).await?,
            "EXPUNGE" if !uid || self.shared.config.capabilities.uidplus => match self.writable() {
                Ok(mailbox) => self.expunge(mailbox, &args, uid).await?,
                Err(refused) => refused,
            },
            _ => bad("unsupported command"),
        };
        if syncs && self.view.is_some() {
            let out = self.sync_view(allow_expunge);
            self.send(&out).await?;
        }
        self.tagged(&tag, status, &text).await?;
        Ok(Flow::Continue)
    }

    /// Brings the session's view up to date with the store and returns the
    /// untagged responses that tell the client about it.
    fn sync_view(&mut self, allow_expunge: bool) -> Vec<u8> {
        self.changes.borrow_and_update();
        let condstore = self.condstore;
        let qresync = self.qresync;
        let Some(view) = self.view.as_mut() else {
            return Vec::new();
        };
        let Some(mailbox) = self.shared.store.mailbox(&view.mailbox) else {
            return Vec::new();
        };
        if mailbox.uid_validity != view.uid_validity {
            // A rebuilt mailbox: a real server would send BYE. The mock
            // quietly switches to the new state; clients re-examine anyway.
            *view = View::snapshot(&mailbox, view.read_only);
            return Vec::new();
        }
        let mut out = Vec::new();
        let current: HashMap<u32, &StoredMessage> =
            mailbox.messages.iter().map(|m| (m.uid, m)).collect();

        if allow_expunge {
            let gone: Vec<(u32, u32)> = view
                .messages
                .iter()
                .enumerate()
                .filter(|(_, m)| !current.contains_key(&m.uid))
                .map(|(i, m)| (seq_number(i), m.uid))
                .collect();
            if !gone.is_empty() {
                if qresync {
                    let uids: Vec<u32> = gone.iter().map(|g| g.1).collect();
                    out.extend_from_slice(
                        format!("* VANISHED {}\r\n", number_set(&uids)).as_bytes(),
                    );
                } else {
                    for (seq, _) in gone.iter().rev() {
                        out.extend_from_slice(format!("* {seq} EXPUNGE\r\n").as_bytes());
                    }
                }
                view.messages.retain(|m| current.contains_key(&m.uid));
            }
        }

        for (index, seen) in view.messages.iter_mut().enumerate() {
            let Some(stored) = current.get(&seen.uid) else {
                continue;
            };
            if stored.modseq == seen.modseq {
                continue;
            }
            if stored.flags != seen.flags {
                out.extend_from_slice(
                    flag_fetch(seq_number(index), stored, true, condstore).as_bytes(),
                );
            }
            seen.flags = stored.flags.clone();
            seen.modseq = stored.modseq;
        }

        let max_uid = view.max_uid();
        let before = view.messages.len();
        view.messages.extend(
            mailbox
                .messages
                .iter()
                .filter(|m| m.uid > max_uid)
                .map(|m| ViewMessage {
                    uid: m.uid,
                    flags: m.flags.clone(),
                    modseq: m.modseq,
                }),
        );
        if view.messages.len() != before {
            out.extend_from_slice(format!("* {} EXISTS\r\n", view.messages.len()).as_bytes());
        }
        out
    }

    /// Records that the client now knows the store state of `uid`.
    fn update_view(&mut self, uid: u32, flags: &[String], modseq: u64) {
        if let Some(message) = self
            .view
            .as_mut()
            .and_then(|v| v.messages.iter_mut().find(|m| m.uid == uid))
        {
            message.flags = flags.to_vec();
            message.modseq = modseq;
        }
    }

    async fn starttls(&mut self, tag: &str) -> io::Result<Flow> {
        if self.shared.config.tls != MockTls::StartTls || self.tls_active {
            self.tagged(tag, "BAD", "STARTTLS not available").await?;
            return Ok(Flow::Continue);
        }
        self.tagged(tag, "OK", "begin TLS negotiation now").await?;
        Ok(Flow::StartTls)
    }

    async fn compress(&mut self, tag: &str, args: &[Token]) -> io::Result<Flow> {
        if !self.shared.config.capabilities.compress {
            self.tagged(tag, "BAD", "unsupported command").await?;
        } else if self.compressed {
            self.tagged(tag, "NO", "[COMPRESSIONACTIVE] already compressing")
                .await?;
        } else if !args
            .first()
            .and_then(Token::as_text)
            .is_some_and(|a| a.eq_ignore_ascii_case("DEFLATE"))
        {
            self.tagged(tag, "BAD", "unknown compression mechanism")
                .await?;
        } else {
            self.tagged(tag, "OK", "DEFLATE active").await?;
            return Ok(Flow::Compress);
        }
        Ok(Flow::Continue)
    }

    fn login_disabled(&self) -> bool {
        self.shared.config.tls == MockTls::StartTls && !self.tls_active
    }

    fn check_credentials(&self, user: &[u8], pass: &[u8]) -> bool {
        user == self.shared.config.username.as_bytes()
            && pass == self.shared.config.password.as_bytes()
    }

    fn check_token(&self, user: &str, token: &str) -> bool {
        user == self.shared.config.username
            && self
                .shared
                .control
                .oauth_token()
                .is_some_and(|valid| valid == token)
    }

    fn login(&mut self, args: &[Token]) -> Reply {
        if self.authenticated {
            return bad("already authenticated");
        }
        if self.login_disabled() {
            return no("[PRIVACYREQUIRED] LOGIN disabled before STARTTLS");
        }
        let [user, pass] = args else {
            return bad("LOGIN needs user name and password");
        };
        let (Some(user), Some(pass)) = (user.as_text(), pass.as_text()) else {
            return bad("LOGIN needs user name and password");
        };
        if self.check_credentials(user.as_bytes(), pass.as_bytes()) {
            self.authenticated = true;
            ok(format!(
                "[CAPABILITY {}] LOGIN completed",
                self.capabilities()
            ))
        } else {
            no("[AUTHENTICATIONFAILED] invalid credentials")
        }
    }

    async fn authenticate(&mut self, tag: &str, args: &[Token]) -> io::Result<Flow> {
        let reply = self.authenticate_reply(args).await?;
        match reply {
            Some((status, text)) => self.tagged(tag, status, &text).await?,
            None => return Ok(Flow::Close),
        }
        Ok(Flow::Continue)
    }

    /// Runs the SASL exchange; `None` when the client disconnected.
    async fn authenticate_reply(&mut self, args: &[Token]) -> io::Result<Option<Reply>> {
        if self.authenticated {
            return Ok(Some(bad("already authenticated")));
        }
        let mechanism = args
            .first()
            .and_then(Token::as_text)
            .unwrap_or_default()
            .to_ascii_uppercase();
        let oauth = matches!(mechanism.as_str(), "XOAUTH2" | "OAUTHBEARER");
        if mechanism != "PLAIN" && !(oauth && self.oauth_offered()) {
            return Ok(Some(no("unsupported authentication mechanism")));
        }
        if self.login_disabled() {
            return Ok(Some(no(
                "[PRIVACYREQUIRED] authentication disabled before STARTTLS",
            )));
        }
        let initial = args.get(1).and_then(Token::as_text);
        if initial.is_some() && !self.shared.config.capabilities.sasl_ir {
            return Ok(Some(bad("SASL-IR is not supported")));
        }
        let response = if let Some(initial) = initial {
            initial
        } else {
            self.send(b"+ \r\n").await?;
            let Some(line) = self.read_line(MAX_COMMAND_LEN).await? else {
                return Ok(None);
            };
            String::from_utf8_lossy(&line).trim().to_owned()
        };
        if response == "*" {
            return Ok(Some(bad("authentication cancelled")));
        }
        let decoded = if response == "=" {
            Ok(Vec::new())
        } else {
            BASE64.decode(response.as_bytes())
        };
        let Ok(decoded) = decoded else {
            return Ok(Some(bad("invalid base64")));
        };
        let accepted = match mechanism.as_str() {
            "PLAIN" => {
                let mut parts = decoded.split(|&b| b == 0);
                match (parts.next(), parts.next(), parts.next(), parts.next()) {
                    (Some(_authzid), Some(user), Some(pass), None) => {
                        self.check_credentials(user, pass)
                    }
                    _ => {
                        return Ok(Some(no("[AUTHENTICATIONFAILED] malformed PLAIN response")));
                    }
                }
            }
            "XOAUTH2" => parse_xoauth2(&decoded).is_some_and(|(u, t)| self.check_token(&u, &t)),
            _ => parse_oauthbearer(&decoded, &self.shared.config.username)
                .is_some_and(|(u, t)| self.check_token(&u, &t)),
        };
        if accepted {
            self.authenticated = true;
            return Ok(Some(ok(format!(
                "[CAPABILITY {}] AUTHENTICATE completed",
                self.capabilities()
            ))));
        }
        if oauth {
            // RFC 7628 / Google's XOAUTH2: an error challenge with JSON
            // details; the client answers with a dummy response.
            let details = BASE64.encode(
                br#"{"status":"401","schemes":"bearer","scope":"https://mail.nail.example/"}"#,
            );
            self.send(format!("+ {details}\r\n").as_bytes()).await?;
            if self.read_line(MAX_COMMAND_LEN).await?.is_none() {
                return Ok(None);
            }
        }
        Ok(Some(no("[AUTHENTICATIONFAILED] invalid credentials")))
    }

    async fn enable(&mut self, args: &[Token]) -> io::Result<Reply> {
        let caps = self.shared.config.capabilities;
        if !caps.enable {
            return Ok(bad("unsupported command"));
        }
        let mut enabled = Vec::new();
        for arg in args {
            let Some(name) = arg.as_text().map(|n| n.to_ascii_uppercase()) else {
                return Ok(bad("ENABLE needs capability names"));
            };
            match name.as_str() {
                "CONDSTORE" if caps.condstore => {
                    self.condstore = true;
                    enabled.push(name);
                }
                "QRESYNC" if caps.qresync_offered() => {
                    self.condstore = true;
                    self.qresync = true;
                    enabled.push(name);
                }
                _ => {}
            }
        }
        let mut line = String::from("* ENABLED");
        for name in enabled {
            line.push(' ');
            line.push_str(&name);
        }
        line.push_str("\r\n");
        self.send(line.as_bytes()).await?;
        Ok(ok("ENABLE completed"))
    }

    async fn id(&mut self, args: &[Token]) -> io::Result<Reply> {
        let params = match args.first() {
            Some(Token::List(items)) => {
                let mut params = Vec::new();
                for pair in items.chunks(2) {
                    let [key, value] = pair else {
                        return Ok(bad("ID needs key/value pairs"));
                    };
                    let value = match value {
                        Token::Atom(atom) if atom.eq_ignore_ascii_case("NIL") => None,
                        other => other.as_text(),
                    };
                    params.push((key.as_text().unwrap_or_default(), value));
                }
                params
            }
            Some(Token::Atom(atom)) if atom.eq_ignore_ascii_case("NIL") => Vec::new(),
            _ => return Ok(bad("ID needs a parameter list or NIL")),
        };
        self.shared.control.record_client_id(params);
        self.send(b"* ID (\"name\" \"Nail mock IMAP\" \"version\" \"1.0\")\r\n")
            .await?;
        Ok(ok("ID completed"))
    }

    async fn list(&mut self, args: &[Token]) -> io::Result<Reply> {
        let [reference, pattern] = args else {
            return Ok(bad("LIST needs reference and pattern"));
        };
        let (Some(reference), Some(pattern)) = (reference.as_text(), pattern.as_text()) else {
            return Ok(bad("LIST needs reference and pattern"));
        };
        let mut out = Vec::new();
        if pattern.is_empty() {
            out.extend_from_slice(b"* LIST (\\Noselect) \"/\" \"\"\r\n");
        } else {
            let pattern = format!("{reference}{pattern}");
            let special_use_on = self.shared.config.capabilities.special_use;
            let mailboxes = self.shared.store.mailboxes();
            for mailbox in &mailboxes {
                if !wildcard_match(&pattern, &mailbox.name) {
                    continue;
                }
                let prefix = format!("{}/", mailbox.name);
                let has_children = mailboxes.iter().any(|m| m.name.starts_with(&prefix));
                let mut attrs = vec![if has_children {
                    "\\HasChildren"
                } else {
                    "\\HasNoChildren"
                }];
                if special_use_on {
                    attrs.extend(special_use(mailbox.role));
                }
                out.extend_from_slice(format!("* LIST ({}) \"/\" ", attrs.join(" ")).as_bytes());
                push_string(&mut out, mailbox.name.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
        }
        self.send(&out).await?;
        Ok(ok("LIST completed"))
    }

    async fn status(&mut self, args: &[Token]) -> io::Result<Reply> {
        let (Some(name), Some(Token::List(items))) =
            (args.first().and_then(Token::as_text), args.get(1))
        else {
            return Ok(bad("STATUS needs a mailbox and items"));
        };
        let Some(mailbox) = self.shared.store.mailbox(&name) else {
            return Ok(no("[NONEXISTENT] no such mailbox"));
        };
        let mut values = Vec::new();
        for item in items {
            let item = item.as_text().unwrap_or_default().to_ascii_uppercase();
            let value = match item.as_str() {
                "MESSAGES" => mailbox.messages.len() as u64,
                "UIDNEXT" => u64::from(mailbox.uid_next),
                "UIDVALIDITY" => u64::from(mailbox.uid_validity),
                "UNSEEN" => mailbox
                    .messages
                    .iter()
                    .filter(|m| !m.has_flag("\\Seen"))
                    .count() as u64,
                "RECENT" => 0,
                "HIGHESTMODSEQ" if self.shared.config.capabilities.condstore => {
                    mailbox.highest_modseq
                }
                _ => return Ok(bad(format!("unsupported STATUS item {item}"))),
            };
            values.push(format!("{item} {value}"));
        }
        let mut out = b"* STATUS ".to_vec();
        push_string(&mut out, mailbox.name.as_bytes());
        out.extend_from_slice(format!(" ({})\r\n", values.join(" ")).as_bytes());
        self.send(&out).await?;
        Ok(ok("STATUS completed"))
    }

    async fn select(&mut self, args: &[Token], read_only: bool) -> io::Result<Reply> {
        self.view = None;
        let Some(name) = args.first().and_then(Token::as_text) else {
            return Ok(bad("mailbox name required"));
        };
        let caps = self.shared.config.capabilities;
        // Select parameters: (CONDSTORE) or (QRESYNC (uidvalidity modseq [known-uids])).
        let mut qresync_params: Option<(u32, u64, Option<SequenceSet>)> = None;
        if args.get(1).is_some() && self.shared.control.refuses_select_parameters() {
            return Ok(bad("Error in IMAP command: Unknown parameter"));
        }
        match args.get(1) {
            None => {}
            Some(Token::List(params)) => {
                let mut iter = params.iter();
                while let Some(param) = iter.next() {
                    let key = param.as_text().unwrap_or_default().to_ascii_uppercase();
                    match key.as_str() {
                        "CONDSTORE" if caps.condstore => self.condstore = true,
                        "QRESYNC" if self.qresync => {
                            let Some(Token::List(values)) = iter.next() else {
                                return Ok(bad("QRESYNC needs parameters"));
                            };
                            let text: Vec<String> =
                                values.iter().filter_map(Token::as_text).collect();
                            let (Some(validity), Some(modseq)) = (
                                text.first().and_then(|v| v.parse().ok()),
                                text.get(1).and_then(|v| v.parse().ok()),
                            ) else {
                                return Ok(bad("invalid QRESYNC parameters"));
                            };
                            let known = text.get(2).and_then(|v| SequenceSet::parse(v));
                            qresync_params = Some((validity, modseq, known));
                        }
                        _ => return Ok(bad(format!("unsupported select parameter {key}"))),
                    }
                }
            }
            Some(_) => return Ok(bad("invalid select parameters")),
        }
        if self.shared.control.is_denied(&name) {
            return Ok(no("[NOPERM] access to this mailbox is denied"));
        }
        let Some(mailbox) = self.shared.store.mailbox(&name) else {
            return Ok(no("[NONEXISTENT] no such mailbox"));
        };
        self.changes.borrow_and_update();
        let permanent = if read_only {
            String::new()
        } else {
            format!("{SYSTEM_FLAGS} \\*")
        };
        let mut out = format!(
            "* FLAGS ({SYSTEM_FLAGS})\r\n* {} EXISTS\r\n* 0 RECENT\r\n\
             * OK [PERMANENTFLAGS ({permanent})] flags\r\n\
             * OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {}] predicted next UID\r\n",
            mailbox.messages.len(),
            mailbox.uid_validity,
            mailbox.uid_next
        );
        if caps.condstore {
            let _ = write!(
                out,
                "* OK [HIGHESTMODSEQ {}] highest\r\n",
                mailbox.highest_modseq
            );
        }
        if let Some((validity, modseq, known)) = qresync_params
            && validity == mailbox.uid_validity
        {
            let max_uid = mailbox.uid_next.saturating_sub(1);
            let mut vanished: Vec<u32> = mailbox
                .expunged
                .iter()
                .filter(|(uid, m)| {
                    *m > modseq && known.as_ref().is_none_or(|k| k.contains(*uid, max_uid))
                })
                .map(|(uid, _)| *uid)
                .collect();
            vanished.sort_unstable();
            vanished.dedup();
            if !vanished.is_empty() {
                let _ = write!(out, "* VANISHED (EARLIER) {}\r\n", number_set(&vanished));
            }
            for (index, message) in mailbox.messages.iter().enumerate() {
                if message.modseq > modseq {
                    out.push_str(&flag_fetch(seq_number(index), message, true, true));
                }
            }
        }
        self.send(out.as_bytes()).await?;
        self.view = Some(View::snapshot(&mailbox, read_only));
        let (code, verb) = if read_only {
            ("READ-ONLY", "EXAMINE")
        } else {
            ("READ-WRITE", "SELECT")
        };
        Ok(ok(format!("[{code}] {verb} completed")))
    }

    fn create(&self, args: &[Token]) -> Reply {
        let Some(name) = args.first().and_then(Token::as_text) else {
            return bad("mailbox name required");
        };
        let name = name.trim_end_matches('/');
        if name.is_empty() {
            return bad("mailbox name required");
        }
        if self.shared.store.add_mailbox(name, FolderRole::Other) {
            ok("CREATE completed")
        } else {
            no("[ALREADYEXISTS] mailbox already exists")
        }
    }

    fn delete(&self, args: &[Token]) -> Reply {
        let Some(name) = args.first().and_then(Token::as_text) else {
            return bad("mailbox name required");
        };
        if name.eq_ignore_ascii_case("INBOX") {
            return no("[CANNOT] INBOX cannot be deleted");
        }
        if self.shared.store.delete_mailbox(&name) {
            ok("DELETE completed")
        } else {
            no("[NONEXISTENT] no such mailbox")
        }
    }

    fn rename(&self, args: &[Token]) -> Reply {
        let (Some(from), Some(to)) = (
            args.first().and_then(Token::as_text),
            args.get(1).and_then(Token::as_text),
        ) else {
            return bad("RENAME needs two mailbox names");
        };
        match self.shared.store.rename_mailbox(&from, &to) {
            Ok(()) => ok("RENAME completed"),
            Err(MailboxError::NonExistent) => no("[NONEXISTENT] no such mailbox"),
            Err(MailboxError::AlreadyExists) => no("[ALREADYEXISTS] target exists"),
            Err(MailboxError::NotAllowed) => no("[CANNOT] cannot rename this mailbox"),
        }
    }

    fn append(&self, args: &[Token]) -> Reply {
        let (Some(name), Some(Token::Str(message))) =
            (args.first().and_then(Token::as_text), args.last())
        else {
            return bad("APPEND needs a mailbox and a message literal");
        };
        if args.len() < 2 {
            return bad("APPEND needs a mailbox and a message literal");
        }
        let rest = &args[1..args.len() - 1];
        let mut flags: Vec<String> = Vec::new();
        let mut date = None;
        for token in rest {
            match token {
                Token::List(items) => flags = items.iter().filter_map(Token::as_text).collect(),
                Token::Str(bytes) => date = Some(String::from_utf8_lossy(bytes).into_owned()),
                Token::Atom(_) => return bad("unexpected APPEND argument"),
            }
        }
        let caps = self.shared.config.capabilities;
        if caps
            .append_limit
            .is_some_and(|limit| message.len() as u64 > limit)
        {
            return no("[TOOBIG] message exceeds APPENDLIMIT");
        }
        let Some(uid) = self
            .shared
            .store
            .append_with_date(&name, message, &flags, date.as_deref())
        else {
            return no("[TRYCREATE] no such mailbox");
        };
        match self.shared.store.mailbox(&name) {
            Some(mailbox) if caps.uidplus => ok(format!(
                "[APPENDUID {} {uid}] APPEND completed",
                mailbox.uid_validity
            )),
            _ => ok("APPEND completed"),
        }
    }

    /// The selected mailbox when it may be changed, else the refusal.
    fn writable(&self) -> Result<String, Reply> {
        match self.selected() {
            None => Err(bad("no mailbox selected")),
            Some((_, true)) => Err(no("[READ-ONLY] mailbox is read-only")),
            Some((name, false)) => Ok(name),
        }
    }

    fn selected(&self) -> Option<(String, bool)> {
        self.view.as_ref().map(|v| (v.mailbox.clone(), v.read_only))
    }

    async fn fetch(&mut self, args: &[Token], uid: bool) -> io::Result<Reply> {
        let Some((mailbox_name, read_only)) = self.selected() else {
            return Ok(bad("no mailbox selected"));
        };
        let (set, items, modifiers) = match args {
            [set, items] => (set, items, None),
            [set, items, Token::List(modifiers)] => (set, items, Some(modifiers)),
            _ => return Ok(bad("FETCH needs a sequence set and items")),
        };
        let Some(set) = set.as_text().as_deref().and_then(SequenceSet::parse) else {
            return Ok(bad("invalid sequence set"));
        };
        let mut items = match command::parse_fetch_items(items) {
            Ok(items) => items,
            Err(err) => return Ok(bad(err)),
        };
        let caps = self.shared.config.capabilities;
        let mut changed_since = None;
        let mut vanished = false;
        if let Some(modifiers) = modifiers {
            let mut iter = modifiers.iter();
            while let Some(modifier) = iter.next() {
                match modifier
                    .as_text()
                    .unwrap_or_default()
                    .to_ascii_uppercase()
                    .as_str()
                {
                    "CHANGEDSINCE" if caps.condstore => {
                        let Some(value) = iter.next().and_then(Token::as_text) else {
                            return Ok(bad("CHANGEDSINCE needs a value"));
                        };
                        let Ok(value) = value.parse::<u64>() else {
                            return Ok(bad("invalid CHANGEDSINCE value"));
                        };
                        changed_since = Some(value);
                    }
                    "VANISHED" if uid && self.qresync => vanished = true,
                    other => return Ok(bad(format!("unsupported FETCH modifier {other}"))),
                }
            }
        }
        if vanished && changed_since.is_none() {
            return Ok(bad("VANISHED needs CHANGEDSINCE"));
        }
        if items.contains(&FetchItem::ModSeq) && !caps.condstore {
            return Ok(bad("MODSEQ is not supported"));
        }
        if changed_since.is_some() || items.contains(&FetchItem::ModSeq) {
            self.condstore = true;
        }
        if uid && !items.contains(&FetchItem::Uid) {
            items.insert(0, FetchItem::Uid);
        }
        let marks_seen = !read_only
            && items.iter().any(|i| {
                matches!(
                    i,
                    FetchItem::Body { peek: false, .. }
                        | FetchItem::Rfc822(Section::Full | Section::Text)
                )
            });
        if marks_seen && !items.contains(&FetchItem::Flags) {
            items.push(FetchItem::Flags);
        }
        if self.condstore
            && (items.contains(&FetchItem::Flags) || changed_since.is_some())
            && !items.contains(&FetchItem::ModSeq)
        {
            items.push(FetchItem::ModSeq);
        }

        let targets = self
            .view
            .as_ref()
            .map(|v| v.matching(&set, uid))
            .unwrap_or_default();
        let control = &self.shared.control;
        if uid
            && control
                .max_fetch_uids()
                .is_some_and(|max| targets.len() > max)
        {
            return Ok(bad("command line too long"));
        }
        let wants_content = items
            .iter()
            .any(|i| matches!(i, FetchItem::Body { .. } | FetchItem::Rfc822(_)));
        if wants_content
            && targets
                .iter()
                .any(|t| control.message_fault(t.1) == Some(MessageFault::Refuse))
        {
            return Ok(no(
                "[SERVERBUG] Internal error occurred. Refer to server log for more information.",
            ));
        }
        if marks_seen {
            let uids: Vec<u32> = targets.iter().map(|t| t.1).collect();
            self.shared.store.store_flags(
                &mailbox_name,
                &uids,
                FlagUpdate::Add,
                &["\\Seen".to_owned()],
                None,
            );
        }
        let Some(mailbox) = self.shared.store.mailbox(&mailbox_name) else {
            return Ok(no("mailbox vanished"));
        };

        let mut out = Vec::new();
        if let Some(since) = changed_since.filter(|_| vanished) {
            // `*` covers every UID ever assigned, expunged ones included.
            let max = mailbox.uid_next.saturating_sub(1);
            let mut gone: Vec<u32> = mailbox
                .expunged
                .iter()
                .filter(|(u, m)| *m > since && set.contains(*u, max))
                .map(|(u, _)| *u)
                .collect();
            gone.sort_unstable();
            gone.dedup();
            if !gone.is_empty() {
                out.extend_from_slice(
                    format!("* VANISHED (EARLIER) {}\r\n", number_set(&gone)).as_bytes(),
                );
            }
        }
        for (seq, message_uid) in targets {
            let Some(message) = mailbox.message(message_uid) else {
                continue; // expunged by another session, not yet reported
            };
            if changed_since.is_some_and(|since| message.modseq <= since) {
                continue;
            }
            let fault = self
                .shared
                .control
                .message_fault(message_uid)
                .filter(|_| wants_content);
            if fault == Some(MessageFault::Withhold) {
                continue;
            }
            if fault == Some(MessageFault::Garbage) {
                out.extend_from_slice(
                    format!("* {seq} FETCH (UID {message_uid} X-BROKEN ((( \"\r\n").as_bytes(),
                );
                continue;
            }
            out.extend_from_slice(format!("* {seq} FETCH (").as_bytes());
            if fault == Some(MessageFault::Malformed) {
                // Raw Latin-1 in a quoted string and an empty list.
                out.extend_from_slice(b"X-ODD (\"Gr\xfc\xdfe\" ()) ");
            }
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                push_fetch_item(&mut out, *item, message);
            }
            out.extend_from_slice(b")\r\n");
            if items.contains(&FetchItem::Flags) {
                let (flags, modseq) = (message.flags.clone(), message.modseq);
                self.update_view(message_uid, &flags, modseq);
            }
        }
        self.send(&out).await?;
        let verb = if uid { "UID FETCH" } else { "FETCH" };
        Ok(ok(format!("{verb} completed")))
    }

    async fn search(&mut self, args: &[Token], uid: bool) -> io::Result<Reply> {
        let Some((mailbox_name, _)) = self.selected() else {
            return Ok(bad("no mailbox selected"));
        };
        let Some(mailbox) = self.shared.store.mailbox(&mailbox_name) else {
            return Ok(no("mailbox vanished"));
        };
        let criteria = match parse_search(args) {
            Ok(criteria) => criteria,
            Err(err) => return Ok(bad(err)),
        };
        let Some(view) = self.view.as_ref() else {
            return Ok(bad("no mailbox selected"));
        };
        let max_uid = view.max_uid();
        let max_seq = u32::try_from(view.messages.len()).unwrap_or(u32::MAX);
        let mut hits = Vec::new();
        for (index, seen) in view.messages.iter().enumerate() {
            let Some(message) = mailbox.message(seen.uid) else {
                continue;
            };
            let seq = seq_number(index);
            let matches = criteria
                .iter()
                .all(|criterion| criterion.matches(message, seq, max_uid, max_seq));
            if matches {
                hits.push(if uid { message.uid } else { seq });
            }
        }
        let mut line = String::from("* SEARCH");
        for hit in hits {
            let _ = write!(line, " {hit}");
        }
        line.push_str("\r\n");
        self.send(line.as_bytes()).await?;
        let verb = if uid { "UID SEARCH" } else { "SEARCH" };
        Ok(ok(format!("{verb} completed")))
    }

    /// `STORE` in the writable selected mailbox `mailbox_name`.
    async fn store(
        &mut self,
        mailbox_name: String,
        args: &[Token],
        uid: bool,
    ) -> io::Result<Reply> {
        let (set, unchanged_since, item, flags) = match args {
            [set, item, flags] => (set, None, item, flags),
            [set, Token::List(modifier), item, flags] => {
                let text: Vec<String> = modifier.iter().filter_map(Token::as_text).collect();
                match (
                    text.first(),
                    text.get(1).and_then(|v| v.parse::<u64>().ok()),
                ) {
                    (Some(key), Some(value))
                        if key.eq_ignore_ascii_case("UNCHANGEDSINCE")
                            && self.shared.config.capabilities.condstore =>
                    {
                        (set, Some(value), item, flags)
                    }
                    _ => return Ok(bad("unsupported STORE modifier")),
                }
            }
            _ => return Ok(bad("STORE needs a set, an item and flags")),
        };
        let Some(set) = set.as_text().as_deref().and_then(SequenceSet::parse) else {
            return Ok(bad("invalid sequence set"));
        };
        let item = item.as_text().unwrap_or_default().to_ascii_uppercase();
        let (base, silent) = match item.strip_suffix(".SILENT") {
            Some(base) => (base, true),
            None => (item.as_str(), false),
        };
        let update = match base {
            "FLAGS" => FlagUpdate::Replace,
            "+FLAGS" => FlagUpdate::Add,
            "-FLAGS" => FlagUpdate::Remove,
            _ => return Ok(bad(format!("unsupported STORE item {item}"))),
        };
        let flags: Vec<String> = match flags {
            Token::List(items) => items.iter().filter_map(Token::as_text).collect(),
            Token::Atom(atom) => vec![atom.clone()],
            Token::Str(_) => return Ok(bad("flags must be atoms")),
        };
        if unchanged_since.is_some() {
            self.condstore = true;
        }
        let targets = self
            .view
            .as_ref()
            .map(|v| v.matching(&set, uid))
            .unwrap_or_default();
        let uids: Vec<u32> = targets.iter().map(|t| t.1).collect();
        let outcome =
            self.shared
                .store
                .store_flags(&mailbox_name, &uids, update, &flags, unchanged_since);
        let refused = outcome.refused;
        let mut out = Vec::new();
        for StoredFlags {
            uid: message_uid,
            flags,
            modseq,
        } in &outcome.updated
        {
            self.update_view(*message_uid, flags, *modseq);
            if silent {
                continue;
            }
            let Some(seq) = self.view.as_ref().and_then(|v| v.seq_of(*message_uid)) else {
                continue;
            };
            let mut line = format!("* {seq} FETCH (");
            if uid || self.qresync {
                let _ = write!(line, "UID {message_uid} ");
            }
            let _ = write!(line, "FLAGS ({})", flags.join(" "));
            if self.condstore {
                let _ = write!(line, " MODSEQ ({modseq})");
            }
            line.push_str(")\r\n");
            out.extend_from_slice(line.as_bytes());
        }
        self.send(&out).await?;
        if refused.is_empty() {
            return Ok(ok("STORE completed"));
        }
        let ids: Vec<u32> = if uid {
            refused
        } else {
            refused
                .iter()
                .filter_map(|u| self.view.as_ref().and_then(|v| v.seq_of(*u)))
                .collect()
        };
        Ok(ok(format!(
            "[MODIFIED {}] conditional STORE failed",
            number_set(&ids)
        )))
    }

    async fn copy(&mut self, args: &[Token], uid: bool, moving: bool) -> io::Result<Reply> {
        let Some((mailbox_name, read_only)) = self.selected() else {
            return Ok(bad("no mailbox selected"));
        };
        let [set, target] = args else {
            return Ok(bad("COPY / MOVE need a set and a mailbox"));
        };
        let (Some(set), Some(target)) = (
            set.as_text().as_deref().and_then(SequenceSet::parse),
            target.as_text(),
        ) else {
            return Ok(bad("invalid arguments"));
        };
        if moving && read_only {
            return Ok(no("[READ-ONLY] mailbox is read-only"));
        }
        let uids: Vec<u32> = self
            .view
            .as_ref()
            .map(|v| v.matching(&set, uid))
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.1)
            .collect();
        let Ok(mapping) = self
            .shared
            .store
            .copy_messages(&mailbox_name, &uids, &target)
        else {
            return Ok(no("[TRYCREATE] no such mailbox"));
        };
        let code = match self.shared.store.mailbox(&target) {
            Some(mailbox) if self.shared.config.capabilities.uidplus && !mapping.is_empty() => {
                let sources: Vec<u32> = mapping.iter().map(|m| m.0).collect();
                let targets: Vec<u32> = mapping.iter().map(|m| m.1).collect();
                format!(
                    "[COPYUID {} {} {}] ",
                    mailbox.uid_validity,
                    number_list(&sources),
                    number_list(&targets)
                )
            }
            _ => String::new(),
        };
        if !moving {
            return Ok(ok(format!("{code}COPY completed")));
        }
        self.send(format!("* OK {code}moved\r\n").as_bytes())
            .await?;
        let moved: Vec<u32> = mapping.iter().map(|m| m.0).collect();
        self.shared.store.expunge_uids(&mailbox_name, &moved);
        let out = self.sync_view(true);
        self.send(&out).await?;
        Ok(ok("MOVE completed"))
    }

    /// `EXPUNGE` / `UID EXPUNGE` in the writable selected mailbox
    /// `mailbox_name`.
    async fn expunge(
        &mut self,
        mailbox_name: String,
        args: &[Token],
        uid: bool,
    ) -> io::Result<Reply> {
        let set = if uid {
            match args
                .first()
                .and_then(Token::as_text)
                .as_deref()
                .and_then(SequenceSet::parse)
            {
                Some(set) => Some(set),
                None => return Ok(bad("UID EXPUNGE needs a UID set")),
            }
        } else {
            None
        };
        let max = self.view.as_ref().map_or(0, View::max_uid);
        self.shared.store.expunge_deleted(&mailbox_name, |u| {
            set.as_ref().is_none_or(|s| s.contains(u, max))
        });
        let out = self.sync_view(true);
        self.send(&out).await?;
        Ok(ok("EXPUNGE completed"))
    }

    async fn idle(&mut self, tag: &str) -> io::Result<Flow> {
        /// Why the IDLE loop ended.
        enum End {
            Done,
            Garbage,
            Closed,
        }

        if !self.shared.config.capabilities.idle {
            self.tagged(tag, "BAD", "unsupported command").await?;
            return Ok(Flow::Continue);
        }
        self.send(b"+ idling\r\n").await?;
        let out = self.sync_view(true);
        self.send(&out).await?;

        let control = Arc::clone(&self.shared.control);
        let kill = control.idle_kill.notified();
        tokio::pin!(kill);
        kill.as_mut().enable();
        let _guard = IdleGuard::enter(Arc::clone(&control));

        let mut line = Vec::new();
        let end = loop {
            tokio::select! {
                () = &mut kill => break End::Closed,
                changed = self.changes.changed() => {
                    if changed.is_ok() {
                        let out = self.sync_view(true);
                        self.send(&out).await?;
                    }
                }
                read = self.stream.read_until(b'\n', &mut line) => {
                    if read? == 0 {
                        break End::Closed;
                    }
                    if line.ends_with(b"\n") {
                        let text = String::from_utf8_lossy(&line);
                        break if text.trim().eq_ignore_ascii_case("DONE") {
                            End::Done
                        } else {
                            End::Garbage
                        };
                    }
                }
            }
        };
        match end {
            End::Closed => Ok(Flow::Close),
            End::Done => {
                self.tagged(tag, "OK", "IDLE terminated").await?;
                Ok(Flow::Continue)
            }
            End::Garbage => {
                self.tagged(tag, "BAD", "expected DONE").await?;
                Ok(Flow::Continue)
            }
        }
    }
}

/// Counts a session as idling while alive.
struct IdleGuard(Arc<Control>);

impl IdleGuard {
    fn enter(control: Arc<Control>) -> Self {
        control.set_idling(1);
        Self(control)
    }
}

impl Drop for IdleGuard {
    fn drop(&mut self) {
        self.0.set_idling(-1);
    }
}

/// An unsolicited or `STORE` flag update: `* seq FETCH (UID u FLAGS (...) MODSEQ (m))`.
fn flag_fetch(seq: u32, message: &StoredMessage, with_uid: bool, with_modseq: bool) -> String {
    let mut line = format!("* {seq} FETCH (");
    if with_uid {
        let _ = write!(line, "UID {} ", message.uid);
    }
    let _ = write!(line, "FLAGS ({})", message.flags.join(" "));
    if with_modseq {
        let _ = write!(line, " MODSEQ ({})", message.modseq);
    }
    line.push_str(")\r\n");
    line
}

/// `user=<user>^Aauth=Bearer <token>^A^A` (Google's XOAUTH2).
fn parse_xoauth2(decoded: &[u8]) -> Option<(String, String)> {
    let text = std::str::from_utf8(decoded).ok()?;
    let mut user = None;
    let mut token = None;
    for part in text.split('\x01') {
        if let Some(value) = part.strip_prefix("user=") {
            user = Some(value.to_owned());
        } else if let Some(value) = part.strip_prefix("auth=Bearer ") {
            token = Some(value.to_owned());
        }
    }
    Some((user?, token?))
}

/// `n,a=<user>,^Ahost=...^Aport=...^Aauth=Bearer <token>^A^A` (RFC 7628).
/// Without `a=`, the configured user is assumed.
fn parse_oauthbearer(decoded: &[u8], default_user: &str) -> Option<(String, String)> {
    let text = std::str::from_utf8(decoded).ok()?;
    let mut parts = text.split('\x01');
    let gs2 = parts.next()?;
    if !gs2.starts_with("n,") {
        return None;
    }
    let user = gs2
        .split(',')
        .find_map(|p| p.strip_prefix("a="))
        .map_or_else(
            || default_user.to_owned(),
            |u| u.replace("=2C", ",").replace("=3D", "="),
        );
    let token = parts.find_map(|p| p.strip_prefix("auth=Bearer "))?;
    Some((user, token.to_owned()))
}

enum Criterion {
    All,
    /// A flag that must (`true`) or must not (`false`) be set.
    Flag(&'static str, bool),
    Uid(SequenceSet),
    Seq(SequenceSet),
    /// `HEADER <field> <string>`: the field contains the string (ASCII
    /// case-insensitive, RFC 3501); also `SUBJECT`, `FROM`, `TO`, `CC`.
    Header(String, String),
    /// `TEXT <string>`: header or body contains it.
    Text(String),
    /// `BODY <string>`: the body contains it.
    Body(String),
    /// `OR <key> <key>`.
    Or(Box<Criterion>, Box<Criterion>),
    /// `NOT <key>`.
    Not(Box<Criterion>),
    /// A parenthesized list: all of them.
    And(Vec<Criterion>),
}

impl Criterion {
    fn matches(&self, message: &StoredMessage, seq: u32, max_uid: u32, max_seq: u32) -> bool {
        match self {
            Criterion::All => true,
            Criterion::Flag(flag, wanted) => message.has_flag(flag) == *wanted,
            Criterion::Uid(set) => set.contains(message.uid, max_uid),
            Criterion::Seq(set) => set.contains(seq, max_seq),
            Criterion::Header(field, value) => header_contains(&message.raw, field, value),
            Criterion::Text(value) => contains_ci(&message.raw, value),
            Criterion::Body(value) => {
                let split = message
                    .raw
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map_or(message.raw.len(), |p| p + 4);
                contains_ci(&message.raw[split..], value)
            }
            Criterion::Or(a, b) => {
                a.matches(message, seq, max_uid, max_seq)
                    || b.matches(message, seq, max_uid, max_seq)
            }
            Criterion::Not(a) => !a.matches(message, seq, max_uid, max_seq),
            Criterion::And(all) => all
                .iter()
                .all(|c| c.matches(message, seq, max_uid, max_seq)),
        }
    }
}

/// Whether `raw` contains `value` (ASCII case-insensitive).
fn contains_ci(raw: &[u8], value: &str) -> bool {
    String::from_utf8_lossy(raw)
        .to_lowercase()
        .contains(&value.to_lowercase())
}

/// Whether a header field of `raw` contains `value` (ASCII case-insensitive;
/// folded lines are unfolded). An empty `value` matches any message that
/// has the field.
fn header_contains(raw: &[u8], field: &str, value: &str) -> bool {
    let end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..end])
        .replace("\r\n ", " ")
        .replace("\r\n\t", " ");
    let value = value.to_ascii_lowercase();
    head.split("\r\n").any(|line| {
        line.split_once(':').is_some_and(|(name, body)| {
            name.trim().eq_ignore_ascii_case(field) && body.to_ascii_lowercase().contains(&value)
        })
    })
}

fn parse_search(args: &[Token]) -> Result<Vec<Criterion>, String> {
    let mut criteria = Vec::new();
    let mut iter = args.iter().peekable();
    // `CHARSET <name>` (UTF-8 and US-ASCII are what the mock knows).
    if iter
        .peek()
        .and_then(|t| t.as_text())
        .is_some_and(|t| t.eq_ignore_ascii_case("CHARSET"))
    {
        iter.next();
        let charset = iter.next().and_then(Token::as_text).unwrap_or_default();
        if !charset.eq_ignore_ascii_case("UTF-8") && !charset.eq_ignore_ascii_case("US-ASCII") {
            return Err(format!(
                "[BADCHARSET (UTF-8)] unsupported charset {charset}"
            ));
        }
    }
    while iter.peek().is_some() {
        criteria.push(parse_key(&mut iter)?);
    }
    if criteria.is_empty() {
        return Err("search criteria required".to_owned());
    }
    Ok(criteria)
}

fn parse_key<'a>(iter: &mut impl Iterator<Item = &'a Token>) -> Result<Criterion, String> {
    let token = iter.next().ok_or("search key expected")?;
    if let Token::List(items) = token {
        let mut inner = items.iter();
        let mut all = Vec::new();
        while let Some(first) = inner.next() {
            let mut chained = std::iter::once(first).chain(&mut inner);
            all.push(parse_key(&mut chained)?);
        }
        return Ok(Criterion::And(all));
    }
    let key = token
        .as_text()
        .ok_or("unsupported search key")?
        .to_ascii_uppercase();
    let mut text = |what: &str| -> Result<String, String> {
        iter.next()
            .and_then(Token::as_text)
            .ok_or_else(|| format!("{what} needs a string"))
    };
    Ok(match key.as_str() {
        "ALL" => Criterion::All,
        "SEEN" => Criterion::Flag("\\Seen", true),
        "UNSEEN" => Criterion::Flag("\\Seen", false),
        "DELETED" => Criterion::Flag("\\Deleted", true),
        "UNDELETED" => Criterion::Flag("\\Deleted", false),
        "FLAGGED" => Criterion::Flag("\\Flagged", true),
        "UNFLAGGED" => Criterion::Flag("\\Flagged", false),
        "ANSWERED" => Criterion::Flag("\\Answered", true),
        "UNANSWERED" => Criterion::Flag("\\Answered", false),
        "UID" => Criterion::Uid(SequenceSet::parse(&text("UID")?).ok_or("invalid UID set")?),
        "HEADER" => {
            let field = text("HEADER")?;
            Criterion::Header(field, text("HEADER")?)
        }
        "SUBJECT" | "FROM" | "TO" | "CC" => {
            Criterion::Header(key.to_ascii_lowercase(), text(&key)?)
        }
        "TEXT" => Criterion::Text(text("TEXT")?),
        "BODY" => Criterion::Body(text("BODY")?),
        "OR" => {
            let a = parse_key(iter)?;
            let b = parse_key(iter)?;
            Criterion::Or(Box::new(a), Box::new(b))
        }
        "NOT" => Criterion::Not(Box::new(parse_key(iter)?)),
        _ => match SequenceSet::parse(&key) {
            Some(set) => Criterion::Seq(set),
            None => return Err(format!("unsupported search key {key}")),
        },
    })
}

fn push_fetch_item(out: &mut Vec<u8>, item: FetchItem, message: &StoredMessage) {
    let raw = message.raw.as_slice();
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map_or(raw.len(), |p| p + 4);
    let section_bytes = |section: Section| match section {
        Section::Full => raw,
        Section::Header => &raw[..split],
        Section::Text => &raw[split..],
    };
    match item {
        FetchItem::Uid => out.extend_from_slice(format!("UID {}", message.uid).as_bytes()),
        FetchItem::Flags => {
            out.extend_from_slice(format!("FLAGS ({})", message.flags.join(" ")).as_bytes());
        }
        FetchItem::InternalDate => {
            out.extend_from_slice(format!("INTERNALDATE \"{}\"", message.internal_date).as_bytes());
        }
        FetchItem::Size => out.extend_from_slice(format!("RFC822.SIZE {}", raw.len()).as_bytes()),
        FetchItem::Envelope => {
            out.extend_from_slice(b"ENVELOPE ");
            push_envelope(out, raw);
        }
        FetchItem::ModSeq => {
            out.extend_from_slice(format!("MODSEQ ({})", message.modseq).as_bytes());
        }
        FetchItem::Body { section, .. } => {
            let name = match section {
                Section::Full => "BODY[] ",
                Section::Header => "BODY[HEADER] ",
                Section::Text => "BODY[TEXT] ",
            };
            out.extend_from_slice(name.as_bytes());
            push_literal(out, section_bytes(section));
        }
        FetchItem::Rfc822(section) => {
            let name = match section {
                Section::Full => "RFC822 ",
                Section::Header => "RFC822.HEADER ",
                Section::Text => "RFC822.TEXT ",
            };
            out.extend_from_slice(name.as_bytes());
            push_literal(out, section_bytes(section));
        }
    }
}

fn special_use(role: FolderRole) -> Option<&'static str> {
    match role {
        FolderRole::Sent => Some("\\Sent"),
        FolderRole::Drafts => Some("\\Drafts"),
        FolderRole::Archive => Some("\\Archive"),
        FolderRole::Junk => Some("\\Junk"),
        FolderRole::Trash => Some("\\Trash"),
        FolderRole::Inbox | FolderRole::Other => None,
    }
}

/// IMAP LIST wildcard matching: `*` matches anything, `%` anything but `/`.
fn wildcard_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match p.split_first() {
            None => n.is_empty(),
            Some(('*', rest)) => (0..=n.len()).any(|i| go(rest, &n[i..])),
            Some(('%', rest)) => (0..=n.len())
                .take_while(|&i| i == 0 || n[i - 1] != '/')
                .any(|i| go(rest, &n[i..])),
            Some((c, rest)) => n.first() == Some(c) && go(rest, &n[1..]),
        }
    }
    let upper_inbox = |s: &str| {
        if s.len() >= 5
            && s[..5].eq_ignore_ascii_case("INBOX")
            && (s.len() == 5 || s.as_bytes()[5] == b'/')
        {
            format!("INBOX{}", &s[5..])
        } else {
            s.to_owned()
        }
    };
    let pattern: Vec<char> = upper_inbox(pattern).chars().collect();
    let name: Vec<char> = upper_inbox(name).chars().collect();
    go(&pattern, &name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcards() {
        assert!(wildcard_match("*", "INBOX"));
        assert!(wildcard_match("*", "Work/Reports"));
        assert!(wildcard_match("%", "Sent"));
        assert!(!wildcard_match("%", "Work/Reports"));
        assert!(wildcard_match("Work/%", "Work/Reports"));
        assert!(wildcard_match("inbox", "INBOX"));
        assert!(!wildcard_match("Sent", "Sent Items"));
    }

    #[test]
    fn search_parsing() {
        let tokens = command::tokenize(b"UID 5:* UNSEEN").unwrap();
        assert_eq!(parse_search(&tokens).unwrap().len(), 2);
        let tokens = command::tokenize(b"1:3 DELETED UNFLAGGED").unwrap();
        assert_eq!(parse_search(&tokens).unwrap().len(), 3);
        assert!(parse_search(&[]).is_err());
        assert!(parse_search(&command::tokenize(b"KEYWORD x").unwrap()).is_err());
        // Text keys, OR / NOT and lists (the server search fallback, M2c).
        let tokens =
            command::tokenize(b"CHARSET UTF-8 OR TEXT \"platypus\" (FROM x NOT BODY y)").unwrap();
        assert_eq!(parse_search(&tokens).unwrap().len(), 1);
        assert!(parse_search(&command::tokenize(b"CHARSET KOI8-R TEXT x").unwrap()).is_err());
        let message = StoredMessage {
            uid: 7,
            flags: Vec::new(),
            internal_date: String::new(),
            modseq: 1,
            raw: b"From: Nature <n@x>\r\nSubject: Weekly\r\n\r\nThe platypus glows.\r\n"
                .to_vec()
                .into(),
        };
        let matches = |text: &[u8]| {
            parse_search(&command::tokenize(text).unwrap())
                .unwrap()
                .iter()
                .all(|c| c.matches(&message, 1, 7, 1))
        };
        assert!(matches(b"TEXT PLATYPUS"));
        assert!(matches(b"BODY platypus FROM nature"));
        assert!(!matches(b"BODY weekly"));
        assert!(matches(b"OR SUBJECT nothing SUBJECT weekly"));
        assert!(!matches(b"NOT TEXT platypus"));
        assert!(matches(b"(SUBJECT weekly TEXT glows)"));
    }

    #[test]
    fn oauth_payloads() {
        assert_eq!(
            parse_xoauth2(b"user=sam@x\x01auth=Bearer tok\x01\x01"),
            Some(("sam@x".to_owned(), "tok".to_owned()))
        );
        assert_eq!(parse_xoauth2(b"auth=Bearer tok\x01\x01"), None);
        assert_eq!(
            parse_oauthbearer(
                b"n,a=sam@x,\x01host=h\x01port=1\x01auth=Bearer tok\x01\x01",
                "d"
            ),
            Some(("sam@x".to_owned(), "tok".to_owned()))
        );
        assert_eq!(
            parse_oauthbearer(b"n,,\x01auth=Bearer tok\x01\x01", "d"),
            Some(("d".to_owned(), "tok".to_owned()))
        );
        assert_eq!(
            parse_oauthbearer(b"y,,\x01auth=Bearer t\x01\x01", "d"),
            None
        );
    }
}
