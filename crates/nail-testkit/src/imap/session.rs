//! One client connection of the mock IMAP server.

use std::io;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nail_api::FolderRole;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::watch;

use super::command::{self, FetchItem, Section, SequenceSet, Token};
use super::encode::{push_envelope, push_literal};
use super::{Control, MockImapConfig, MockTls};
use crate::mailstore::{MailStore, StoredMessage};
use crate::tls::TestTls;

/// Upper bound for one command line (literals included), to stop runaway clients.
const MAX_COMMAND_LEN: usize = 1 << 20;
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

enum State {
    NotAuthenticated,
    Authenticated,
    Selected { mailbox: String, read_only: bool },
}

enum Flow {
    Continue,
    Close,
    StartTls,
}

struct Session {
    shared: Shared,
    state: State,
    tls_active: bool,
    stream: BufReader<Box<dyn Io>>,
}

/// Serves one connection until logout, disconnect or server shutdown.
pub(crate) async fn run(stream: TcpStream, shared: Shared, mut shutdown: watch::Receiver<bool>) {
    tokio::select! {
        result = serve(stream, shared) => {
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
    let mut session = Session {
        shared,
        state: State::NotAuthenticated,
        tls_active: implicit,
        stream: BufReader::new(stream),
    };
    session
        .send(b"* OK Nail mock IMAP4rev1 server ready\r\n")
        .await?;

    loop {
        let Some(line) = session.read_command().await? else {
            return Ok(());
        };
        match session.handle(&line).await? {
            Flow::Continue => {}
            Flow::Close => return Ok(()),
            Flow::StartTls => {
                let Some(tls) = session.shared.tls.clone() else {
                    return Ok(());
                };
                // Anything the client pipelined after STARTTLS is discarded
                // (as required, to prevent command injection).
                let plain = std::mem::replace(
                    &mut session.stream,
                    BufReader::new(Box::new(tokio::io::duplex(1).0)),
                );
                let upgraded = tls.acceptor.accept(plain.into_inner()).await?;
                session.stream = BufReader::new(Box::new(upgraded));
                session.tls_active = true;
            }
        }
    }
}

impl Session {
    async fn send(&mut self, bytes: &[u8]) -> io::Result<()> {
        let stream = self.stream.get_mut();
        stream.write_all(bytes).await?;
        stream.flush().await
    }

    async fn tagged(&mut self, tag: &str, status: &str, text: &str) -> io::Result<()> {
        self.send(format!("{tag} {status} {text}\r\n").as_bytes())
            .await
    }

    /// Reads one command including any literals; `None` at EOF.
    async fn read_command(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut command = Vec::new();
        loop {
            let mut line = Vec::new();
            let n = (&mut self.stream)
                .take((MAX_COMMAND_LEN - command.len().min(MAX_COMMAND_LEN)) as u64)
                .read_until(b'\n', &mut line)
                .await?;
            if n == 0 {
                return Ok(None);
            }
            if !line.ends_with(b"\n") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "command too long",
                ));
            }
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            let literal = command::trailing_literal(&line);
            command.extend_from_slice(&line);
            let Some((len, synchronizing)) = literal else {
                return Ok(Some(command));
            };
            if command.len() + len > MAX_COMMAND_LEN {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "literal too long",
                ));
            }
            if synchronizing {
                self.send(b"+ Ready for literal data\r\n").await?;
            }
            let mut data = vec![0; len];
            self.stream.read_exact(&mut data).await?;
            command.extend_from_slice(b"\r\n");
            command.extend_from_slice(&data);
        }
    }

    fn capabilities(&self) -> String {
        let mut caps = String::from("IMAP4rev1 LITERAL+ SPECIAL-USE");
        if self.shared.config.tls == MockTls::StartTls && !self.tls_active {
            caps.push_str(" STARTTLS LOGINDISABLED");
        } else {
            caps.push_str(" AUTH=PLAIN");
        }
        caps
    }

    async fn handle(&mut self, line: &[u8]) -> io::Result<Flow> {
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
            if args.is_empty() {
                self.tagged(&tag, "BAD", "UID needs a command").await?;
                return Ok(Flow::Continue);
            }
            let Token::Atom(sub) = args.remove(0) else {
                self.tagged(&tag, "BAD", "UID needs a command").await?;
                return Ok(Flow::Continue);
            };
            uid = true;
            name = sub.to_ascii_uppercase();
        }
        let full_name = if uid {
            format!("UID {name}")
        } else {
            name.clone()
        };
        if self.shared.control.should_disconnect(&full_name) {
            tracing::debug!(command = %full_name, "mock IMAP: injected disconnect");
            return Ok(Flow::Close);
        }
        if self.shared.control.should_fail(&full_name) {
            self.tagged(&tag, "NO", "[SERVERBUG] injected failure")
                .await?;
            return Ok(Flow::Continue);
        }

        match name.as_str() {
            "CAPABILITY" => {
                let caps = self.capabilities();
                self.send(format!("* CAPABILITY {caps}\r\n").as_bytes())
                    .await?;
                self.tagged(&tag, "OK", "CAPABILITY completed").await?;
            }
            "NOOP" => self.tagged(&tag, "OK", "NOOP completed").await?,
            "LOGOUT" => {
                self.send(b"* BYE Nail mock server logging out\r\n").await?;
                self.tagged(&tag, "OK", "LOGOUT completed").await?;
                return Ok(Flow::Close);
            }
            "STARTTLS" => return self.starttls(&tag).await,
            "LOGIN" => self.login(&tag, &args).await?,
            "AUTHENTICATE" => return self.authenticate(&tag, &args).await,
            _ if matches!(self.state, State::NotAuthenticated) => {
                self.tagged(&tag, "BAD", "command not allowed before authentication")
                    .await?;
            }
            "LIST" => self.list(&tag, &args).await?,
            "SELECT" => self.select(&tag, &args, false).await?,
            "EXAMINE" => self.select(&tag, &args, true).await?,
            "CLOSE" | "UNSELECT" if matches!(self.state, State::Selected { .. }) => {
                self.state = State::Authenticated;
                self.tagged(&tag, "OK", "mailbox closed").await?;
            }
            "FETCH" => self.fetch(&tag, &args, uid).await?,
            "SEARCH" => self.search(&tag, &args, uid).await?,
            _ => self.tagged(&tag, "BAD", "unsupported command").await?,
        }
        Ok(Flow::Continue)
    }

    async fn starttls(&mut self, tag: &str) -> io::Result<Flow> {
        if self.shared.config.tls != MockTls::StartTls || self.tls_active {
            self.tagged(tag, "BAD", "STARTTLS not available").await?;
            return Ok(Flow::Continue);
        }
        self.tagged(tag, "OK", "begin TLS negotiation now").await?;
        Ok(Flow::StartTls)
    }

    fn login_disabled(&self) -> bool {
        self.shared.config.tls == MockTls::StartTls && !self.tls_active
    }

    fn check_credentials(&self, user: &[u8], pass: &[u8]) -> bool {
        user == self.shared.config.username.as_bytes()
            && pass == self.shared.config.password.as_bytes()
    }

    async fn login(&mut self, tag: &str, args: &[Token]) -> io::Result<()> {
        if !matches!(self.state, State::NotAuthenticated) {
            return self.tagged(tag, "BAD", "already authenticated").await;
        }
        if self.login_disabled() {
            return self
                .tagged(
                    tag,
                    "NO",
                    "[PRIVACYREQUIRED] LOGIN disabled before STARTTLS",
                )
                .await;
        }
        let [user, pass] = args else {
            return self
                .tagged(tag, "BAD", "LOGIN needs user name and password")
                .await;
        };
        let (Some(user), Some(pass)) = (user.as_text(), pass.as_text()) else {
            return self
                .tagged(tag, "BAD", "LOGIN needs user name and password")
                .await;
        };
        if self.check_credentials(user.as_bytes(), pass.as_bytes()) {
            self.state = State::Authenticated;
            self.tagged(tag, "OK", "LOGIN completed").await
        } else {
            self.tagged(tag, "NO", "[AUTHENTICATIONFAILED] invalid credentials")
                .await
        }
    }

    async fn authenticate(&mut self, tag: &str, args: &[Token]) -> io::Result<Flow> {
        if !matches!(self.state, State::NotAuthenticated) {
            self.tagged(tag, "BAD", "already authenticated").await?;
            return Ok(Flow::Continue);
        }
        let mechanism = args.first().and_then(Token::as_text).unwrap_or_default();
        if !mechanism.eq_ignore_ascii_case("PLAIN") {
            self.tagged(tag, "NO", "unsupported authentication mechanism")
                .await?;
            return Ok(Flow::Continue);
        }
        if self.login_disabled() {
            self.tagged(
                tag,
                "NO",
                "[PRIVACYREQUIRED] authentication disabled before STARTTLS",
            )
            .await?;
            return Ok(Flow::Continue);
        }
        let response = match args.get(1).and_then(Token::as_text) {
            Some(initial) => initial,
            None => {
                self.send(b"+ \r\n").await?;
                let Some(line) = self.read_command().await? else {
                    return Ok(Flow::Close);
                };
                String::from_utf8_lossy(&line).trim().to_owned()
            }
        };
        if response == "*" {
            self.tagged(tag, "BAD", "authentication cancelled").await?;
            return Ok(Flow::Continue);
        }
        let decoded = if response == "=" {
            Ok(Vec::new())
        } else {
            BASE64.decode(response.as_bytes())
        };
        let Ok(decoded) = decoded else {
            self.tagged(tag, "BAD", "invalid base64").await?;
            return Ok(Flow::Continue);
        };
        let mut parts = decoded.split(|&b| b == 0);
        let (Some(_authzid), Some(user), Some(pass), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            self.tagged(tag, "NO", "[AUTHENTICATIONFAILED] malformed PLAIN response")
                .await?;
            return Ok(Flow::Continue);
        };
        if self.check_credentials(user, pass) {
            self.state = State::Authenticated;
            self.tagged(tag, "OK", "AUTHENTICATE completed").await?;
        } else {
            self.tagged(tag, "NO", "[AUTHENTICATIONFAILED] invalid credentials")
                .await?;
        }
        Ok(Flow::Continue)
    }

    async fn list(&mut self, tag: &str, args: &[Token]) -> io::Result<()> {
        let [reference, pattern] = args else {
            return self
                .tagged(tag, "BAD", "LIST needs reference and pattern")
                .await;
        };
        let (Some(reference), Some(pattern)) = (reference.as_text(), pattern.as_text()) else {
            return self
                .tagged(tag, "BAD", "LIST needs reference and pattern")
                .await;
        };
        let mut out = Vec::new();
        if pattern.is_empty() {
            out.extend_from_slice(b"* LIST (\\Noselect) \"/\" \"\"\r\n");
        } else {
            let pattern = format!("{reference}{pattern}");
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
                attrs.extend(special_use(mailbox.role));
                out.extend_from_slice(format!("* LIST ({}) \"/\" ", attrs.join(" ")).as_bytes());
                super::encode::push_string(&mut out, mailbox.name.as_bytes());
                out.extend_from_slice(b"\r\n");
            }
        }
        self.send(&out).await?;
        self.tagged(tag, "OK", "LIST completed").await
    }

    async fn select(&mut self, tag: &str, args: &[Token], read_only: bool) -> io::Result<()> {
        let Some(name) = args.first().and_then(Token::as_text) else {
            return self.tagged(tag, "BAD", "mailbox name required").await;
        };
        let Some(mailbox) = self.shared.store.mailbox(&name) else {
            self.state = State::Authenticated;
            return self
                .tagged(tag, "NO", "[NONEXISTENT] no such mailbox")
                .await;
        };
        let permanent = if read_only {
            String::new()
        } else {
            format!("{SYSTEM_FLAGS} \\*")
        };
        let response = format!(
            "* FLAGS ({SYSTEM_FLAGS})\r\n* {} EXISTS\r\n* 0 RECENT\r\n\
             * OK [PERMANENTFLAGS ({permanent})] flags\r\n\
             * OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {}] predicted next UID\r\n",
            mailbox.messages.len(),
            mailbox.uid_validity,
            mailbox.uid_next
        );
        self.send(response.as_bytes()).await?;
        self.state = State::Selected {
            mailbox: mailbox.name,
            read_only,
        };
        let (code, verb) = if read_only {
            ("READ-ONLY", "EXAMINE")
        } else {
            ("READ-WRITE", "SELECT")
        };
        self.tagged(tag, "OK", &format!("[{code}] {verb} completed"))
            .await
    }

    fn selected(&self) -> Option<(String, bool)> {
        match &self.state {
            State::Selected { mailbox, read_only } => Some((mailbox.clone(), *read_only)),
            _ => None,
        }
    }

    async fn fetch(&mut self, tag: &str, args: &[Token], uid: bool) -> io::Result<()> {
        let Some((mailbox_name, read_only)) = self.selected() else {
            return self.tagged(tag, "BAD", "no mailbox selected").await;
        };
        let [set, items] = args else {
            return self
                .tagged(tag, "BAD", "FETCH needs a sequence set and items")
                .await;
        };
        let Some(set) = set.as_text().as_deref().and_then(SequenceSet::parse) else {
            return self.tagged(tag, "BAD", "invalid sequence set").await;
        };
        let mut items = match command::parse_fetch_items(items) {
            Ok(items) => items,
            Err(err) => return self.tagged(tag, "BAD", &err).await,
        };
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

        let selected: Vec<(usize, StoredMessage)> = self
            .shared
            .store
            .with_mailbox_mut(&mailbox_name, |mailbox| {
                let max = if uid {
                    mailbox.messages.last().map_or(0, |m| m.uid)
                } else {
                    u32::try_from(mailbox.messages.len()).unwrap_or(u32::MAX)
                };
                let mut out = Vec::new();
                for (index, message) in mailbox.messages.iter_mut().enumerate() {
                    let seq = u32::try_from(index + 1).unwrap_or(u32::MAX);
                    let key = if uid { message.uid } else { seq };
                    if !set.contains(key, max) {
                        continue;
                    }
                    if marks_seen
                        && !message
                            .flags
                            .iter()
                            .any(|f| f.eq_ignore_ascii_case("\\Seen"))
                    {
                        message.flags.push("\\Seen".to_owned());
                    }
                    out.push((index + 1, message.clone()));
                }
                out
            })
            .unwrap_or_default();

        for (seq, message) in selected {
            let mut out = format!("* {seq} FETCH (").into_bytes();
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b' ');
                }
                push_fetch_item(&mut out, *item, &message);
            }
            out.extend_from_slice(b")\r\n");
            self.send(&out).await?;
        }
        let verb = if uid { "UID FETCH" } else { "FETCH" };
        self.tagged(tag, "OK", &format!("{verb} completed")).await
    }

    async fn search(&mut self, tag: &str, args: &[Token], uid: bool) -> io::Result<()> {
        let Some((mailbox_name, _)) = self.selected() else {
            return self.tagged(tag, "BAD", "no mailbox selected").await;
        };
        let Some(mailbox) = self.shared.store.mailbox(&mailbox_name) else {
            return self.tagged(tag, "NO", "mailbox vanished").await;
        };
        let criteria = match parse_search(args) {
            Ok(criteria) => criteria,
            Err(err) => return self.tagged(tag, "BAD", &err).await,
        };
        let max_uid = mailbox.messages.last().map_or(0, |m| m.uid);
        let mut hits = Vec::new();
        for (index, message) in mailbox.messages.iter().enumerate() {
            let seen = message
                .flags
                .iter()
                .any(|f| f.eq_ignore_ascii_case("\\Seen"));
            let matches = criteria.iter().all(|criterion| match criterion {
                Criterion::All => true,
                Criterion::Seen => seen,
                Criterion::Unseen => !seen,
                Criterion::Uid(set) => set.contains(message.uid, max_uid),
            });
            if matches {
                hits.push(if uid {
                    message.uid
                } else {
                    u32::try_from(index + 1).unwrap_or(u32::MAX)
                });
            }
        }
        let mut line = String::from("* SEARCH");
        for hit in hits {
            line.push_str(&format!(" {hit}"));
        }
        line.push_str("\r\n");
        self.send(line.as_bytes()).await?;
        let verb = if uid { "UID SEARCH" } else { "SEARCH" };
        self.tagged(tag, "OK", &format!("{verb} completed")).await
    }
}

