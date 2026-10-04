//! HTTP batch requests (`POST /batch/gmail/v1`, `multipart/mixed`): many
//! `messages.get` calls in one round trip. Each part is answered on its
//! own; parts that hit a rate limit or a server error are retried in a
//! later batch with backoff, the rest keep their answer.

use std::fmt::Write as _;
use std::time::Duration;

use reqwest::Method;

use crate::client::{Body, Call, GmailClient, parse_json, parse_retry_after};
use crate::error::GmailError;
use crate::messages::{Format, boundary_for};
use crate::wire::{ErrorBody, Message};

/// Most parts Gmail accepts in one batch.
pub const MAX_BATCH: usize = 100;

/// Parts per batch Nail sends (Google advises at most 50 against
/// per-user rate limits).
pub const DEFAULT_BATCH: usize = 50;

/// One answered part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PartReply {
    /// The request index from the `Content-ID`, if it had one.
    pub index: Option<usize>,
    /// HTTP status.
    pub status: u16,
    /// `Retry-After` in seconds.
    pub retry_after: Option<u32>,
    /// Body.
    pub body: String,
}

/// Whether a Gmail id may go into a batch path as is.
pub(crate) fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Builds the batch body for GET requests of `paths` (`(index, path and
/// query)`).
pub(crate) fn build_batch(boundary: &str, paths: &[(usize, String)]) -> Vec<u8> {
    let mut out = String::new();
    for (index, path) in paths {
        let _ = write!(
            out,
            "--{boundary}\r\nContent-Type: application/http\r\nContent-ID: <item-{index}>\r\n\r\nGET {path} HTTP/1.1\r\n\r\n"
        );
    }
    let _ = write!(out, "--{boundary}--\r\n");
    out.into_bytes()
}

/// The `boundary` parameter of a `multipart/*` content type.
pub(crate) fn boundary_of(content_type: &str) -> Option<String> {
    content_type.split(';').skip(1).find_map(|param| {
        let (name, value) = param.split_once('=')?;
        name.trim()
            .eq_ignore_ascii_case("boundary")
            .then(|| value.trim().trim_matches('"').to_owned())
    })
}

fn split_head(text: &str) -> (&str, &str) {
    if let Some(at) = text.find("\r\n\r\n") {
        (&text[..at], &text[at + 4..])
    } else if let Some(at) = text.find("\n\n") {
        (&text[..at], &text[at + 2..])
    } else {
        (text, "")
    }
}

