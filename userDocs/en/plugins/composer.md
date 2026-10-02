# Composer

With the `composer` permission a plug-in can add **composer actions** —
buttons and menu items in the composer that insert or transform text — and
**Markdown blocks** it renders when the user previews or sends a Markdown
message.

```json
"permissions": ["composer"],
"contributes": {
  "composerActions": [
    { "id": "insert-ticket", "title": "%insert-ticket%", "icon": "ticket" },
    { "id": "translate", "title": "%translate%", "description": "%translate-tooltip%" }
  ],
  "markdownBlocks": [{ "id": "ticket", "title": "%ticket-block%", "template": "ACME-1234" }]
}
```

The template `node packages/create-nail-plugin/index.mjs my-plugin --template composer`
starts a composer plug-in.

## Composer actions

The composer shows the actions in its toolbar and menu (the layout decides
where). When the user picks one, the handler gets a `PluginActionContext`
whose `composer` says which draft (`draftId`), its mode (`plain`, `html`,
`markdown`) and the selection (`text`, `start`, `end`):

```ts
export default definePlugin({
  composer: {
    actions: {
      translate: async (ctx) => {
        const text = ctx.composer?.selection.text ?? "";
        if (!text) return;
        await nail.composer.replaceSelection(await translate(text));
      },
    },
  },
});
```

The edits:

| Call | What it does |
| --- | --- |
| `nail.composer.insert(text, options?)` | Inserts at the cursor |
| `nail.composer.replaceSelection(text, options?)` | Replaces the selection (inserts at the cursor without one) |
| `nail.composer.replaceAll(text, options?)` | Replaces the whole body |

`options.format` is `text` (default), `markdown` or `html` (sanitized, and
converted for plain-text drafts); `options.draftId` names the draft
(default: the draft of the latest composer action). The composer applies
an edit live and the user can undo it like typing.

Edits work only on a draft a composer action of this plug-in ran on, and
only for a short while after it ran: a plug-in cannot type into drafts on
its own. Writing drafts outside the composer is `nail.mail.createDraft` /
`updateDraft` with `mail.send` ([writing data](writing-data.md)).

## Markdown blocks

A Markdown block is a fenced block whose info string is
`<plugin-id>.<block>`:

````markdown
```acme-crm.ticket
ACME-1234
```
````

The composer's block menu inserts one (with the manifest's `template` as
its content). When the user previews the message, and again when it is
sent, Nail asks the plug-in to render it:

```ts
composer: {
  markdownBlocks: {
    ticket: ({ source, purpose }) => ({
      html: `<p><a href="https://crm.acme.example/t/${escape(source)}">Ticket ${escape(source)}</a></p>`,
      text: `Ticket ${source}`,
    }),
  },
},
```

The renderer gets `{ block, source, purpose }` (`purpose` is `preview` or
`send`) and answers `{ html, text }`: the HTML for the HTML part (at most
256 KB, **sanitized like a message body**: no scripts) and the text for
the plain-text part. A renderer that fails or is too slow leaves the block as a code
block, so the message can always be sent. Escape the source yourself when
you put it into HTML.

## Testing

```ts
await host.runComposerAction("translate", { draftId: "d1", selection: { text: "Hallo", start: 0, end: 5 } });
expect(host.composerEdits).toEqual([{ draftId: "d1", edit: { op: "replace-selection", text: "Hello" } }]);
await host.renderMarkdownBlock("ticket", "ACME-1", "send");
```
