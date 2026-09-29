//! Test support for Nail: an in-process mock IMAP server, a mail corpus and
//! TLS test material.
//!
//! * [`corpus`]: realistic, malformed and hostile `.eml` messages in
//!   `INBOX` / `Sent` / `Archive`, embedded at build time ([`demo_corpus`]).
//! * [`MailStore`]: the mailbox state a mock server serves. Tests change it
//!   while clients are connected ([`MailStore::add_message`],
//!   [`MailStore::remove_message`], [`MailStore::set_flags`], ...); every
//!   change bumps the mailbox mod-sequence and is pushed to sessions in
//!   `IDLE`.
//! * [`MockImapServer`]: an IMAP4rev1 server on `127.0.0.1:0` with plaintext,
//!   implicit TLS or STARTTLS, configurable credentials and OAuth2 token,
//!   configurable extensions ([`MockCapabilities`]: IDLE, CONDSTORE,
//!   QRESYNC, MOVE, UIDPLUS, COMPRESS, ID, SASL-IR, LITERAL+, APPENDLIMIT),
//!   a command log, and failure injection (disconnects, `NO`s, dropped IDLE
//!   connections, slow responses).
//!
//! ```no_run
//! # async fn demo() -> Result<(), Box<dyn std::error::Error>> {
//! use nail_testkit::{MailStore, MockImapConfig, MockImapServer};
//!
//! let server = MockImapServer::start(MockImapConfig::default(), MailStore::demo()).await?;
//! println!("IMAP on {}", server.addr());
//! # Ok(()) }
//! ```

pub mod corpus;
pub mod imap;
pub mod mailstore;
pub mod tls;

pub use corpus::{
    CorpusFolder, CorpusMessage, corpus_dir, corpus_message, demo_corpus, demo_corpus_raw,
    load_mail_dir,
};
pub use imap::{
    ClientId, MockCapabilities, MockImapConfig, MockImapServer, MockServerError, MockTls,
};
pub use mailstore::{
    FlagUpdate, MailStore, Mailbox, MailboxError, StoreOutcome, StoredFlags, StoredMessage,
};
