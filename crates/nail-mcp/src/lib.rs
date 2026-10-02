//! Nail's MCP server, read-only (M1b WP23; design.md, "MCP server"; ADR
//! 0025).
//!
//! * [`McpServer`] serves Streamable HTTP on `127.0.0.1` with per-client
//!   bearer tokens (only their SHA-256 is stored, in `nail-store`).
//! * Resources: `nail://accounts`, `nail://accounts/{id}/folders`,
//!   `nail://folders/{id}/messages`, `nail://messages/{id}` (+ `/raw`,
//!   `/attachments/{n}`), `nail://threads/{id}`, `nail://rules`,
//!   `nail://ui/context` ([`uri`]); read tools `search_messages`,
//!   `get_message`, `get_thread`, `list_folders`, `get_ui_context`
//!   ([`tools`]).
//! * Subscriptions (legacy `resources/subscribe` and
//!   `subscriptions/listen`): new mail updates the account's folders and
//!   the folder's messages; a new UI report updates `nail://ui/context`.
//! * Grants ([`access`]): nothing until granted; per-account scope and
//!   folder exclusions; separate scopes for raw source, attachments,
//!   encrypted content and the UI context. Checked on every call, with the
//!   grant read fresh from the store.
//! * One audit entry per call; content from the mailbox is marked as
//!   untrusted data ([`content`]).
//! * [`admin`] answers the Client API requests of the settings UI;
//!   [`bridge`] is `nail mcp --client <name>` (stdio ↔ loopback HTTP).
//! * M2c (WP53): a fixed port from the MCP settings ([`configured_port`]);
//!   user-facing texts as Fluent ids ([`messages`]: audit details, start
//!   and bridge errors, admin errors). Agents keep reading English.
//! * M4a (WP101, ADR 0079): the write tools ([`writes`]: grant, idempotency
//!   keys, rate limits ([`limits`]), action policies and confirmations
//!   through a [`broker::ConfirmationBroker`] — Nail's confirmation center
//!   always, MCP elicitation where the client supports it — and audit
//!   entries with before / after lines), `test_rule`, `edit_open_draft`
//!   and the four prompts ([`prompts`], templates in `core-mcp.ftl`).
//!
//! Data comes through the [`McpData`] trait, which the core implements
//! (usually via [`CoreData`]) and tests fake.

#![warn(missing_docs)]

pub mod access;
pub mod admin;
pub mod bridge;
pub mod broker;
pub mod content;
pub mod core_data;
pub mod data;
pub mod endpoint;
pub mod http;
pub mod limits;
pub mod messages;
pub mod pim;
mod prompts;
pub mod server;
mod subscriptions;
pub mod tools;
pub mod uri;
mod writes;

pub use core_data::{CoreAccess, CoreData};
pub use data::{
    AttachmentData, DataResult, McpData, MessagePage, PluginTool, SearchOutcome, ThreadData,
};
pub use http::{McpServer, ServerOptions, configured_port};
pub use server::{AuthedClient, ClientSource, NailMcp};
pub use writes::WriteSettings;
pub use writes::actor::{ActorWrite, ActorWriteRequest, ActorWrites, PimDelete, actor_audit_name};
