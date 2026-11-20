//! Values for the rules window and the shared pickers (R2, ADR 0222):
//!
//! * `GetMessageFacts`: what one stored message offers a rule condition
//!   (sender, recipients, subject, `List-Id`, account, folder, header
//!   names), for "values from this mail" when the rules window was opened
//!   from a message.
//! * `SuggestValues`: tags in use, and the `List-Id`s and header names of
//!   the newest stored mail ([`VALUE_SCAN`] raw messages), for the tag,
//!   mailing-list and header-name pickers. Folders, accounts and addresses
//!   have their own requests.

use std::collections::HashMap;

use nail_api::rules::{MessageFacts, SuggestKind, ValueSuggestion};
use nail_api::{ApiError, Request, Response};
use nail_rules::MessageData;

use crate::error::Result;
use crate::service::Service;

/// How many of the newest stored raw messages a mailing-list or
/// header-name suggestion reads.
pub(crate) const VALUE_SCAN: u32 = 200;

/// Most values one `SuggestValues` answers with.
pub(crate) const MAX_VALUE_SUGGESTIONS: u32 = 100;

/// The `List-Id`s of a header list (id and phrase).
fn list_ids(headers: &[(String, String)]) -> Option<(String, Option<String>)> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("List-Id"))
        .find_map(|(_, value)| nail_rules::parse_list_id(value))
}

/// Every header name once (first spelling, case-insensitive), in order.
fn header_names(headers: &[(String, String)]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (name, _) in headers {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            names.push(name.clone());
        }
    }
    names
}

/// The facts of a message as rules see it.
pub(crate) fn facts_of(data: &MessageData) -> MessageFacts {
    MessageFacts {
        message_id: data.id.clone(),
        account_id: data.account_id.clone(),
        folder_id: data.folder_id.clone(),
        subject: data.subject.clone(),
        from: data.from.clone(),
        sender: data.sender.clone(),
        to: data.to.clone(),
        cc: data.cc.clone(),
        reply_to: data.reply_to.clone(),
        list_id: list_ids(&data.headers).map(|(id, _)| id),
        header_names: header_names(&data.headers),
    }
}

/// Counts `values` (value, label) case-insensitively, keeps those that
/// start with `prefix`, most used first (then alphabetically), at most
/// `limit`. The first spelling and the first label seen win.
pub(crate) fn rank(
    values: impl IntoIterator<Item = (String, Option<String>)>,
    prefix: &str,
    limit: u32,
) -> Vec<ValueSuggestion> {
    let needle = prefix.trim().to_lowercase();
    let mut counted: HashMap<String, ValueSuggestion> = HashMap::new();
    for (value, label) in values {
        let key = value.to_lowercase();
        if !key.starts_with(&needle) {
            continue;
        }
        let entry = counted.entry(key).or_insert_with(|| ValueSuggestion {
            value,
            label: None,
            count: 0,
        });
        entry.count += 1;
        if entry.label.is_none() {
            entry.label = label;
        }
    }
    let mut out: Vec<ValueSuggestion> = counted.into_values().collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.value.to_lowercase().cmp(&b.value.to_lowercase()))
    });
    out.truncate(limit as usize);
    out
}

impl Service {
    /// The header fields of the newest stored raw messages.
    async fn recent_headers(&self) -> Result<Vec<Vec<(String, String)>>> {
        let store = self.inner.store.clone();
        let blobs = self.inner.blobs.clone();
        tokio::task::spawn_blocking(move || -> Result<_> {
            let hashes = store.recent_blob_hashes(VALUE_SCAN)?;
            Ok(hashes
                .iter()
                .filter_map(|h| blobs.get(h).ok())
                .map(|raw| nail_mime::header_fields(&raw))
                .collect())
        })
        .await?
    }

    async fn suggest_values(
        &self,
        kind: SuggestKind,
        prefix: String,
        limit: u32,
    ) -> Result<Vec<ValueSuggestion>> {
        let limit = limit.min(MAX_VALUE_SUGGESTIONS);
        Ok(match kind {
            SuggestKind::Tag => self
                .inner
                .store
                .call(move |s| s.suggest_tags(&prefix, limit))
                .await?
                .into_iter()
                .map(|(value, count)| ValueSuggestion {
                    value,
                    label: None,
                    count,
                })
                .collect(),
            SuggestKind::MailingList => {
                let headers = self.recent_headers().await?;
                rank(headers.iter().filter_map(|h| list_ids(h)), &prefix, limit)
            }
            SuggestKind::HeaderName => {
                let headers = self.recent_headers().await?;
                let names = headers
                    .iter()
                    .flat_map(|h| header_names(h).into_iter().map(|n| (n, None)));
                rank(names, &prefix, limit)
            }
        })
    }
}

/// Handles `GetMessageFacts` and `SuggestValues`.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    Ok(match request {
        Request::GetMessageFacts { message_id } => {
            let store = service.inner.store.clone();
            let blobs = service.inner.blobs.clone();
            let id = message_id.clone();
            let view = tokio::task::spawn_blocking(move || {
                crate::rules::message_view(&store, &blobs, &id)
            })
            .await?;
            let (data, _) =
                view.ok_or_else(|| ApiError::not_found(format!("message {message_id}")))?;
            Response::MessageFacts {
                facts: Box::new(facts_of(&data)),
            }
        }
        Request::SuggestValues {
            kind,
            prefix,
            limit,
        } => Response::ValueSuggestions {
            values: service.suggest_values(kind, prefix, limit).await?,
        },
        other => {
            return Err(ApiError::internal(format!(
                "{} routed to the values module",
                other.wire_type()
            ))
            .into());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::Address;

    fn h(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    #[test]
    fn facts_name_the_list_and_every_header_once() {
        let mut data = MessageData::new("m1", "a1", "inbox");
        data.subject = "Hello".into();
        data.from = vec![Address {
            name: Some("Ann".into()),
            email: "ann@example.org".into(),
        }];
        data.headers = vec![
            h("From", "Ann <ann@example.org>"),
            h("Received", "a"),
            h("received", "b"),
            h("List-Id", "\"News\" <news.example.org>"),
        ];
        let facts = facts_of(&data);
        assert_eq!(facts.list_id.as_deref(), Some("news.example.org"));
        assert_eq!(facts.header_names, ["From", "Received", "List-Id"]);
        assert_eq!(facts.subject, "Hello");
        assert_eq!(facts.from[0].email, "ann@example.org");
        assert_eq!(facts.folder_id.as_str(), "inbox");
    }

    #[test]
    fn values_rank_by_use_and_filter_by_prefix() {
        let values = [
            ("news.example.org", Some("News")),
            ("dev.example.net", None),
            ("News.example.org", None),
            ("dev.example.net", Some("Dev")),
            ("other.example.com", None),
        ]
        .map(|(v, l)| (v.to_owned(), l.map(str::to_owned)));
        let all = rank(values.clone(), "", 10);
        assert_eq!(
            all.iter()
                .map(|v| (v.value.as_str(), v.count))
                .collect::<Vec<_>>(),
            [
                ("dev.example.net", 2),
                ("news.example.org", 2),
                ("other.example.com", 1)
            ]
        );
        assert_eq!(all[0].label.as_deref(), Some("Dev"));
        assert_eq!(all[1].label.as_deref(), Some("News"));
        assert_eq!(rank(values.clone(), "NE", 10).len(), 1);
        assert_eq!(rank(values, "", 1).len(), 1);
    }
}
