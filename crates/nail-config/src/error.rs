//! The configuration error type.

use nail_api::{ApiError, ConfigIssue, ErrorCode, Event, LocalizedMessage, SourceLocation};

use crate::l10n;

/// What went wrong with a configuration file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfigErrorKind {
    /// The file could not be read (or the config directory could not be
    /// watched).
    Io,
    /// The file is not valid KDL.
    Syntax,
    /// The file is valid KDL but not a valid theme or layout: an unknown
    /// node, a wrong value, an unknown token reference, …
    Invalid,
}

/// A configuration error with its position in the file.
///
/// `localized` is the user-facing text (a Fluent id of
/// `locales/<lang>/core.ftl` with arguments, M2c ADR 0043); `message` is its
/// English rendering, a diagnostic like a compiler message (logs, CLI).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{file}:{line}:{column}: {message}")]
pub struct ConfigError {
    /// Error class.
    pub kind: ConfigErrorKind,
    /// File name as shown to the user, e.g. `"theme.kdl"`.
    pub file: String,
    /// 1-based line (1 when the error has no position, e.g. I/O errors).
    pub line: u32,
    /// 1-based column in characters (1 when the error has no position).
    pub column: u32,
    /// Readable description of the problem (English).
    pub message: String,
    /// The same as a Fluent id + arguments (what the UI shows).
    pub localized: Box<LocalizedMessage>,
}

impl ConfigError {
    /// Creates an error at a source location.
    pub fn at(kind: ConfigErrorKind, loc: &SourceLocation, localized: LocalizedMessage) -> Self {
        Self {
            kind,
            file: loc.file.clone(),
            line: loc.line,
            column: loc.column,
            message: l10n::english(&localized),
            localized: Box::new(localized),
        }
    }

    /// Creates an [`ConfigErrorKind::Invalid`] error at a source location.
    pub fn invalid(loc: &SourceLocation, localized: LocalizedMessage) -> Self {
        Self::at(ConfigErrorKind::Invalid, loc, localized)
    }

    /// Creates an [`ConfigErrorKind::Io`] error for a whole file
    /// (`config-io`; the OS error text is an argument).
    pub fn io(file: impl Into<String>, error: &std::io::Error) -> Self {
        let file = file.into();
        let localized = l10n::msg("config-io")
            .arg("file", file.as_str())
            .arg("detail", error.to_string());
        Self {
            kind: ConfigErrorKind::Io,
            file,
            line: 1,
            column: 1,
            message: error.to_string(),
            localized: Box::new(localized),
        }
    }

    /// The error's location.
    pub fn location(&self) -> SourceLocation {
        SourceLocation {
            file: self.file.clone(),
            line: self.line,
            column: self.column,
        }
    }

    /// The [`Event::ConfigError`] announcing this error to clients.
    pub fn to_event(&self) -> Event {
        Event::ConfigError {
            file: self.file.clone(),
            line: self.line,
            column: self.column,
            message: self.message.clone(),
            localized: Some((*self.localized).clone()),
        }
    }

    /// The error as a protocol [`ConfigIssue`] (position and message; the
    /// file is given by the surrounding event or response).
    pub fn to_issue(&self) -> ConfigIssue {
        ConfigIssue {
            line: self.line,
            column: self.column,
            message: self.message.clone(),
            localized: Some((*self.localized).clone()),
        }
    }
}

/// The [`Event::ConfigErrors`] listing every error of `file` (empty =
/// valid). Errors located in another file (e.g. in the shipped defaults a
/// sparse file extends) keep their message but are prefixed with that
/// file's name, since the event names only one file.
pub fn issues_event(file: &str, errors: &[ConfigError]) -> Event {
    Event::ConfigErrors {
        file: file.to_owned(),
        errors: issues(file, errors),
    }
}

/// [`ConfigError`]s as [`ConfigIssue`]s of `file`; see [`issues_event`].
pub fn issues(file: &str, errors: &[ConfigError]) -> Vec<ConfigIssue> {
    errors
        .iter()
        .map(|err| {
            let mut issue = err.to_issue();
            if err.file != file {
                issue.message = format!("{}: {}", err.file, err.message);
                // The event names one file: say where this error is.
                if let Some(localized) = &mut issue.localized {
                    localized
                        .args
                        .insert("in_file".to_owned(), err.file.as_str().into());
                }
            }
            issue
        })
        .collect()
}

impl From<ConfigError> for ApiError {
    fn from(error: ConfigError) -> Self {
        let localized = (*error.localized)
            .clone()
            .arg("file", error.file.as_str())
            .arg("line", error.line)
            .arg("column", error.column);
        ApiError::new(ErrorCode::Config, error.to_string()).with_localized(localized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ConfigError {
        ConfigError::invalid(
            &SourceLocation {
                file: "theme.kdl".into(),
                line: 3,
                column: 7,
            },
            l10n::msg("config-unknown-token").arg("token", "$color.nope"),
        )
    }

    #[test]
    fn display_is_compiler_like() {
        assert_eq!(
            sample().to_string(),
            "theme.kdl:3:7: unknown token `$color.nope`"
        );
    }

    #[test]
    fn converts_to_event_and_api_error() {
        let err = sample();
        assert_eq!(
            err.to_event(),
            Event::ConfigError {
                file: "theme.kdl".into(),
                line: 3,
                column: 7,
                message: "unknown token `$color.nope`".into(),
                localized: Some(l10n::msg("config-unknown-token").arg("token", "$color.nope")),
            }
        );
        let api: ApiError = err.clone().into();
        assert_eq!(api.code, ErrorCode::Config);
        assert!(api.message.contains("theme.kdl:3:7"));
        let localized = api.localized.unwrap();
        assert_eq!(localized.id, "config-unknown-token");
        assert_eq!(localized.args["line"], 3u32.into());
        assert_eq!(err.location().line, 3);
        // An error of another file names it in the issue.
        let issues = issues("layout.kdl", &[err]);
        assert!(issues[0].message.starts_with("theme.kdl: "));
        assert_eq!(
            issues[0].localized.as_ref().unwrap().args["in_file"],
            "theme.kdl".into()
        );
    }

    #[test]
    fn io_errors_have_no_position() {
        let err = ConfigError::io(
            "layout.kdl",
            &std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        assert_eq!(
            (err.kind, err.line, err.column),
            (ConfigErrorKind::Io, 1, 1)
        );
        assert_eq!(err.message, "denied");
        assert_eq!(err.localized.id, "config-io");
        assert_eq!(
            l10n::english(&err.localized),
            "cannot read layout.kdl: denied"
        );
    }
}
