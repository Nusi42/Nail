//! Reading a message file the user opened (`.eml`, ADR 0041) with
//! localized errors (M2c carry-over WP65).
//!
//! [`read_message_file`] reads at most [`MAX_MESSAGE_FILE`] bytes, parses
//! them and refuses empty, oversized and non-mail files. Every refusal is a
//! [`MessageFileError`] whose user-facing text is a [`LocalizedMessage`]
//! (`message-file-*` in `locales/en/core-crypto.ftl`); its `Display` is the
//! English diagnostic for logs. The OS error text stays an English
//! `detail` argument (library text).

use std::io::Read;
use std::path::Path;

use nail_api::{ApiError, ErrorCode, LocalizedMessage};

use crate::{ParsedMessage, parse_message};

/// The largest message file Nail opens (50 MiB).
pub const MAX_MESSAGE_FILE: u64 = 50 * 1024 * 1024;

/// The Fluent ids [`MessageFileError::localized`] builds.
pub const MESSAGE_FILE_MESSAGE_IDS: &[&str] = &[
    "message-file-unreadable",
    "message-file-size",
    "message-file-not-mail",
];

/// A message file that was read and parsed.
#[derive(Debug, Clone)]
pub struct MessageFile {
    /// The raw bytes (at most [`MAX_MESSAGE_FILE`]).
    pub raw: Vec<u8>,
    /// The parsed message.
    pub parsed: ParsedMessage,
}

/// Why a message file cannot be opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageFileError {
    /// The file cannot be opened or read.
    Unreadable {
        /// The path as shown to the user.
        path: String,
        /// The OS error (English library text).
        detail: String,
    },
    /// The file is empty or larger than [`MAX_MESSAGE_FILE`].
    Size {
        /// The path as shown to the user.
        path: String,
    },
    /// The content does not look like an email message.
    NotMail {
        /// The path as shown to the user.
        path: String,
    },
}

impl MessageFileError {
    /// The protocol error class (the same classes the core used before).
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Unreadable { .. } => ErrorCode::NotFound,
            Self::Size { .. } | Self::NotMail { .. } => ErrorCode::InvalidRequest,
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        match self {
            Self::Unreadable { path, detail } => LocalizedMessage::new("message-file-unreadable")
                .arg("path", path.as_str())
                .arg("detail", detail.as_str()),
            Self::Size { path } => LocalizedMessage::new("message-file-size")
                .arg("path", path.as_str())
                .arg("max", MAX_MESSAGE_FILE),
            Self::NotMail { path } => {
                LocalizedMessage::new("message-file-not-mail").arg("path", path.as_str())
            }
        }
    }
}

impl std::fmt::Display for MessageFileError {
    /// The English diagnostic (logs, the `message` of an [`ApiError`]).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreadable { path, detail } => write!(f, "{path}: {detail}"),
            Self::Size { path } => {
                write!(f, "{path} is empty or larger than {MAX_MESSAGE_FILE} bytes")
            }
            Self::NotMail { path } => write!(f, "{path} is not an email message"),
        }
    }
}

impl std::error::Error for MessageFileError {}

impl From<MessageFileError> for ApiError {
    fn from(err: MessageFileError) -> Self {
        let localized = err.localized();
        ApiError::new(err.code(), err.to_string()).with_localized(localized)
    }
}

/// Reads and parses the message file at `path`.
pub fn read_message_file(path: &Path) -> Result<MessageFile, MessageFileError> {
    let shown = path.display().to_string();
    let unreadable = |err: std::io::Error| MessageFileError::Unreadable {
        path: shown.clone(),
        detail: err.to_string(),
    };
    let file = std::fs::File::open(path).map_err(unreadable)?;
    // Read one byte past the limit instead of trusting the metadata, so a
    // file that grows meanwhile is still refused.
    let mut raw = Vec::new();
    file.take(MAX_MESSAGE_FILE + 1)
        .read_to_end(&mut raw)
        .map_err(unreadable)?;
    let parsed = check_message_bytes(&shown, &raw)?;
    Ok(MessageFile { raw, parsed })
}

