//! User-facing texts of the MCP server (M2c WP53; ADR 0043).
//!
//! Agents read English: tool descriptions, JSON-RPC errors and the `hint`
//! of a refused call stay English on the MCP wire. What a *user* reads is
//! a [`LocalizedMessage`] (ids `mcp-core-*` in `locales/<lang>/core.ftl`):
//!
//! * the audit log's details ([`audit_detail`]): the store keeps the
//!   English diagnostic the server wrote (no `localized` column yet); this
//!   module maps every detail it writes back to its message, so the core
//!   can fill `McpAuditEntry.localized` when it lists the log;
//! * administration errors (client names), with `ApiError::localized`;
//! * starting the server on a fixed port ([`start_error`]);
//! * the stdio bridge's errors ([`crate::bridge::BridgeError::localized`]),
//!   which `nail mcp` prints.
//! * M4a (ids `mcp-tool-*` in `core-mcp.ftl`): the audit details of writes
//!   (`write {…}` with the content-free before / after lines), replayed
//!   idempotent calls and rate-limited calls.

use nail_api::mcp::McpScope;
use nail_api::{ErrorCode, LocalizedMessage};

use crate::access::{Denial, scope_name};

/// Audit detail of a request without a bearer token.
pub const DETAIL_MISSING_TOKEN: &str = "missing bearer token";
/// Audit detail of a request with an unknown or revoked token.
pub const DETAIL_REFUSED_TOKEN: &str = "unknown or revoked token";
/// Suffix of the summary entry of refused connections not logged one by one.
const MORE_REFUSED_SUFFIX: &str = " more refused connections not logged";
/// Prefix of a write's audit detail (followed by JSON).
pub const WRITE_DETAIL_PREFIX: &str = "write ";
/// Audit detail of a write answered from its idempotency key.
pub const DETAIL_REPLAYED: &str = "idempotent replay of an earlier result";

/// The audit detail summing up `count` refused connections.
pub fn refused_summary(count: u32) -> String {
    format!("{count}{MORE_REFUSED_SUFFIX}")
}

/// The user-facing text of a refusal.
pub fn denial(denial: &Denial) -> LocalizedMessage {
    match denial {
        Denial::Scope(scope) => {
            LocalizedMessage::new("mcp-core-denied-scope").arg("scope", scope_name(*scope))
        }
        Denial::Account(account) => {
            LocalizedMessage::new("mcp-core-denied-account").arg("account", account.as_str())
        }
        Denial::Folder(folder) => {
            LocalizedMessage::new("mcp-core-denied-folder").arg("folder", folder.as_str())
        }
        Denial::AllAccounts => LocalizedMessage::new("mcp-core-denied-all-accounts"),
    }
}

/// The user-facing text of an audit entry's `detail` as the MCP server
/// writes it (refusals, token checks, API errors as `code: message`).
/// `None` for a detail this server does not write.
pub fn audit_detail(detail: &str) -> Option<LocalizedMessage> {
    let detail = detail.trim();
    match detail {
        DETAIL_MISSING_TOKEN => return Some(LocalizedMessage::new("mcp-core-token-missing")),
        DETAIL_REFUSED_TOKEN => return Some(LocalizedMessage::new("mcp-core-token-refused")),
        _ => {}
    }
    if let Some(count) = detail
        .strip_suffix(MORE_REFUSED_SUFFIX)
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(LocalizedMessage::new("mcp-core-refused-more").arg("count", count));
    }
    if let Some(parsed) = parse_denial(detail) {
        return Some(denial(&parsed));
    }
    if detail == DETAIL_REPLAYED {
        return Some(LocalizedMessage::new("mcp-tool-audit-replayed"));
    }
    if let Some(json) = detail.strip_prefix(WRITE_DETAIL_PREFIX) {
        return write_detail(json);
    }
    if let Some(rest) = detail.strip_prefix("rate limit `") {
        let limit = rest.split('`').next().unwrap_or_default();
        return Some(LocalizedMessage::new("mcp-tool-audit-rate-limited").arg("limit", limit));
    }
    // `ApiError`'s display form: `<code>: <message>`.
    let (code, _) = detail.split_once(": ")?;
    ErrorCode::ALL
        .iter()
        .find(|c| c.as_str() == code)
        .map(|c| LocalizedMessage::for_code(*c))
}

/// The text of a write's audit detail: what happened, to how many items,
/// with how many changed fields.
fn write_detail(json: &str) -> Option<LocalizedMessage> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let targets = value["targets"].as_array().map_or(0, Vec::len)
        + usize::try_from(value["more_targets"].as_u64().unwrap_or(0)).unwrap_or(0);
    let changes = value["changes"].as_array().map_or(0, Vec::len);
    let id = match value["status"].as_str()? {
        "done" => "mcp-tool-audit-done",
        "denied" => "mcp-tool-audit-denied",
        "expired" => "mcp-tool-audit-expired",
        "vetoed" => "mcp-tool-audit-vetoed",
        "failed" => "mcp-tool-audit-failed",
        _ => return None,
    };
    Some(
        LocalizedMessage::new(id)
            .arg("count", targets)
            .arg("changes", changes),
    )
}

/// Reads back a [`Denial`]'s display form.
fn parse_denial(detail: &str) -> Option<Denial> {
    let between = |prefix: &str, suffix: &str| {
        detail
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
            .map(str::to_owned)
    };
    if detail == Denial::AllAccounts.to_string() {
        return Some(Denial::AllAccounts);
    }
    if let Some(scope) = between("scope `", "` not granted") {
        let scope: McpScope = serde_json::from_value(serde_json::Value::String(scope)).ok()?;
        return Some(Denial::Scope(scope));
    }
    if let Some(account) = between("account `", "` not granted") {
        return Some(Denial::Account(account.into()));
    }
    between("folder `", "` excluded or not in a granted account")
        .map(|folder| Denial::Folder(folder.into()))
}

