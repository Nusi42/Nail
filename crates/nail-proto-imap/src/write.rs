//! Writes: flags (`UID STORE`), `COPY` / `MOVE` (with the `COPY` +
//! `\Deleted` + `UID EXPUNGE` fallback), expunge, `APPEND` (with UIDPLUS
//! results) and folder `CREATE` / `RENAME` / `DELETE`.

use std::borrow::Cow;
use std::time::Duration;

use imap_proto::Response;
use imap_proto::types::{MailboxDatum, ResponseCode};
use nail_api::FlagChange;

use crate::changes::{FlagUpdate, flag_update};
use crate::client::{Completion, ImapClient, Part, expand_uid_set, quote, sorted_uids, uid_set};
use crate::error::{ImapError, Result};
use crate::flags::{Flag, flag_list};
use crate::transport::with_timeout;

/// UIDs per write command, to keep command lines short for sparse sets.
const WRITE_CHUNK: usize = 500;
/// Assumed worst-case upload speed for the `APPEND` deadline.
const MIN_UPLOAD_BYTES_PER_SEC: u64 = 16 * 1024;

/// How [`ImapClient::store_flags`] changes the flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreMode {
    /// `+FLAGS`: add the flags.
    Add,
    /// `-FLAGS`: remove the flags.
    Remove,
    /// `FLAGS`: replace all flags.
    Replace,
}

/// Result of a flag change.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreResult {
    /// The flags the server reported after the change (usually one entry
    /// per changed message; servers may omit unchanged ones).
    pub updated: Vec<FlagUpdate>,
    /// UIDs left unchanged because they were modified after
    /// `unchanged_since` (RFC 7162 `MODIFIED`): the caller has to re-read
    /// them and decide again.
    pub conflicts: Vec<u32>,
}

/// Result of a copy.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CopyResult {
    /// UIDVALIDITY of the target mailbox (UIDPLUS `COPYUID`).
    pub uid_validity: Option<u32>,
    /// `(source UID, target UID)` pairs (UIDPLUS); empty when the server
    /// does not report them.
    pub mapping: Vec<(u32, u32)>,
}

/// How [`ImapClient::move_messages`] moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveMethod {
    /// `UID MOVE` (RFC 6851).
    Move,
    /// `UID COPY`, `\Deleted`, then an expunge of exactly the moved
    /// messages (see [`ExpungeMethod`]).
    CopyAndExpunge,
    /// `UID COPY` and `\Deleted`, but not expunged: without UIDPLUS the only
    /// expunge would also remove other messages marked `\Deleted`.
    CopyAndMarkDeleted,
}

/// Result of a move.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveResult {
    /// The method used.
    pub method: MoveMethod,
    /// UIDVALIDITY of the target mailbox and the UID mapping, when the
    /// server reported them.
    pub copy: CopyResult,
}

/// How [`ImapClient::expunge_messages`] removed the messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpungeMethod {
    /// `UID EXPUNGE` (UIDPLUS): exactly these messages.
    UidExpunge,
    /// `EXPUNGE`: no other message was marked `\Deleted`, so a plain
    /// expunge removed exactly these.
    Expunge,
    /// Only marked `\Deleted`: other messages carry `\Deleted` too, and
    /// without UIDPLUS they would be expunged as well.
    MarkedOnly,
}

/// Result of an `APPEND`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AppendResult {
    /// UIDVALIDITY of the mailbox (UIDPLUS `APPENDUID`).
    pub uid_validity: Option<u32>,
    /// UID of the new message (UIDPLUS `APPENDUID`).
    pub uid: Option<u32>,
}