/// Checks bytes read from a message file shown as `path` and parses them.
pub fn check_message_bytes(path: &str, raw: &[u8]) -> Result<ParsedMessage, MessageFileError> {
    let too_large = u64::try_from(raw.len()).map_or(true, |len| len > MAX_MESSAGE_FILE);
    if raw.is_empty() || too_large {
        return Err(MessageFileError::Size {
            path: path.to_owned(),
        });
    }
    let parsed = parse_message(raw);
    if parsed.headers.from.is_empty() && parsed.headers.subject.is_empty() && parsed.text.is_empty()
    {
        return Err(MessageFileError::NotMail {
            path: path.to_owned(),
        });
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::localized::MessageArg;
    use pretty_assertions::assert_eq;

    const MAIL: &[u8] =
        b"From: Shop <news@shop.example>\r\nSubject: Autumn offer\r\n\r\nHello Ada\r\n";

    fn catalog() -> String {
        std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales/en/core-crypto.ftl"),
        )
        .unwrap()
    }

    #[test]
    fn reads_and_parses_a_message_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("offer.eml");
        std::fs::write(&path, MAIL).unwrap();
        let file = read_message_file(&path).unwrap();
        assert_eq!(file.raw, MAIL);
        assert_eq!(file.parsed.headers.subject, "Autumn offer");
    }

    #[test]
    fn a_missing_file_is_unreadable_with_the_os_detail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing.eml");
        let err = read_message_file(&path).unwrap_err();
        let shown = path.display().to_string();
        let MessageFileError::Unreadable { path: p, detail } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(p, &shown);
        assert!(!detail.is_empty());
        assert_eq!(err.code(), ErrorCode::NotFound);
        let localized = err.localized();
        assert_eq!(localized.id, "message-file-unreadable");
        assert_eq!(localized.args["path"], MessageArg::Text(shown.clone()));
        assert_eq!(localized.args["detail"], MessageArg::Text(detail.clone()));
    }

    #[test]
    fn empty_and_oversized_files_are_refused() {
        let empty = check_message_bytes("a.eml", b"").unwrap_err();
        assert_eq!(
            empty,
            MessageFileError::Size {
                path: "a.eml".into()
            }
        );
        assert_eq!(empty.code(), ErrorCode::InvalidRequest);
        let localized = empty.localized();
        assert_eq!(localized.id, "message-file-size");
        assert_eq!(
            localized.args["max"],
            MessageArg::Number(i64::try_from(MAX_MESSAGE_FILE).unwrap())
        );
        let big = vec![b'a'; usize::try_from(MAX_MESSAGE_FILE).unwrap() + 1];
        assert!(matches!(
            check_message_bytes("big.eml", &big),
            Err(MessageFileError::Size { .. })
        ));
    }

    #[test]
    fn an_oversized_file_on_disk_is_refused_without_reading_it_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.eml");
        let file = std::fs::File::create(&path).unwrap();
        // A sparse-ish file one byte over the limit.
        file.set_len(MAX_MESSAGE_FILE + 1).unwrap();
        drop(file);
        assert!(matches!(
            read_message_file(&path),
            Err(MessageFileError::Size { .. })
        ));
    }

    #[test]
    fn binary_data_is_not_mail() {
        let err = check_message_bytes("x.bin", &[0u8; 16]).unwrap_err();
        assert_eq!(
            err,
            MessageFileError::NotMail {
                path: "x.bin".into()
            }
        );
        assert_eq!(err.localized().id, "message-file-not-mail");
        assert_eq!(err.to_string(), "x.bin is not an email message");
    }

    #[test]
    fn api_errors_carry_code_english_message_and_localized_text() {
        let err = MessageFileError::NotMail {
            path: "x.bin".into(),
        };
        let api: ApiError = err.clone().into();
        assert_eq!(api.code, ErrorCode::InvalidRequest);
        assert_eq!(api.message, "x.bin is not an email message");
        assert_eq!(api.localized.as_deref(), Some(&err.localized()));
    }

    #[test]
    fn every_message_id_has_an_english_text_with_its_arguments() {
        let catalog = catalog();
        let errors = [
            MessageFileError::Unreadable {
                path: "p".into(),
                detail: "d".into(),
            },
            MessageFileError::Size { path: "p".into() },
            MessageFileError::NotMail { path: "p".into() },
        ];
        let built: Vec<String> = errors.iter().map(|e| e.localized().id).collect();
        assert_eq!(built, MESSAGE_FILE_MESSAGE_IDS);
        for error in &errors {
            let localized = error.localized();
            let line = catalog
                .lines()
                .find(|l| l.starts_with(&format!("{} =", localized.id)))
                .unwrap_or_else(|| panic!("{} is not in core-crypto.ftl", localized.id));
            for arg in localized.args.keys() {
                assert!(
                    line.contains(&format!("{{ ${arg} }}")),
                    "{} does not use ${arg}",
                    localized.id
                );
            }
        }
    }
}
