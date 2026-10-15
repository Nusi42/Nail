//! A message's source for "Show source" (ADR 0194): the message exactly as
//! it was received, as text, and a decoded view of it for reading.
//!
//! [`crate::Request::GetMessageSource`] answers both. Nothing is loaded from
//! the network to build them (a body synced headers-only is downloaded
//! first, like [`crate::Request::GetMessage`] does).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The most bytes of one view [`MessageSource`] returns; longer views are
/// cut there (at a character boundary) and flagged as truncated.
pub const MAX_MESSAGE_SOURCE_BYTES: usize = 4 * 1024 * 1024;

/// How deep the decoded view follows nested parts (multiparts and attached
/// `message/rfc822` messages); deeper ones are left out.
pub const MAX_SOURCE_NESTING: usize = 32;

/// A message's source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct MessageSource {
    /// Size of the whole message in bytes.
    pub size: u64,
    /// The message as received (RFC 5322 header and MIME body, encoded as
    /// sent), as text: bytes that are not UTF-8 show as U+FFFD.
    pub original: String,
    /// `original` was cut at [`MAX_MESSAGE_SOURCE_BYTES`].
    pub original_truncated: bool,
    /// The message's header fields, then each MIME part with its header
    /// fields and its content, decoded (encoded words, transfer encoding,
    /// charset; folded fields joined); parts that are not text are left out. An end-to-end encrypted message is shown
    /// decrypted, as the reader shows it.
    pub decoded: Vec<SourceChunk>,
    /// `decoded` was cut at [`MAX_MESSAGE_SOURCE_BYTES`].
    pub decoded_truncated: bool,
    /// The message is end-to-end encrypted: `original` is the encrypted
    /// message, `decoded` the decrypted one.
    pub encrypted: bool,
}

/// One piece of [`MessageSource::decoded`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SourceChunk {
    /// Text shown as it is (headers, boundaries, decoded text parts).
    Text {
        /// The text.
        text: String,
    },
    /// A part whose content is not shown (not text, or nested too deep);
    /// the UI writes one line naming it.
    Omitted {
        /// The part's MIME type.
        mime: String,
        /// The part's file name, if it has one.
        filename: Option<String>,
        /// Size of the decoded content in bytes.
        size: u64,
    },
}
