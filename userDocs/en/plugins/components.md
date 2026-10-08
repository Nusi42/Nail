# Component reference

Plug-ins build dialogs, panels and attachment previews from a fixed set of
components. They are
data, not HTML: Nail draws them with its own controls, your theme styles
them (parts `plugin.<plugin-id>.*`), and they look the same on the desktop
and in a browser connected through remote access.

## Writing trees with JSX

With `"jsxImportSource": "@nail/plugin-api"` (the template sets it), JSX
elements are the components:

```tsx
const body = (
  <stack>
    <text style="strong">Remind me about “{message.subject}”</text>
    <date id="due" label="Due" default="tomorrow" required />
    <textarea id="note" label="Note" rows={3} />
  </stack>
);
await nail.ui.showDialog({ title: "Follow up", body, buttons: [{ id: "save", label: "Save", primary: true }] });
```

- Element names are the component types below; attributes are their fields.
- `<text>` and `<markdown>` take their text from their children;
  `<stack>` takes nested elements, and plain strings inside it become
  `<text>` elements. Other components take no children.
- `null`, `undefined`, `false` and empty strings render nothing, so
  `{note && <text>{note}</text>}` works. Arrays are flattened.
- Your own functions can return elements (`function Row(props) { … }`),
  and fragments (`<>…</>`) group several.
- The result is plain JSON. You can also write the objects directly:
  `{ "type": "date", "id": "due", "label": "Due" }`.

## Dialogs, panels and values

- **Dialogs** (`nail.ui.showDialog`): the `body` is a list of components;
  the dialog's `buttons` close it. The result is the button's `id` (or
  `null` when the dialog was dismissed) and the `values` of every input by
  its `id`. A primary button can only be used when every `required` input
  has a value.
- **Panels** (`ctx.panels.register`): Nail asks the plug-in to render the
  panel when it is shown; `nail.ui.updatePanel` replaces its content.
- **Attachment previews** ([attachments and links](attachments-and-links.md)):
  a preview renderer may return components (or a raster image); Nail
  shows them in the preview pane. Previews are for showing data; use
  attachment actions for things the user does with an attachment.
- **Buttons inside the content** (`<button>`) do not close anything: they
  call the dialog's `onAction` option or the panel's `onAction` with the
  current values.
- **Ids** must be unique in one dialog or panel. Every input and button
  needs one.

## Limits

A dialog or panel has at most 500 components, nested at most 8 deep; texts
up to 64 KB; at most 500 options, table rows or messages per component.
Images must be PNG, JPEG, GIF or WebP `data:` URIs of at most 512 KB, or
the path of an image file in the plug-in's package (`assets/logo.png`),
which Nail inlines; never web addresses. A tree that breaks a rule is not
shown and the plug-in's log says why. `checkTree` from `@nail/plugin-api`
finds most problems while you develop.

## Components

<!-- BEGIN GENERATED (npm run docs -w packages/plugin-api) -->

### `<text>`

A paragraph of plain text.

In JSX the `value` can be the element's text: `<text>Hello {name}</text>`.

| Member | Type | Description |
| --- | --- | --- |
| `value` | `string` | The text. |
| `style?` | `PluginTextStyle` | How it is shown. |

### `<markdown>`

CommonMark text (no raw HTML, no remote images; links open outside Nail after a confirmation).

In JSX the `value` can be the element's text: `<markdown>Hello {name}</markdown>`.

| Member | Type | Description |
| --- | --- | --- |
| `value` | `string` | The Markdown source. |

### `<input>`

A one-line text field.

Value in dialog results and UI actions: string.

| Member | Type | Description |
| --- | --- | --- |
| `kind?` | `PluginInputKind` | The kind of text. |
| `id` | `string` | Value id (unique in the tree). |
| `label` | `string` | The label. |
| `default?` | `string` | Initial value. |
| `placeholder?` | `string` | Placeholder text. |
| `required?` | `boolean` | Whether a value is needed to submit with a primary button. |
| `maxLength?` | `number` | Longest value (characters). |

### `<textarea>`

A multi-line text field.

Value in dialog results and UI actions: string.

| Member | Type | Description |
| --- | --- | --- |
| `rows?` | `number` | Visible rows. |
| `id` | `string` | Value id (unique in the tree). |
| `label` | `string` | The label. |
| `default?` | `string` | Initial value. |
| `placeholder?` | `string` | Placeholder text. |
| `required?` | `boolean` | Whether a value is needed to submit with a primary button. |
| `maxLength?` | `number` | Longest value (characters). |

### `<select>`

A choice from a list.

