# 0217 — MCP drafts by message id, declined forms leave the decision to Nail

Status: accepted (2026-11-19)

## Context

A user report against 0.8.5 (an agent over `nail mcp` on macOS) showed two
problems:

1. `update_draft` and `send_message` took only the `draft-N` id that
   `create_draft` returned. Every read (`search_messages`, `get_message`,
   `get_ui_context`) shows a saved draft only by the message id of its
   server copy, and that id changes with every save. Drafts written in
   Nail's composer, or created in an earlier agent session, could not be
   changed or sent through MCP at all.
2. `send_message` and `delete_messages` were denied at once, recorded as
   `decided_via = elicitation`, and the agent was told "The user denied
   this action in Nail" although nobody saw a question. The client
   declared form elicitation in each request's metadata (a stateless
   2026-07-28 client; the session info rmcp logs is a synthesized default
   without capabilities), received the MRTR `input_required` form, and
   answered *decline* without showing it. ADR 0080 counted *decline* as a
   denial of anything, so the client's fallback decided for the user.

## Decision

- A `draft_id` that does not name a saved draft is taken as a message id.
  The message must be readable under the client's grant (`read`, its
  account and folder, `encrypted` when it is encrypted) and be a draft:
  flagged `\Draft` or in a folder with the Drafts role (the same rule as
  the UI's Continue writing). It resolves through
  `McpData::draft_for_message`, which is `EditDraftMessage`: Nail's own
  server copy answers with its draft, a draft of another client is
  imported into a new saved draft whose server copy it is. Such an import
  happens before the confirmation, like opening the draft in the composer;
  it changes no mail on the server until the draft is saved or sent.
- An elicitation answer of *decline* no longer denies (amends ADR 0080
  item 6). Like *cancel* and errors it leaves the decision to Nail's own
  confirmation, which is always raised and expires as before. Only an
  accepted form with `approve: false` denies. Clients that cannot show a
  form decline it on their own, so *decline* does not show that the user
  answered.
- The denied result's hint no longer names Nail as the place of the
  decision: "The user denied this action; nothing changed."

## Consequences

- An agent can change and send any draft it can read, by the id it sees.
- A user who declines in the agent's own prompt still has to deny (or let
  expire) the request in Nail; the agent's call waits for it. Approving in
  the agent's prompt works as before.
- Not done: reads still do not return a `draft_id` for draft messages; the
  message id is enough now.
