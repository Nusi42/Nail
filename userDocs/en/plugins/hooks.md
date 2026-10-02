# Mail hooks

Mail hooks let a plug-in take part when mail arrives, before it is sent
and when it is opened. A hook is declared in the manifest
(`contributes.hooks`, at most one per kind) and needs its permission; its
handler is registered in code:

```json
"permissions": ["mail.read", "mail.organize", "hooks.arrival", "hooks.beforeSend", "hooks.open"],
"contributes": {
  "hooks": [
    { "hook": "arrival", "filter": "from:@acme.example" },
    { "hook": "beforeSend", "timeoutMs": 1000 },
    { "hook": "open", "filter": "has:attachment" }
  ]
}
```

```ts
import { banner, definePlugin, veto } from "@nail/plugin-api";

export default definePlugin({
  hooks: {
    arrival: ({ message }) => ({ actions: [{ type: "addTag", tag: "acme" }] }),
    beforeSend: (message) => (message.subject ? null : veto("The subject is empty.")),
    open: ({ message }) => ({ banners: [banner("crm", "This customer has an open ticket.")] }),
    onBannerAction: ({ messageId, bannerId, buttonId }) => { /* … */ },
  },
});
```

(or `ctx.hooks.register("arrival", handler)` in `activate`).

**A hook never blocks mail.** Every call has a time budget: 2 s on
arrival, 3 s before send, 1 s on open by default; `timeoutMs` in the
manifest may ask for 50 ms to 10 s. A hook that throws, answers something
invalid or runs out of time is skipped and counted in the plug-in
manager's **Activity** ("Failed", "Too slow"); the mail arrives, the
message is sent, the reader opens. A hook that loops is also stopped by
the CPU budget, which stops the plug-in.

Hooks are only called for mail of the accounts the grant covers.
Answering nothing (`null` / `undefined`) is fine.

## On arrival

`hooks.arrival` (needs `mail.read`) is called for each new message that
matches the hook's `filter` (Nail's [search language](../search.md), like a
rule's condition; without a filter, every new message), after your own
arrival rules. It gets `{ message, body? }` — the headers, and the body
when the plug-in also has `mail.read.body` for the account (never for
end-to-end encrypted mail).

It may answer **actions**, applied in order like a rule's actions through
the operation queue (undoable): `addTag`, `removeTag`, `setFlagged`,
`setRead`, `move` (a folder of the same account), `archive` — at most 20.
Actions need `mail.organize` (and follow its confirmations); there is no
delete action, and a `move` into Trash or Junk counts as one: such an
answer is refused (the plug-in's log says so). An answer with actions but
without `mail.organize` is skipped.

```ts
arrival: ({ message, body }) =>
  body?.text.includes("invoice") ? { actions: [{ type: "addTag", tag: "invoice" }] } : null,
```

For work that takes longer than a hook's budget, use a
[background job](background-jobs.md) on `message.received` instead.

## Before send

`hooks.beforeSend` sees every outgoing message (`PluginOutgoingMessage`:
sender, recipients, subject, the text, the HTML, attachment names and
sizes, the replied-to message, whether it will be encrypted) before it is
queued. A message that will be **end-to-end encrypted** comes without its
content: `text` is empty and `html` absent (plug-ins never see encrypted
content). It may answer:

- `veto: { reason }` — stop sending. Nail shows the reason **with the
  plug-in's name** and offers **Keep editing** and **Send anyway**, which
  sends the message as it was. Use `veto(reason)`.
- `headers` — up to 10 `X-` headers (one line each; never `X-Nail-…`).
- `addContent` — text (and HTML, sanitized) added at the `top` or `bottom`
  (after the signature, before the quoted original), 64 KB in all. Use
  `addContent(text, { position, html })`.

Hooks cannot change whether a message is signed or encrypted; content
added to an encrypted message is encrypted with it. When an AI agent's
`send_message` is vetoed, the agent is told and the user sends from Nail.

Write veto reasons as a sentence the user understands, in their language
(`nail.l10n.t`), saying what to do: "You mention “attached”, but nothing
is attached. Attach the file, or send the message anyway."

## On open

`hooks.open` (needs `mail.read`) is called when the user opens a message
that matches the hook's `filter`. It gets `{ message, body? }` like the
arrival hook and answers up to 3 **banners** and 3 **badges**:

- a banner (`banner(id, text, { level, buttons })`) is a line above the
  message with up to 3 buttons; pressing one calls the
  `onBannerAction` handler with `{ messageId, bannerId, buttonId }`;
- a badge (`badge(text, { level, icon, tooltip })`) is a short label (at
  most 32 characters) in the message header.

Nail names the plug-in next to every banner and badge. The message itself
is never changed. Keep open hooks fast: one that is too slow simply shows
nothing.

## Testing hooks

The SDK's test host runs hooks with the same budgets and checks:

```ts
const host = createTestHost({ manifest, catalogs: loadCatalogs(dir) });
const restore = host.install();
await plugin.activate(host.activationContext());
const run = await host.runBeforeSend(outgoingMessage);
expect(run.outcome).toBe("vetoed");          // ok | vetoed | failed | timed-out | skipped
await host.runArrival(message);              // valid actions are applied to host.messages
await host.runOpen(message);
restore();
```

The test host does not evaluate `filter`; Nail does.
