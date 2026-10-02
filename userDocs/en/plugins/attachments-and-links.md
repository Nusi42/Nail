# Attachments and links

With the `attachments` permission a plug-in can render **previews** of the
attachment types it names and add **actions** to the attachment menu
("Save to …", "Open in …"). **Link actions** in the link menu need no
permission.

```json
"permissions": ["attachments"],
"contributes": {
  "attachmentPreviews": [
    { "id": "table", "title": "%table%", "mimeTypes": ["text/csv"], "extensions": ["csv", "tsv"], "maxBytes": 2097152 }
  ],
  "attachmentActions": [{ "id": "save-crm", "title": "%save-crm%", "mimeTypes": ["application/pdf"] }],
  "linkActions": [{ "id": "open-ticket", "title": "%open-ticket%", "hosts": ["jira.acme.example"] }]
}
```

MIME types are lower case (`text/csv`, `image/*`); extensions are lower
case without the dot. A preview or attachment action applies when either
the type or the extension matches; an attachment action with neither
applies to every attachment, a link action without `hosts` to every link.

The template `node packages/create-nail-plugin/index.mjs my-plugin --template attachment`
starts an attachment plug-in; `examples/csv-preview` is a complete one.

## Previews

When the user previews an attachment the renderer matches, Nail calls it
with the attachment (`PluginAttachmentRef`: message, index, name, MIME
type, size) and its **bytes** as a `Uint8Array` — at most `maxBytes`
(default and limit 10 MB; larger attachments are not previewed by it).
The renderer answers either

- **components** — JSX, nodes, or `previewTree(…)`: a tree drawn by Nail
  like a panel (tables, text, images, …), or
- **an image** — `previewImage("image/png", bytes, alt)`: PNG, JPEG, GIF
  or WebP, at most 5 MB.

Never HTML and never SVG: a preview cannot run code or load anything from
the web. Nail checks the answer like a panel's tree and shows the
plug-in's name with it.

```tsx
import { decodeText, definePlugin } from "@nail/plugin-api";

export default definePlugin({
  attachments: {
    previews: {
      table: (attachment, bytes) => {
        const rows = parse(decodeText(bytes));        // the sandbox has no TextDecoder
        return <table columns={rows[0].map((label) => ({ label }))} rows={rows.slice(1)} />;
      },
    },
  },
});
```

`decodeText` reads UTF-8 (or Windows-1252 when the bytes are not UTF-8);
`decodeUtf8`, `encodeUtf8`, `toBase64` and `fromBase64` are there too.

The content of end-to-end encrypted messages is never given to a plug-in:
their attachments get Nail's own preview only.

## Attachment actions

The handler gets the context (`ctx.attachment`: message, index, name, type,
size) and the attachment's bytes:

```ts
attachments: {
  actions: {
    "save-crm": async (ctx, bytes) => {
      await nail.http.fetch("https://crm.acme.example/upload", { method: "POST", body: bytes });
    },
  },
},
```

(sending the bytes somewhere needs `network:<host>` for that host).

## Link actions

The handler gets `ctx.link`: the URL, the link's text, and the message it
is in when the plug-in may read that message (`mail.read`). Only `http`,
`https` and `mailto` links, and only those whose host matches `hosts`.

```ts
links: {
  "open-ticket": async (ctx) => {
    const key = /\/browse\/([A-Z]+-\d+)/.exec(ctx.link?.url ?? "")?.[1];
    if (key) await nail.ui.showDialog({ title: key, body: <text>…</text> });
  },
},
```

## Testing

```ts
const preview = await host.renderPreview("table", { name: "people.csv", mimeType: "text/csv" }, "a,b\n1,2");
await host.runAttachmentAction("save-crm", { name: "offer.pdf" }, pdfBytes);
await host.runLinkAction("open-ticket", { url: "https://jira.acme.example/browse/ACME-1" });
```

`renderPreview` refuses input larger than the renderer's `maxBytes` and
answers that Nail would refuse (SVG, too large, a broken tree).
