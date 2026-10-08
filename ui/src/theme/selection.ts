/**
 * Text selection (ADR 0182): Nail's own chrome — labels, titles, buttons,
 * list rows, menus — cannot be selected; the user's data can: mail text,
 * header values (addresses, subject, date), contact details, keys and
 * fingerprints, error details, logs and diagnostics, plug-in content, and
 * every text field and editor. Mail documents in their frames are
 * documents of their own and stay selectable.
 *
 * One list ({@link SELECTABLE_PARTS}) decides both the stylesheet
 * ({@link SELECTION_CSS}, emitted right after the structural base CSS, so
 * any skin or theme rule still overrides it) and the text menu
 * ({@link selectableRoot}: Copy / Select all on selectable text; ADR 0184).
 */

/**
 * Parts whose text is the user's data. Their descendants follow them
 * (`user-select: auto` takes the parent's value).
 */
export const SELECTABLE_PARTS: readonly string[] = [
  // The reader: subject, header values, plain-text bodies and quotes.
  "message-view.header.subject",
  "message-view.header.value",
  "message-view.text",
  "message-view.quote",
  "thread-view.subject",
  "thread-view.participants",
  "thread-view.body",
  "thread-view.quote",
  "thread-view.text",
  // Invitations and sender checks.
  "invitation.detail.value",
  "invitation.comment.text",
  "invitation.attendee.name",
  "auth-details.server",
  "auth-details.method.domain",
  // Keys, certificates, codes and addresses to copy.
  "security-details.value",
  "security-chain.subject",
  "security-chain.issuer",
  "security-fingerprint",
  "security-keys.key.fingerprint",
  "security-key.value",
  "security-key.exported.text",
  "security-autocrypt.code.value",
  "smartcards.object.fingerprint",
  "recovery-key.value",
  "remote-fingerprint",
  "remote-login.fingerprint",
  "remote-pair.code",
  "remote-pair.fingerprint",
  "remote-settings.url.text",
  "mcp-token.value",
  // Contacts.
  "contact-card.name",
  "contact-card.organization",
  "contact-card.value",
  "contact-card.address.line",
  "contact-card.notes",
  "contact-card.text",
  // Error details, logs, diagnostics, scripts and differences.
  "config-errors.item.message",
  "config-errors.item.source",
  "diagnostics.preview",
  "diagnostics.entry.details",
  "portability.issue.details",
  "portability.issue.path",
  "plugin-logs.message",
  "plugin-activity.last-error",
  "mcp-audit.detail",
  "merge-dialog.diff",
  "merge-dialog.preview",
  "sieve-status.script",
  "sieve-status.details",
  "search-bar.error.query",
  "outbox.item.error",
  // Plug-in content (its text is the plug-in's, not Nail's chrome).
  "plugin.text",
  "plugin.markdown",
  "plugin-preview.body",
];

/** Text fields and editors (always selectable). */
const EDITABLE = ["input", "textarea", '[contenteditable]:not([contenteditable="false"])'];

/** Everything selectable: the parts and every text field or editor. */
export const SELECTABLE_SELECTOR = [
  ...SELECTABLE_PARTS.map((part) => `[data-part="${part}"]`),
  ...EDITABLE,
].join(", ");

const PARTS = new Set(SELECTABLE_PARTS);
const EDITABLE_SELECTOR = EDITABLE.join(", ");

/**
 * The selectable element around `element`: the field or editor it is in,
 * else the outermost selectable part around it; `null` in the chrome.
 */
export function selectableRoot(element: Element | null): Element | null {
  if (element === null) return null;
  const editable = element.closest(EDITABLE_SELECTOR);
  if (editable !== null) return editable;
  let root: Element | null = null;
  for (let at: Element | null = element; at !== null; at = at.parentElement) {
    const part = at.getAttribute("data-part");
    if (part !== null && PARTS.has(part)) root = at;
  }
  return root;
}

/** Whether `element` lies in selectable text (a part above, a field, an editor). */
export function isSelectable(element: Element | null): boolean {
  return selectableRoot(element) !== null;
}

/**
 * The selection rules: nothing in the page is selectable unless it is in
 * {@link SELECTABLE_SELECTOR}.
 */
export const SELECTION_CSS = `body { user-select: none; }
${SELECTABLE_SELECTOR} { user-select: text; }`;