fn header<'a>(head: &'a str, name: &str) -> Option<&'a str> {
    head.lines().find_map(|line| {
        let (n, v) = line.split_once(':')?;
        n.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Parses a batch answer into its parts.
pub(crate) fn parse_batch(content_type: &str, body: &[u8]) -> Result<Vec<PartReply>, GmailError> {
    let boundary = boundary_of(content_type).ok_or_else(|| {
        GmailError::Protocol(format!("batch answer without boundary: {content_type}"))
    })?;
    let text = std::str::from_utf8(body)
        .map_err(|_| GmailError::Protocol("batch answer is not UTF-8".into()))?;
    let delimiter = format!("--{boundary}");
    let mut out = Vec::new();
    for chunk in text.split(delimiter.as_str()).skip(1) {
        if chunk.starts_with("--") {
            break;
        }
        let chunk = chunk.trim_start_matches(['\r', '\n']);
        let (part_head, http) = split_head(chunk);
        let index = header(part_head, "content-id").and_then(|id| {
            id.trim_matches(['<', '>'])
                .rsplit_once("item-")
                .and_then(|(_, n)| n.parse().ok())
        });
        let (http_head, http_body) = split_head(http);
        let status_line = http_head.lines().next().unwrap_or_default();
        let status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|s| s.parse::<u16>().ok())
            .ok_or_else(|| {
                GmailError::Protocol(format!("bad batch part status {status_line:?}"))
            })?;
        let retry_after =
            header(http_head, "retry-after").and_then(|v| parse_retry_after(v, chrono::Utc::now()));
        let body = http_body.trim_end_matches(['\r', '\n']).to_owned();
        out.push(PartReply {
            index,
            status,
            retry_after,
            body,
        });
    }
    Ok(out)
}

impl GmailClient {
    /// `messages.get` for many ids through batch requests of at most
    /// `batch_size` parts (clamped to 1–[`MAX_BATCH`]). The answer has one
    /// entry per id, in order: the message or that message's error
    /// (`NotFound` for a message deleted meanwhile). Parts hit by rate
    /// limits or server errors are retried with backoff; the whole call
    /// fails only when a batch request itself fails.
    pub async fn batch_get(
        &self,
        ids: &[String],
        format: Format,
        batch_size: usize,
    ) -> Result<Vec<Result<Message, GmailError>>, GmailError> {
        let batch_size = batch_size.clamp(1, MAX_BATCH);
        let mut results: Vec<Option<Result<Message, GmailError>>> = vec![None; ids.len()];
        let mut pending: Vec<usize> = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            if is_plain_id(id) {
                pending.push(i);
            } else {
                results[i] = Some(Err(GmailError::NotFound(format!("message id {id:?}"))));
            }
        }
        let policy = self.retry_policy();
        let mut attempt = 0u32;
        while !pending.is_empty() {
            let mut retry: Vec<(usize, GmailError)> = Vec::new();
            let mut wait: Option<u32> = None;
            for chunk in pending.chunks(batch_size) {
                let paths: Vec<(usize, String)> = chunk
                    .iter()
                    .map(|&i| {
                        let url = self.url(
                            &["messages", ids[i].as_str()],
                            &[("format", format.as_str().to_owned())],
                        );
                        (
                            i,
                            format!("{}?{}", url.path(), url.query().unwrap_or_default()),
                        )
                    })
                    .collect();
                let parts = self.send_batch(&paths).await?;
                let mut answered = vec![false; ids.len()];
                for (position, part) in parts.into_iter().enumerate() {
                    let Some(index) = part.index.or_else(|| chunk.get(position).copied()) else {
                        continue;
                    };
                    if !chunk.contains(&index) || answered[index] {
                        continue;
                    }
                    answered[index] = true;
                    if (200..300).contains(&part.status) {
                        results[index] =
                            Some(parse_json(part.body.as_bytes(), "batch messages.get"));
                        continue;
                    }
                    let error_body: Option<ErrorBody> = serde_json::from_str(&part.body).ok();
                    let reason = error_body.as_ref().and_then(ErrorBody::reason);
                    let error = GmailError::from_status(
                        part.status,
                        part.retry_after,
                        error_body.as_ref(),
                        "batch messages.get",
                    );
                    if GmailError::is_transient(part.status, reason) {
                        wait = wait.max(part.retry_after);
                        retry.push((index, error));
                    } else {
                        results[index] = Some(Err(error));
                    }
                }
                for &index in chunk {
                    if !answered[index] {
                        retry.push((
                            index,
                            GmailError::Protocol("batch answer without this part".into()),
                        ));
                    }
                }
            }
            if retry.is_empty() {
                break;
            }
            let delay = wait.map_or_else(
                || policy.backoff(attempt),
                |s| Duration::from_secs(u64::from(s)),
            );
            if attempt >= policy.max_retries || delay > policy.max_inline_wait {
                for (index, error) in retry {
                    results[index] = Some(Err(error));
                }
                break;
            }
            tracing::debug!(parts = retry.len(), ?delay, "retrying batch parts");
            tokio::time::sleep(delay).await;
            attempt += 1;
            pending = retry.into_iter().map(|(i, _)| i).collect();
        }
        Ok(results
            .into_iter()
            .map(|r| r.unwrap_or_else(|| Err(GmailError::Protocol("no batch answer".into()))))
            .collect())
    }

    async fn send_batch(&self, paths: &[(usize, String)]) -> Result<Vec<PartReply>, GmailError> {
        let probe: String = paths.iter().map(|(_, p)| p.as_str()).collect();
        let boundary = boundary_for("nail_batch", probe.as_bytes());
        let call = Call {
            method: Method::POST,
            url: self.batch_url(),
            body: Body::Raw {
                content_type: format!("multipart/mixed; boundary={boundary}"),
                bytes: build_batch(&boundary, paths),
            },
            idempotent: true,
            what: "batch".to_owned(),
        };
        let reply = self.execute(&call).await?;
        let content_type = reply
            .headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        parse_batch(&content_type, &reply.body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_batch_body() {
        let body = build_batch(
            "b1",
            &[
                (0, "/gmail/v1/users/me/messages/a?format=minimal".into()),
                (3, "/gmail/v1/users/me/messages/b?format=minimal".into()),
            ],
        );
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with("--b1\r\nContent-Type: application/http\r\nContent-ID: <item-0>\r\n\r\nGET /gmail/v1/users/me/messages/a?format=minimal HTTP/1.1\r\n\r\n"));
        assert!(text.contains("Content-ID: <item-3>"));
        assert!(text.ends_with("--b1--\r\n"));
    }

    #[test]
    fn parses_a_recorded_batch_answer() {
        let body = include_str!("../fixtures/responses/batch-get.txt")
            .replace("\r\n", "\n")
            .replace('\n', "\r\n");
        let parts = parse_batch(
            "multipart/mixed; boundary=batch_HvGbJkY3qG0_AAhC7pLc2dg",
            body.as_bytes(),
        )
        .unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].index, Some(0));
        assert_eq!(parts[0].status, 200);
        let message: Message = serde_json::from_str(&parts[0].body).unwrap();
        assert_eq!(message.id, "18c0a1b2c3d4e5f6");
        assert_eq!(parts[1].status, 404);
        assert_eq!(parts[2].status, 429);
        assert_eq!(parts[2].retry_after, Some(1));
        assert_eq!(parts[2].index, Some(2));
    }

    #[test]
    fn boundaries_and_ids() {
        assert_eq!(
            boundary_of("multipart/mixed; boundary=\"abc\"").as_deref(),
            Some("abc")
        );
        assert_eq!(
            boundary_of("multipart/mixed; charset=x; BOUNDARY=z").as_deref(),
            Some("z")
        );
        assert_eq!(boundary_of("application/json"), None);
        assert!(is_plain_id("18c0a1b2c3d4e5f6"));
        assert!(is_plain_id("r-123_x"));
        assert!(!is_plain_id("a/b"));
        assert!(!is_plain_id("a b"));
        assert!(!is_plain_id(""));
        assert!(parse_batch("multipart/mixed", b"").is_err());
        assert!(
            parse_batch(
                "multipart/mixed; boundary=x",
                b"--x\r\nContent-ID: <response-item-0>\r\n\r\nnonsense\r\n--x--"
            )
            .is_err()
        );
    }
}