/// Why the server could not start on `port` (0 = any free port).
pub fn start_error(port: u16, error: &std::io::Error) -> LocalizedMessage {
    match error.kind() {
        std::io::ErrorKind::AddrInUse if port != 0 => {
            LocalizedMessage::new("mcp-core-port-in-use").arg("port", port)
        }
        std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::AddrNotAvailable
            if port != 0 =>
        {
            LocalizedMessage::new("mcp-core-port-denied").arg("port", port)
        }
        _ => LocalizedMessage::new("mcp-core-start-failed").arg("detail", error.to_string()),
    }
}

/// The error for an MCP client name outside 1 to `max` characters.
pub fn client_name_length(max: usize) -> LocalizedMessage {
    LocalizedMessage::new("mcp-core-client-name-length").arg("max", max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_written_detail_maps_back() {
        for (detail, id) in [
            (DETAIL_MISSING_TOKEN, "mcp-core-token-missing"),
            (DETAIL_REFUSED_TOKEN, "mcp-core-token-refused"),
            ("scope `raw-source` not granted", "mcp-core-denied-scope"),
            ("account `work` not granted", "mcp-core-denied-account"),
            (
                "folder `f-1` excluded or not in a granted account",
                "mcp-core-denied-folder",
            ),
            (
                "needs a grant for all accounts",
                "mcp-core-denied-all-accounts",
            ),
            ("not_found: message m1", "error-not-found"),
        ] {
            assert_eq!(
                audit_detail(detail).map(|m| m.id),
                Some(id.to_owned()),
                "{detail}"
            );
        }
        let summary = audit_detail(&refused_summary(7)).unwrap();
        assert_eq!(summary.id, "mcp-core-refused-more");
        assert_eq!(summary.args["count"], 7u32.into());
        let scope = audit_detail("scope `raw-source` not granted").unwrap();
        assert_eq!(scope.args["scope"], "raw-source".into());
        assert_eq!(audit_detail("something else"), None);
        // M4a: writes, replays, rate limits.
        let write = audit_detail(
            r#"write {"targets":["m1","m2"],"changes":[{"field":"flagged"}],"status":"done"}"#,
        )
        .unwrap();
        assert_eq!(write.id, "mcp-tool-audit-done");
        assert_eq!(write.args["count"], 2u32.into());
        assert_eq!(write.args["changes"], 1u32.into());
        for (status, id) in [
            ("denied", "mcp-tool-audit-denied"),
            ("expired", "mcp-tool-audit-expired"),
            ("vetoed", "mcp-tool-audit-vetoed"),
            ("failed", "mcp-tool-audit-failed"),
        ] {
            let detail = format!(r#"write {{"targets":[],"changes":[],"status":"{status}"}}"#);
            assert_eq!(audit_detail(&detail).map(|m| m.id), Some(id.to_owned()));
        }
        assert_eq!(audit_detail("write {broken"), None);
        assert_eq!(
            audit_detail(DETAIL_REPLAYED).map(|m| m.id).as_deref(),
            Some("mcp-tool-audit-replayed")
        );
        let limited =
            audit_detail("rate limit `writes_per_minute` (20) reached; retry after 3 s").unwrap();
        assert_eq!(limited.id, "mcp-tool-audit-rate-limited");
        assert_eq!(limited.args["limit"], "writes_per_minute".into());
        assert_eq!(audit_detail("bogus_code: x"), None);
        assert_eq!(audit_detail("scope `nope` not granted"), None);
    }

    /// The display forms the parser reads are the ones `Denial` writes.
    #[test]
    fn denials_round_trip_through_their_display_form() {
        for denial_value in [
            Denial::Scope(McpScope::RawSource),
            Denial::Account("work".into()),
            Denial::Folder("f-1".into()),
            Denial::AllAccounts,
        ] {
            assert_eq!(
                audit_detail(&denial_value.to_string()),
                Some(denial(&denial_value)),
                "{denial_value}"
            );
        }
    }

    /// Every `mcp-core-*` id built here has an English text in core.ftl.
    #[test]
    fn every_message_id_is_in_the_catalog() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let missing = nail_config::l10n::missing_message_ids(
            &src,
            &[
                "mcp-core-",
                "error-",
                "mcp-tool-",
                "mcp-prompt-",
                "confirmation-summary-",
                "action-field-",
            ],
        );
        assert!(missing.is_empty(), "{missing:#?}");
        let english = nail_config::l10n::english(&start_error(
            4799,
            &std::io::Error::from(std::io::ErrorKind::AddrInUse),
        ));
        assert!(english.contains("4799"), "{english}");
    }

    #[test]
    fn start_errors_explain_a_taken_fixed_port() {
        let in_use = std::io::Error::from(std::io::ErrorKind::AddrInUse);
        assert_eq!(start_error(4799, &in_use).id, "mcp-core-port-in-use");
        assert_eq!(start_error(4799, &in_use).args["port"], 4799u16.into());
        assert_eq!(start_error(0, &in_use).id, "mcp-core-start-failed");
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(start_error(4799, &denied).id, "mcp-core-port-denied");
        assert_eq!(client_name_length(64).args["max"], 64u32.into());
    }
}