enum Criterion {
    All,
    Seen,
    Unseen,
    Uid(SequenceSet),
}

fn parse_search(args: &[Token]) -> Result<Vec<Criterion>, String> {
    let mut criteria = Vec::new();
    let mut iter = args.iter();
    while let Some(token) = iter.next() {
        let key = token
            .as_text()
            .ok_or("unsupported search key")?
            .to_ascii_uppercase();
        let criterion = match key.as_str() {
            "ALL" => Criterion::All,
            "SEEN" => Criterion::Seen,
            "UNSEEN" => Criterion::Unseen,
            "UID" => {
                let set = iter
                    .next()
                    .and_then(Token::as_text)
                    .ok_or("UID needs a set")?;
                Criterion::Uid(SequenceSet::parse(&set).ok_or("invalid UID set")?)
            }
            _ => return Err(format!("unsupported search key {key}")),
        };
        criteria.push(criterion);
    }
    if criteria.is_empty() {
        return Err("search criteria required".to_owned());
    }
    Ok(criteria)
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
            out.extend_from_slice(format!("FLAGS ({})", message.flags.join(" ")).as_bytes())
        }
        FetchItem::InternalDate => {
            out.extend_from_slice(format!("INTERNALDATE \"{}\"", message.internal_date).as_bytes());
        }
        FetchItem::Size => out.extend_from_slice(format!("RFC822.SIZE {}", raw.len()).as_bytes()),
        FetchItem::Envelope => {
            out.extend_from_slice(b"ENVELOPE ");
            push_envelope(out, raw);
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
        assert!(parse_search(&[]).is_err());
        assert!(parse_search(&command::tokenize(b"FROM x").unwrap()).is_err());
    }
}
