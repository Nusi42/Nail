//! Test support for Nail: an in-process mock IMAP server, a mail corpus and
//! TLS test material.
//!
//! * [`corpus`]: realistic, malformed and hostile `.eml` messages in
//!   `INBOX` / `Sent` / `Archive`, embedded at build time ([`demo_corpus`]).
//! * [`MailStore`]: the mailbox state a mock server serves.
//! * [`MockImapServer`]: an IMAP4rev1 server on `127.0.0.1:0` with plaintext,
//!   implicit TLS or STARTTLS, configurable credentials and failure injection.
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
pub use imap::{MockImapConfig, MockImapServer, MockServerError, MockTls};
pub use mailstore::{MailStore, Mailbox, StoredMessage};