Value in dialog results and UI actions: string (`multiple`: array of strings).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Value id. |
| `label` | `string` | The label. |
| `options` | `Array<PluginSelectOption>` | The choices. |
| `default?` | `Array<string>` | Initially chosen value(s) (each one of the options' values). |
| `multiple?` | `boolean` | Whether several can be chosen (the value is then an array). |
| `required?` | `boolean` | Whether a choice is needed. |

### `<checkbox>`

A check box.

Value in dialog results and UI actions: boolean.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Value id. |
| `label` | `string` | The label. |
| `default?` | `boolean` | Initially checked. |

### `<date>`

A date (or date and time) field.

Value in dialog results and UI actions: `"YYYY-MM-DD"` (`time`: `"YYYY-MM-DDTHH:MM"`, local time), or `null`.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Value id. |
| `label` | `string` | The label. |
| `default?` | `string` | Initial value: `YYYY-MM-DD` (`YYYY-MM-DDTHH:MM` with `time`), `today` or `tomorrow`. |
| `min?` | `string` | Earliest date (`YYYY-MM-DD`). |
| `max?` | `string` | Latest date (`YYYY-MM-DD`). |
| `time?` | `boolean` | Whether a time is picked too. |
| `required?` | `boolean` | Whether a value is needed. |

### `<contact-picker>`

Addresses picked from the collected addresses (contacts in M3).

Value in dialog results and UI actions: array of `{name, email}`.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Value id. |
| `label` | `string` | The label. |
| `default?` | `Array<Address>` | Initial addresses. |
| `multiple?` | `boolean` | Whether several can be picked. |
| `required?` | `boolean` | Whether one is needed. |

### `<folder-picker>`

A folder picked from the folder tree.

Value in dialog results and UI actions: folder id or `null`.

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Value id. |
| `label` | `string` | The label. |
| `accountId?` | `AccountId` | Only this account's folders; `null` = every account. |
| `default?` | `FolderId` | Initial folder. |
| `required?` | `boolean` | Whether one is needed. |

### `<message-list>`

A list of messages (subject, sender, date). The plug-in names the ids; the host fills `messages` from what the plug-in may read (ids it may not read are dropped) before the tree reaches the UI.

Value in dialog results and UI actions: array of message ids (when `selectable`).

| Member | Type | Description |
| --- | --- | --- |
| `id?` | `string` | Value id (needed when `selectable`). |
| `messageIds` | `Array<MessageId>` | The messages, in display order. |
| `messages?` | `Array<PluginMessage>` | Filled by the host; ignored when a plug-in sends it. |
| `selectable?` | `boolean` | Whether rows can be selected (the value is the selected ids). |
| `multiple?` | `boolean` | Whether several rows can be selected. |

### `<table>`

A table of text cells.

Value in dialog results and UI actions: array of row indexes (when `selectable`).

| Member | Type | Description |
| --- | --- | --- |
| `id?` | `string` | Value id (needed when `selectable`). |
| `columns` | `Array<PluginTableColumn>` | The columns. |
| `rows` | `Array<Array<string>>` | The rows: one text per column. |
| `selectable?` | `boolean` | Whether rows can be selected (the value is the row indexes). |
| `multiple?` | `boolean` | Whether several rows can be selected. |

### `<button>`

A button inside a dialog body or panel: sends a UI action to the plug-in with the current values (dialog buttons that close the dialog are `PluginDialogButton`s instead).

| Member | Type | Description |
| --- | --- | --- |
| `id` | `string` | Action id. |
| `label` | `string` | The label. |
| `variant?` | `PluginButtonVariant` | Its emphasis. |
| `icon?` | `string` | Icon name from the theme's icon set. |
| `disabled?` | `boolean` | Whether it is disabled. |

### `<progress>`

A progress bar.

| Member | Type | Description |
| --- | --- | --- |
| `value?` | `number` | Percent done (0–100); `null` = unknown (indeterminate). |
| `label?` | `string` | A label. |

### `<image>`

An image.

| Member | Type | Description |
| --- | --- | --- |
| `src` | `string` | `data:image/png;base64,…` (PNG, JPEG, GIF or WebP). A plug-in may name a file of its package (`assets/logo.png`); the host inlines it before the tree reaches the UI. Never a remote URL. |
| `alt` | `string` | The text alternative (empty = decorative). |
| `width?` | `number` | Width in CSS pixels. |
| `height?` | `number` | Height in CSS pixels. |

### `<stack>`

Arranges its children in a row or a column.

In JSX its `children` are the nested elements; strings become `<text>` nodes.

| Member | Type | Description |
| --- | --- | --- |
| `direction?` | `PluginStackDirection` | The direction. |
| `children` | `Array<PluginUiNode>` | The children. |

<!-- END GENERATED -->
