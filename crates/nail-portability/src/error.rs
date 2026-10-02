//! Errors that end a whole job (per-item problems are
//! [`nail_api::portability::ItemIssue`]s in the report instead).

use nail_api::portability::{JobReport, JobState};
use nail_api::{ApiError, ErrorCode, LocalizedMessage};

/// Result of this crate.
pub type Result<T, E = PortabilityError> = std::result::Result<T, E>;

/// Why a job (or a preview / inspection) stopped.
#[derive(Debug, thiserror::Error)]
pub enum PortabilityError {
    /// The source or target could not be read / written.
    #[error("{context}: {source}")]
    Io {
        /// What was being done (a path).
        context: String,
        /// The error.
        #[source]
        source: std::io::Error,
    },
    /// The source is not what its format says (no mbox `From ` line, no
    /// Maildir `cur` folder, no Thunderbird `prefs.js`).
    #[error("not a valid source: {0}")]
    InvalidSource(String),
    /// The export / backup target exists and `overwrite` is off.
    #[error("the target exists: {0}")]
    TargetExists(String),
    /// The user cancelled the job.
    #[error("cancelled")]
    Cancelled,
    /// Wrong passphrase, or not a Nail backup.
    #[error("wrong passphrase or not a Nail backup")]
    WrongPassphrase,
    /// Not a Nail backup at all (magic missing).
    #[error("not a Nail backup: {0}")]
    NotABackup(String),
    /// A backup written by a newer Nail.
    #[error("backup format {0} is newer than this Nail")]
    NewerFormat(u32),
    /// The archive is damaged (authentication failed after the first
    /// chunk, broken zip, missing manifest).
    #[error("the backup is damaged: {0}")]
    Corrupt(String),
    /// The options were refused (passphrase too short, nothing chosen).
    #[error("invalid options: {0}")]
    InvalidOptions(String),
    /// The core's store / queue refused for good.
    #[error("{0}")]
    Sink(String),
    /// The source changed while a backup read it twice.
    #[error("the data changed while it was backed up: {0}")]
    SourceChanged(String),
}

impl PortabilityError {
    /// One error of every kind (for catalog checks).
    pub fn examples() -> Vec<Self> {
        vec![
            Self::io("x", std::io::Error::other("x")),
            Self::InvalidSource(String::new()),
            Self::TargetExists(String::new()),
            Self::Cancelled,
            Self::WrongPassphrase,
            Self::NotABackup(String::new()),
            Self::NewerFormat(2),
            Self::Corrupt(String::new()),
            Self::InvalidOptions(String::new()),
            Self::Sink(String::new()),
            Self::SourceChanged(String::new()),
        ]
    }

    /// An I/O error with context.
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    /// The contract's error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::Io { .. } | Self::Sink(_) | Self::SourceChanged(_) => ErrorCode::Internal,
            Self::InvalidSource(_) | Self::InvalidOptions(_) => ErrorCode::InvalidRequest,
            Self::TargetExists(_) => ErrorCode::Conflict,
            Self::Cancelled => ErrorCode::Cancelled,
            Self::WrongPassphrase => ErrorCode::AuthFailed,
            Self::NotABackup(_) | Self::Corrupt(_) => ErrorCode::Crypto,
            Self::NewerFormat(_) => ErrorCode::Unsupported,
        }
    }

    /// The user-facing text (`core-portability.ftl`).
    pub fn localized(&self) -> LocalizedMessage {
        let id = match self {
            Self::Io { .. } => "portability-error-io",
            Self::InvalidSource(_) => "portability-error-invalid-source",
            Self::TargetExists(_) => "portability-error-target-exists",
            Self::Cancelled => "portability-state-cancelled",
            Self::WrongPassphrase => "backup-wrong-passphrase",
            Self::NotABackup(_) => "portability-error-not-a-backup",
            Self::NewerFormat(_) => "portability-issue-newer-format",
            Self::Corrupt(_) => "portability-error-corrupt-backup",
            Self::InvalidOptions(_) => "portability-error-invalid-options",
            Self::Sink(_) => "portability-error-store",
            Self::SourceChanged(_) => "portability-error-source-changed",
        };
        LocalizedMessage::new(id)
    }

    /// As the contract's API error (previews, inspections).
    pub fn to_api_error(&self) -> ApiError {
        ApiError::new(self.code(), self.to_string()).with_localized(self.localized())
    }

    /// Ends `report` with this error: `cancelled` for
    /// [`PortabilityError::Cancelled`], else `failed` with the error.
    pub fn finish_report(&self, report: &mut JobReport) {
        if matches!(self, Self::Cancelled) {
            report.state = JobState::Cancelled;
            return;
        }
        report.state = JobState::Failed;
        report.error = Some(self.code());
        report.message = Some(self.to_string());
        report.localized = Some(self.localized());
    }
}

/// A store / queue problem reported by the core's sink or source.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    /// This item failed (reported as `write-failed`, the job goes on).
    #[error("{0}")]
    Item(String),
    /// Nothing more can be written (the job fails).
    #[error("{0}")]
    Fatal(String),
}

impl From<SinkError> for PortabilityError {
    fn from(error: SinkError) -> Self {
        Self::Sink(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::portability::JobKind;

    #[test]
    fn errors_map_to_codes_and_texts() {
        assert_eq!(
            PortabilityError::WrongPassphrase.code(),
            ErrorCode::AuthFailed
        );
        assert_eq!(
            PortabilityError::WrongPassphrase.localized().id,
            "backup-wrong-passphrase"
        );
        assert_eq!(
            PortabilityError::TargetExists("x".into()).code(),
            ErrorCode::Conflict
        );
        let api = PortabilityError::NewerFormat(9).to_api_error();
        assert_eq!(api.code, ErrorCode::Unsupported);
        assert!(api.message.contains('9'));
    }

    #[test]
    fn finishing_a_report() {
        let mut report = JobReport::started("j".into(), JobKind::Import, "2026-10-02T00:00:00Z");
        PortabilityError::Cancelled.finish_report(&mut report);
        assert_eq!(report.state, JobState::Cancelled);
        assert!(report.error.is_none());
        PortabilityError::InvalidSource("no".into()).finish_report(&mut report);
        assert_eq!(report.state, JobState::Failed);
        assert_eq!(report.error, Some(ErrorCode::InvalidRequest));
        assert_eq!(
            report.localized.unwrap().id,
            "portability-error-invalid-source"
        );
    }
}