impl ImapClient {
    /// Changes flags of messages in `path` with `UID STORE`. With
    /// `unchanged_since` (CONDSTORE), messages changed after that
    /// mod-sequence are left alone and reported in
    /// [`StoreResult::conflicts`].
    pub async fn store_flags(
        &mut self,
        path: &str,
        uids: &[u32],
        mode: StoreMode,
        flags: &[Flag],
        unchanged_since: Option<u64>,
    ) -> Result<StoreResult> {
        let uids = sorted_uids(uids)?;
        let list = flag_list(flags)?;
        if unchanged_since.is_some() && !self.capabilities.has("CONDSTORE") {
            return Err(ImapError::Unsupported(
                "UNCHANGEDSINCE needs CONDSTORE".to_owned(),
            ));
        }
        self.ensure_selected(path, true).await?;
        let item = match mode {
            StoreMode::Add => "+FLAGS",
            StoreMode::Remove => "-FLAGS",
            StoreMode::Replace => "FLAGS",
        };
        let modifier =
            unchanged_since.map_or_else(String::new, |m| format!(" (UNCHANGEDSINCE {m})"));
        let mut result = StoreResult::default();
        for chunk in uids.chunks(WRITE_CHUNK) {
            let command = format!("UID STORE {}{modifier} {item} {list}", uid_set(chunk));
            let mut updated = Vec::new();
            let completion = self
                .command(&command, |response| {
                    if let Response::Fetch(_, attributes) = response
                        && let Some(update) = flag_update(attributes)
                        && chunk.binary_search(&update.uid).is_ok()
                    {
                        updated.push(update);
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await?;
            result.updated.extend(updated);
            if let Some(modified) = modified_set(&completion.text) {
                result.conflicts.extend(modified);
            }
        }
        Ok(result)
    }

    /// Applies an API [`FlagChange`] (absolute system flag states, tags to
    /// add and remove) with at most two `UID STORE` commands. Tags must be
    /// valid IMAP keywords.
    pub async fn apply_flag_change(
        &mut self,
        path: &str,
        uids: &[u32],
        change: &FlagChange,
    ) -> Result<StoreResult> {
        let mut add = Vec::new();
        let mut remove = Vec::new();
        for (state, flag) in [
            (change.seen, Flag::Seen),
            (change.flagged, Flag::Flagged),
            (change.answered, Flag::Answered),
            (change.draft, Flag::Draft),
        ] {
            match state {
                Some(true) => add.push(flag),
                Some(false) => remove.push(flag),
                None => {}
            }
        }
        for tag in &change.add_tags {
            add.push(Flag::keyword(tag.clone())?);
        }
        for tag in &change.remove_tags {
            remove.push(Flag::keyword(tag.clone())?);
        }
        let mut result = StoreResult::default();
        for (mode, flags) in [(StoreMode::Add, add), (StoreMode::Remove, remove)] {
            if flags.is_empty() {
                continue;
            }
            let step = self.store_flags(path, uids, mode, &flags, None).await?;
            // Later reports supersede earlier ones for the same message.
            result
                .updated
                .retain(|u| !step.updated.iter().any(|s| s.uid == u.uid));
            result.updated.extend(step.updated);
            result.conflicts.extend(step.conflicts);
        }
        Ok(result)
    }

    /// Copies messages from `from` to `to` (`UID COPY`).
    pub async fn copy_messages(
        &mut self,
        from: &str,
        uids: &[u32],
        to: &str,
    ) -> Result<CopyResult> {
        let uids = sorted_uids(uids)?;
        self.ensure_selected(from, false).await?;
        let target = quote(to)?;
        let mut result = CopyResult::default();
        for chunk in uids.chunks(WRITE_CHUNK) {
            let completion = self
                .command(&format!("UID COPY {} {target}", uid_set(chunk)), |_| {
                    Ok(false)
                })
                .await?;
            merge_copyuid(&mut result, copyuid(completion.code.as_ref()));
        }
        Ok(result)
    }

    /// Moves messages from `from` to `to`: `UID MOVE` when the server has
    /// MOVE, else `UID COPY` + `\Deleted` + an expunge of exactly these
    /// messages (see [`MoveMethod`]).
    pub async fn move_messages(
        &mut self,
        from: &str,
        uids: &[u32],
        to: &str,
    ) -> Result<MoveResult> {
        let uids = sorted_uids(uids)?;
        if !self.capabilities.has("MOVE") {
            let copy = self.copy_messages(from, &uids, to).await?;
            let method = match self.expunge_messages(from, &uids).await? {
                ExpungeMethod::MarkedOnly => MoveMethod::CopyAndMarkDeleted,
                ExpungeMethod::UidExpunge | ExpungeMethod::Expunge => MoveMethod::CopyAndExpunge,
            };
            return Ok(MoveResult { method, copy });
        }
        self.ensure_selected(from, true).await?;
        let target = quote(to)?;
        let mut copy = CopyResult::default();
        for chunk in uids.chunks(WRITE_CHUNK) {
            let mut untagged = None;
            let completion = self
                .command(
                    &format!("UID MOVE {} {target}", uid_set(chunk)),
                    |response| {
                        match response {
                            // RFC 6851: COPYUID arrives in an untagged OK.
                            Response::Data {
                                code: Some(code @ ResponseCode::CopyUid(..)),
                                ..
                            } => untagged = copyuid(Some(code)),
                            // Our own expunges are not news for the caller.
                            Response::Expunge(_) | Response::Vanished { .. } => {}
                            _ => return Ok(false),
                        }
                        Ok(true)
                    },
                )
                .await?;
            merge_copyuid(
                &mut copy,
                untagged.or_else(|| copyuid(completion.code.as_ref())),
            );
        }
        Ok(MoveResult {
            method: MoveMethod::Move,
            copy,
        })
    }

    /// Permanently removes messages from `path`: marks them `\Deleted`,
    /// then `UID EXPUNGE` (UIDPLUS). Without UIDPLUS a plain `EXPUNGE` is
    /// used only when no other message is marked `\Deleted`; otherwise the
    /// messages stay marked ([`ExpungeMethod::MarkedOnly`]).
    pub async fn expunge_messages(&mut self, path: &str, uids: &[u32]) -> Result<ExpungeMethod> {
        let uids = sorted_uids(uids)?;
        self.ensure_selected(path, true).await?;
        for chunk in uids.chunks(WRITE_CHUNK) {
            self.command(
                &format!("UID STORE {} +FLAGS.SILENT (\\Deleted)", uid_set(chunk)),
                |response| Ok(is_own_fetch(response, chunk)),
            )
            .await?;
        }
        if self.capabilities.has("UIDPLUS") {
            for chunk in uids.chunks(WRITE_CHUNK) {
                self.command(&format!("UID EXPUNGE {}", uid_set(chunk)), |response| {
                    Ok(matches!(
                        response,
                        Response::Expunge(_) | Response::Vanished { .. }
                    ))
                })
                .await?;
            }
            return Ok(ExpungeMethod::UidExpunge);
        }
        let others = self
            .search("DELETED", |uid| uids.binary_search(&uid).is_err())
            .await?;
        if !others.is_empty() {
            tracing::debug!(
                others = others.len(),
                "not expunging: other messages are marked \\Deleted and UIDPLUS is missing"
            );
            return Ok(ExpungeMethod::MarkedOnly);
        }
        self.command("EXPUNGE", |response| {
            Ok(matches!(
                response,
                Response::Expunge(_) | Response::Vanished { .. }
            ))
        })
        .await?;
        Ok(ExpungeMethod::Expunge)
    }

    /// Appends a message to `path` (drafts, sent copies). `internal_date`
    /// is an IMAP date-time such as `28-Sep-2026 08:15:00 +0200`. Returns
    /// the new UID when the server supports UIDPLUS.
    ///
    /// Fails with [`ImapError::TooLarge`] when the message exceeds the
    /// server's `APPENDLIMIT` (checked before uploading) or the server
    /// answers `[TOOBIG]`, and with [`ImapError::No`] (`TRYCREATE`) when the
    /// mailbox does not exist.
    pub async fn append(
        &mut self,
        path: &str,
        raw: &[u8],
        flags: &[Flag],
        internal_date: Option<&str>,
    ) -> Result<AppendResult> {
        if let Some(limit) = self.append_limit()
            && raw.len() as u64 > limit
        {
            return Err(ImapError::TooLarge(format!(
                "{} bytes exceed the server's APPENDLIMIT of {limit}",
                raw.len()
            )));
        }
        let mut command = format!("APPEND {} {}", quote(path)?, flag_list(flags)?);
        if let Some(date) = internal_date {
            if !is_valid_date_time(date) {
                return Err(ImapError::InvalidArgument(format!(
                    "{date:?} is not an IMAP date-time"
                )));
            }
            command.push_str(&format!(" \"{date}\""));
        }
        command.push(' ');
        let parts = [Part::Text(Cow::Owned(command)), Part::Literal(raw)];
        let deadline =
            self.config.timeout + Duration::from_secs(raw.len() as u64 / MIN_UPLOAD_BYTES_PER_SEC);
        let mut on = |_: &Response<'static>| Ok(false);
        let result: Result<Completion> =
            with_timeout(deadline, self.run_parts(&parts, &mut on, false)).await;
        if matches!(result, Err(ImapError::Timeout(_))) {
            self.broken = true;
        }
        let completion = result.map_err(|err| match err {
            ImapError::No(text)
                if crate::error::response_code(&text)
                    .is_some_and(|c| c.eq_ignore_ascii_case("TOOBIG")) =>
            {
                ImapError::TooLarge(text)
            }
            other => other,
        })?;
        Ok(match completion.code {
            Some(ResponseCode::AppendUid(uid_validity, uids)) => AppendResult {
                uid_validity: Some(uid_validity),
                uid: expand_uid_set(&uids).first().copied(),
            },
            _ => AppendResult::default(),
        })
    }

    /// The server's `APPENDLIMIT` (RFC 7889), if it announces one.
    pub fn append_limit(&self) -> Option<u64> {
        self.capabilities
            .iter()
            .find_map(|cap| cap.strip_prefix("APPENDLIMIT="))
            .and_then(|limit| limit.parse().ok())
    }

    /// Creates a mailbox (`CREATE`). `path` is the server path (see
    /// [`crate::folder_path`] for building one from a display name). Fails
    /// with `ALREADYEXISTS` ([`nail_api::ErrorCode::Conflict`]) when taken.
    pub async fn create_folder(&mut self, path: &str) -> Result<()> {
        self.command(&format!("CREATE {}", quote(path)?), |_| Ok(false))
            .await
            .map(drop)
    }

    /// Renames a mailbox and its children (`RENAME`).
    pub async fn rename_folder(&mut self, from: &str, to: &str) -> Result<()> {
        self.leave_if_selected(from).await?;
        self.command(&format!("RENAME {} {}", quote(from)?, quote(to)?), |_| {
            Ok(false)
        })
        .await
        .map(drop)
    }

    /// Deletes a mailbox (`DELETE`).
    pub async fn delete_folder(&mut self, path: &str) -> Result<()> {
        self.leave_if_selected(path).await?;
        self.command(&format!("DELETE {}", quote(path)?), |_| Ok(false))
            .await
            .map(drop)
    }

    /// Subscribes to (or unsubscribes from) a mailbox.
    pub async fn set_subscribed(&mut self, path: &str, subscribed: bool) -> Result<()> {
        let verb = if subscribed {
            "SUBSCRIBE"
        } else {
            "UNSUBSCRIBE"
        };
        self.command(&format!("{verb} {}", quote(path)?), |_| Ok(false))
            .await
            .map(drop)
    }

    /// Closes the selection of `path` without expunging, before it is
    /// renamed or deleted.
    async fn leave_if_selected(&mut self, path: &str) -> Result<()> {
        if self.selected.as_ref().is_some_and(|s| s.path == path) {
            if self.capabilities.has("UNSELECT") {
                self.command("UNSELECT", |_| Ok(false)).await?;
                self.selected = None;
            } else {
                // EXAMINE + CLOSE never expunges (the mailbox is read-only).
                self.examine(path).await?;
                self.command("CLOSE", |_| Ok(false)).await?;
                self.selected = None;
            }
            self.changes.clear();
        }
        Ok(())
    }
}

/// Whether `response` is a `FETCH` for one of `uids` (our own STORE echo).
fn is_own_fetch(response: &Response<'static>, uids: &[u32]) -> bool {
    matches!(response, Response::Fetch(_, attributes)
        if flag_update(attributes).is_some_and(|u| uids.binary_search(&u.uid).is_ok()))
        || matches!(response, Response::MailboxData(MailboxDatum::Flags(_)))
}

/// UIDVALIDITY, source and target UIDs of a `COPYUID` code.
fn copyuid(code: Option<&ResponseCode<'_>>) -> Option<(u32, Vec<u32>, Vec<u32>)> {
    match code {
        Some(ResponseCode::CopyUid(uid_validity, sources, targets)) => Some((
            *uid_validity,
            expand_uid_set(sources),
            expand_uid_set(targets),
        )),
        _ => None,
    }
}

fn merge_copyuid(result: &mut CopyResult, copyuid: Option<(u32, Vec<u32>, Vec<u32>)>) {
    if let Some((uid_validity, sources, targets)) = copyuid {
        result.uid_validity = Some(uid_validity);
        if sources.len() == targets.len() {
            result.mapping.extend(sources.into_iter().zip(targets));
        } else {
            tracing::warn!("COPYUID with sets of different sizes ignored");
        }
    }
}

/// The UIDs of a `[MODIFIED <set>]` response code.
fn modified_set(text: &str) -> Option<Vec<u32>> {
    let rest = text.trim_start().strip_prefix('[')?;
    let (code, rest) = rest.split_once(' ')?;
    if !code.eq_ignore_ascii_case("MODIFIED") {
        return None;
    }
    let set = &rest[..rest.find(']')?];
    parse_number_set(set)
}

/// Parses a sequence set without `*` (`1:3,7`).
fn parse_number_set(text: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for part in text.split(',') {
        match part.split_once(':') {
            Some((a, b)) => {
                let (a, b): (u32, u32) = (a.parse().ok()?, b.parse().ok()?);
                out.extend(a.min(b)..=a.max(b));
            }
            None => out.push(part.parse().ok()?),
        }
    }
    Some(out)
}

/// `dd-Mon-yyyy hh:mm:ss +zzzz` (day may be space-padded).
fn is_valid_date_time(value: &str) -> bool {
    value.len() == 26
        && value.is_ascii()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b" -:+".contains(&b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modified_codes() {
        assert_eq!(
            modified_set("[MODIFIED 3:5,9] conditional STORE failed"),
            Some(vec![3, 4, 5, 9])
        );
        assert_eq!(modified_set("STORE completed"), None);
        assert_eq!(modified_set("[READ-WRITE] x"), None);
        assert_eq!(parse_number_set("7"), Some(vec![7]));
        assert_eq!(parse_number_set("x"), None);
    }

    #[test]
    fn copyuid_mapping() {
        use imap_proto::types::UidSetMember;
        let mut result = CopyResult::default();
        let code = ResponseCode::CopyUid(
            9,
            vec![UidSetMember::UidRange(1..=2), UidSetMember::Uid(5)],
            vec![UidSetMember::UidRange(10..=12)],
        );
        merge_copyuid(&mut result, copyuid(Some(&code)));
        assert_eq!(result.uid_validity, Some(9));
        assert_eq!(result.mapping, [(1, 10), (2, 11), (5, 12)]);
        let bad = ResponseCode::CopyUid(9, vec![UidSetMember::Uid(1)], vec![]);
        merge_copyuid(&mut result, copyuid(Some(&bad)));
        assert_eq!(copyuid(Some(&ResponseCode::ReadOnly)), None);
        assert_eq!(result.mapping.len(), 3);
    }

    #[test]
    fn date_times() {
        assert!(is_valid_date_time("28-Sep-2026 08:15:00 +0200"));
        assert!(is_valid_date_time(" 1-Sep-2026 08:15:00 -0000"));
        assert!(!is_valid_date_time("2026-09-28T08:15:00Z"));
        assert!(!is_valid_date_time("28-Sep-2026 08:15:00 +0200\""));
    }
}
