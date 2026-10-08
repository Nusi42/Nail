//! Recorded-style fixtures of the mock (`crates/nail-proto-graph/fixtures/`).
//!
//! * [`DEMO_TENANT`] — the demo tenant: `ada@contoso.example` with mail in
//!   several folders (a Focused / Other split, a flagged message with a
//!   category, a PDF attachment), master categories, an inbox rule,
//!   scheduled automatic replies, a weekly series and a single event, a
//!   meeting invitation from Bob, contacts in two folders, To Do lists with
//!   a checklist; the shared mailbox `support@contoso.example`; a mailbox
//!   the user may not open; rooms in two room lists; Bob's and the rooms'
//!   schedules. Used by this crate's tests, the core's tests and the
//!   demo-multi seed (WP95).
//! * [`RECORDED_ERRORS`] — Graph's error bodies as recorded (throttling,
//!   expired sync state, missing items, access denied, invalid token,
//!   on-premises mailbox, server errors).

/// The demo tenant (JSON; see `Tenant::from_fixture`).
pub const DEMO_TENANT: &str = include_str!("../../fixtures/tenant-demo.json");

/// The shared mailbox of [`DEMO_TENANT`].
pub const DEMO_SHARED_MAILBOX: &str = "support@contoso.example";

/// Recorded error bodies by name (`429`, `410`, `404`, `403`, `401`,
/// `503`, `500`, `mailbox-on-premises`).
pub const RECORDED_ERRORS: &str = include_str!("../../fixtures/responses/errors.json");

/// A recorded `messages/delta` page (initial sync of an inbox).
pub const RECORDED_MESSAGES_DELTA: &str =
    include_str!("../../fixtures/responses/messages-delta.json");

/// A recorded `$batch` answer.
pub const RECORDED_BATCH: &str = include_str!("../../fixtures/responses/batch.json");
