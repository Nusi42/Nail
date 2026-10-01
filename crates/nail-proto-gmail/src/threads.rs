//! `users.threads`: list and get. Gmail thread ids are Nail's thread ids
//! for Gmail messages (ADR 0065), so the engine reports them on every
//! message; these calls serve thread views and repairs.

use nail_api::gmail::LabelChange;
use reqwest::Method;

use crate::client::GmailClient;
use crate::error::GmailError;
use crate::messages::{Format, ListQuery};
use crate::wire::{ListThreadsResponse, ModifyRequest, Thread};

impl GmailClient {
    /// `threads.list`: one page of threads (ids and snippets).
    pub async fn list_threads(&self, query: &ListQuery) -> Result<ListThreadsResponse, GmailError> {
        self.json(
            Method::GET,
            &["threads"],
            &query.params(),
            None::<&()>,
            true,
            "users.threads.list",
        )
        .await
    }

    /// `threads.get`: a thread with its messages in `format` (`minimal`,
    /// `metadata` or `full`; Gmail has no `raw` for threads).
    pub async fn get_thread(&self, id: &str, format: Format) -> Result<Thread, GmailError> {
        let format = if format == Format::Raw {
            Format::Minimal
        } else {
            format
        };
        self.json(
            Method::GET,
            &["threads", id],
            &[("format", format.as_str().to_owned())],
            None::<&()>,
            true,
            "users.threads.get",
        )
        .await
    }

    /// `threads.modify`: adds and removes labels on every message of a
    /// thread.
    pub async fn modify_thread(
        &self,
        id: &str,
        change: &LabelChange,
    ) -> Result<Thread, GmailError> {
        let body = ModifyRequest {
            add_label_ids: change.add.clone(),
            remove_label_ids: change.remove.clone(),
        };
        self.json(
            Method::POST,
            &["threads", id, "modify"],
            &[],
            Some(&body),
            true,
            "users.threads.modify",
        )
        .await
    }
}
