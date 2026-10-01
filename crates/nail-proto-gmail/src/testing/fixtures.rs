//! Recorded-style fixtures of the mock Gmail server (`fixtures/`):
//! mailboxes in the shape `messages.get?format=raw` / `labels.list`
//! answer, and recorded API answers (`fixtures/responses/`) the unit
//! tests parse.

/// The demo mailbox of `ada@gmail.example`: an inbox thread with a sent
/// reply, promotions / social / updates categories, an archived starred
/// message with a user label and a PDF attachment, a receipt with a second
/// user label (stored as recorded base64url), spam, trash, a draft and a
/// chat transcript (which Nail does not sync). History id 4200.
pub const DEMO_MAILBOX: &str = include_str!("../../fixtures/mailbox-demo.json");

/// The message ids of [`DEMO_MAILBOX`] Nail syncs (all but the chat).
pub const DEMO_SYNCED_IDS: [&str; 9] = [
    "18c0000000000001",
    "18c0000000000002",
    "18c0000000000003",
    "18c0000000000004",
    "18c0000000000005",
    "18c0000000000006",
    "18c0000000000007",
    "18c0000000000008",
    "18c0000000000009",
];
