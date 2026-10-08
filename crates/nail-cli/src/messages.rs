//! User-facing output of the `nail` command (M2c WP53; ADR 0043).
//!
//! Errors and notices a user reads are [`LocalizedMessage`]s (ids `cli-*`
//! and the producers' own, in `locales/<lang>/core.ftl`), rendered with
//! `nail-config`'s catalogs in the language of `NAIL_LOCALE` / `LANG`
//! (English otherwise). The command-line help and log lines stay English
//! developer tooling (ADR 0009); stdout of `nail mcp` carries only
//! JSON-RPC, so everything here goes to stderr.

use nail_api::LocalizedMessage;
use nail_config::l10n::{self, Catalog};
use nail_mcp::bridge::BridgeError;
use nail_store::LockError;

/// An error with its user-facing text; the cause (if any) is kept as the
/// diagnostic.
#[derive(Debug)]
pub(crate) struct CliError {
    /// What the user reads.
    pub message: LocalizedMessage,
    /// The underlying error.
    pub source: Option<anyhow::Error>,
}

impl std::fmt::Display for CliError {
    /// The English text (logs, tests).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&l10n::english(&self.message))
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_ref().map(AsRef::as_ref)
    }
}

/// An error that is only its message.
pub(crate) fn error(message: LocalizedMessage) -> anyhow::Error {
    CliError {
        message,
        source: None,
    }
    .into()
}

/// Attaches a user-facing message to a failure.
pub(crate) trait Localized<T> {
    /// `Err(e)` becomes a [`CliError`] with `message()` and `e` as cause.
    fn localized(self, message: impl FnOnce() -> LocalizedMessage) -> anyhow::Result<T>;
}

impl<T, E> Localized<T> for Result<T, E>
where
    E: Into<anyhow::Error>,
{
    fn localized(self, message: impl FnOnce() -> LocalizedMessage) -> anyhow::Result<T> {
        self.map_err(|e| {
            CliError {
                message: message(),
                source: Some(e.into()),
            }
            .into()
        })
    }
}

impl<T> Localized<T> for Option<T> {
    fn localized(self, message: impl FnOnce() -> LocalizedMessage) -> anyhow::Result<T> {
        self.ok_or_else(|| error(message()))
    }
}

/// The user-facing text of an error: the first message in its chain that
/// has one (a [`CliError`], a bridge or lock error), else a generic text
/// with the English error as detail; causes follow as "cause" lines.
pub(crate) fn render_error(error: &anyhow::Error, catalog: &Catalog) -> String {
    let mut lines = Vec::new();
    let mut explained = false;
    for cause in error.chain() {
        let localized = if let Some(cli) = cause.downcast_ref::<CliError>() {
            Some(cli.message.clone())
        } else if let Some(bridge) = cause.downcast_ref::<BridgeError>() {
            Some(bridge.localized())
        } else {
            cause.downcast_ref::<LockError>().map(lock_message)
        };
        match localized {
            Some(message) if !explained => {
                lines.push(catalog.render(&message));
                explained = true;
            }
            // A second explained error adds nothing a user needs.
            Some(_) => {}
            None if !explained => {
                lines.push(
                    catalog.render(
                        &LocalizedMessage::new("cli-error").arg("detail", cause.to_string()),
                    ),
                );
                explained = true;
            }
            None => lines.push(catalog.render(
                &LocalizedMessage::new("cli-error-cause").arg("detail", cause.to_string()),
            )),
        }
    }
    lines.join("\n")
}

/// The text of a data directory lock failure.
pub(crate) fn lock_message(error: &LockError) -> LocalizedMessage {
    match error {
        LockError::Held(Some(holder)) => LocalizedMessage::new("cli-data-dir-in-use-by")
            .arg("owner", holder.owner.as_str())
            .arg("pid", holder.pid),
        LockError::Held(None) => LocalizedMessage::new("cli-data-dir-in-use"),
        LockError::Io(err) => {
            LocalizedMessage::new("cli-data-dir-lock-failed").arg("detail", err.to_string())
        }
    }
}

/// The error of an MCP server that did not start on `port`: why, when the
/// cause is known (a fixed port in use), else the error as detail.
pub(crate) fn mcp_start_failed(port: u16, err: anyhow::Error) -> anyhow::Error {
    let message = err
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>())
        .map_or_else(
            || LocalizedMessage::new("mcp-core-start-failed").arg("detail", err.to_string()),
            |io| nail_mcp::messages::start_error(port, io),
        );
    CliError {
        message,
        source: Some(err),
    }
    .into()
}

/// The catalog for command-line output.
pub(crate) fn catalog() -> Catalog {
    Catalog::for_locale(l10n::locale_from_env().as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_store::LockHolder;

    #[test]
    fn errors_render_their_first_user_facing_text() {
        let en = Catalog::english();
        let io = std::io::Error::other("disk on fire");
        let err: anyhow::Result<()> = Err(io)
            .localized(|| LocalizedMessage::new("cli-cannot-open-data-dir").arg("dir", "D:\\nail"));
        let text = render_error(&err.unwrap_err(), &en);
        assert_eq!(
            text,
            "Cannot open the data directory D:\\nail.\nCause: disk on fire"
        );

        let bridge = anyhow::Error::from(BridgeError::Unauthorized);
        assert!(render_error(&bridge, &en).contains("refused"));

        let held = anyhow::Error::from(LockError::Held(Some(LockHolder {
            pid: 42,
            owner: "serve".into(),
            since: 0,
        })));
        let text = render_error(&held, &en);
        assert!(text.contains("serve") && text.contains("42"), "{text}");

        let plain = anyhow::anyhow!("something odd");
        assert_eq!(
            render_error(&plain, &en),
            en.render(&LocalizedMessage::new("cli-error").arg("detail", "something odd"))
        );
        let none: Option<u8> = None;
        let err = none
            .localized(|| LocalizedMessage::new("cli-mcp-no-data-dir"))
            .unwrap_err();
        assert!(err.to_string().contains("--data-dir"), "{err}");
    }

    /// Every `cli-*` id built here has an English text in core.ftl.
    #[test]
    fn every_message_id_is_in_the_catalog() {
        l10n::assert_message_ids_in_catalog(
            env!("CARGO_MANIFEST_DIR"),
            &["cli-", "mcp-core-", "config-"],
        );
    }
}
