/**
 * Responsive rules shared by the windows and dialogs outside the mail and
 * PIM views (P1 WP191, ADR 0139): form rows that wrap with aligned labels,
 * button rows that wrap, cards and window bodies that scroll their own
 * content and grow with the window. Token schema only (`var(--nail-…)`) and
 * `[data-part]` selectors only, like `./skin.ts`.
 *
 * The compiler emits it after the shared skin and every feature skin and
 * before the theme's part rules: it may adjust the primitives of
 * `./skin.ts` (buttons, inputs, cards) for narrow windows, and any
 * `theme.kdl` still overrides it. Where a rule must win over a default
 * `theme.kdl` rule of the same part, its selector names the part's place
 * (inside a window body or a settings card), which is more specific than
 * the theme's bare part selector — a user theme can still be more specific.
 *
 * - Window bodies (`window.body`) pad their content; the dialog inside
 *   does not pad again (no doubled margins in a small window) and may
 *   shrink below its content's natural width (it wraps instead of
 *   scrolling sideways); its last button row stays at the bottom of the
 *   window while the body scrolls (`DIALOG_BUTTON_ROWS`).
 * - Form rows, fieldsets, hints and button rows wrap.
 * - Two-pane widgets (a list beside its editor: accounts, rules) stack when
 *   the card or window that holds them is narrow (`TWO_PANES_NARROW`).
 */

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;
/** Joined selectors of several parts. */
const ps = (...parts: string[]) => parts.map(p).join(", ");

/** Token shorthands. */
const T = {
  bg: "var(--nail-color-bg)",
  border: "var(--nail-color-border)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  lg: "var(--nail-space-lg)",
  pane: "var(--nail-space-pane)",
  control: "var(--nail-size-control)",
} as const;

/**
 * The container name of window bodies and settings cards: what their form
 * rows and two-pane widgets measure (`features/settings/skin.ts` names the
 * card the same).
 */
export const FIELDS_CONTAINER = "nail-fields";

/**
 * The button row that ends each dialog (the last child of its parent):
 * it sticks to the bottom of the window while the body scrolls, so
 * "Next", "Save" or "Cancel" never scroll out of reach (the P1 standard:
 * header and button row stay visible). It reaches into the scroller's
 * bottom padding (`bottom` = minus the window body's padding, which it
 * fills with its own), so nothing scrolls by below it whether the window
 * body or the dialog itself scrolls.
 */
export const DIALOG_BUTTON_ROWS = [
  "wizard.buttons",
  "account-setup.actions",
  "plugin-install.actions",
  "remote-pair.actions",
  "gmail.dialog.actions",
  "scheduling.dialog.actions",
  "merge-dialog.actions",
  "snooze.actions",
  "mail-confirm.actions",
  "confirmation.buttons",
  "composer.confirm.actions",
] as const;

/** A part inside a window's body. */
const inBody = (part: string) => `${p("window.body")} ${p(part)}`;

/**
 * Dialogs that cap their own width (a theme's `max-width`) grow with their
 * window instead (no fixed-width island in a wide window); nested minimum
 * widths give way to the window's own min-size.
 */
export const GROWING_DIALOGS = ["scheduling.dialog", "gmail.dialog"] as const;

const WINDOWS = `${p("window.body")} { container: ${FIELDS_CONTAINER} / inline-size; }
${p("window.body")} > [data-part] { min-width: 0; padding: 0; }
${GROWING_DIALOGS.map((part) => `${p("window.body")} > ${p(part)}`).join(", ")} { max-width: none; }
${p("window.body")} ${p("scheduling.find-time")} { min-width: 0; padding: 0; }
${DIALOG_BUTTON_ROWS.map((part) => `${inBody(part)}:last-child`).join(",\n")} {
  position: sticky; bottom: calc(-1 * ${T.pane}); z-index: 1;
  display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm};
  margin-block-start: auto; padding-block: ${T.sm} ${T.pane};
  border-top: 1px solid ${T.border}; background-color: ${T.bg};
}`;

/**
 * Form rows, fieldsets and button rows that wrap instead of overflowing:
 * every `….actions` / `….buttons` row of buttons is a wrapping flex row
 * with a gap (a theme's own rule for the row still wins).
 */
const FORMS = `${p("form.field")} { min-width: 0; }
${p("form.field")} > :is(${ps("form.input", "form.select", "form.textarea")}) { min-width: 0; max-width: 100%; }
${ps("form.section", "form.check", "form.check.label")} { min-width: 0; }
${ps("form.check.label", "form.hint")} { overflow-wrap: anywhere; }
:is([data-part$=".actions"], [data-part$=".buttons"], [data-part$="-actions"]):has(> ${p("form.button")}) {
  display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm};
}
${p("account-setup.steps")} {
  flex-wrap: wrap; column-gap: ${T.lg}; row-gap: ${T.xs}; list-style-position: inside;
}`;

/**
 * Paragraphs inside windows and settings cards: their containers space them
 * (flex / grid gaps), so the browser's paragraph margins would double the
 * space (a hint drifting away from its field).
 */
const TEXT = `:is(${ps("window.body", "settings-page.card")}) p[data-part] { margin-block: 0; }`;

/**
 * Rows of inline fields (a search bar, "Client name [  ] Create"): each
 * field is label + control on one line, the row wraps; inside a settings
 * card they are not the card's two-column rows.
 */
export const INLINE_FIELD_ROWS = ["security-keys.filters", "mcp-clients.create"] as const;

const inlineRows = INLINE_FIELD_ROWS.map(p).join(", ");
const INLINE = `:is(${inlineRows}) { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm} ${T.lg}; }
:is(${inlineRows}) > ${p("form.field")}, ${p("settings-page.card")} :is(${inlineRows}) > ${p("form.field")} {
  display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; flex: 1 1 14rem; padding: 0; border: none;
}
${p("settings-page.card")} :is(${inlineRows}) > ${p("form.field")} > :is(${ps("form.input", "form.select")}) {
  flex: 1 1 8rem; width: auto; min-width: 8rem;
}
${p("settings-page.card")} :is(${inlineRows}) > ${p("form.check")} { padding: 0; border: none; }`;

/**
 * View switchers made of pressed buttons (the confirmation center's
 * Waiting / History, the rule editor's Editor / Text) look like the shared
 * segmented control.
 */
const SWITCHERS = `:is(${ps("confirmation-center.views", "rules.views")}) {
  display: inline-flex; align-self: flex-start; gap: ${T.xxs}; padding: ${T.xxs};
  border: 1px solid ${T.border}; border-radius: var(--nail-radius-md); background-color: var(--nail-color-bg-subtle);
}
:is(${ps("confirmation-center.view", "rules.view")}) {
  display: inline-flex; align-items: center; min-height: calc(${T.control} - ${T.sm}); padding: 0 ${T.md};
  border: none; border-radius: var(--nail-radius-sm); font-size: var(--nail-font-size-small);
  color: var(--nail-color-fg-muted); background-color: transparent;
}
:is(${ps("confirmation-center.view", "rules.view")}):hover { color: var(--nail-color-fg); }
:is(${ps("confirmation-center.view", "rules.view")}):focus-visible { outline: 2px solid var(--nail-color-focus-ring); outline-offset: 1px; }
:is(${ps("confirmation-center.view", "rules.view")})[aria-pressed="true"] {
  opacity: 1; color: var(--nail-color-fg); background-color: ${T.bg};
  font-weight: var(--nail-font-weight-semibold); box-shadow: var(--nail-shadow-raised);
}`;

/**
 * "Apply a theme or layout" (customization): the kinds of file as aligned
 * radio rows, and "Load a file" as a button (its file input is visually
 * hidden, so the keyboard still reaches it, instead of `display: none`).
 */
const MERGE = `${p("merge-dialog.files")} { flex-wrap: wrap; row-gap: ${T.xs}; }
${p("merge-dialog.file")} { display: inline-flex; align-items: center; gap: ${T.xs}; min-height: ${T.control}; }
${p("merge-dialog.upload")} {
  position: relative; display: inline-flex; align-items: center; align-self: flex-start; gap: ${T.sm};
  min-height: ${T.control}; padding: 0 var(--nail-space-md);
  border: 1px solid ${T.border}; border-radius: var(--nail-radius-md);
  font-weight: var(--nail-font-weight-medium); background-color: ${T.bg}; cursor: default;
}
${p("merge-dialog.upload")}:hover { background-color: var(--nail-color-bg-subtle); }
${p("merge-dialog.upload")}:focus-within { outline: 2px solid var(--nail-color-focus-ring); outline-offset: 2px; }
${p("merge-dialog.upload")} > ${p("merge-dialog.upload.input")} {
  display: block; position: absolute; width: 1px; height: 1px; margin: -1px; padding: 0;
  overflow: hidden; clip-path: inset(50%); white-space: nowrap; border: 0;
}`;

/**
 * The wizards' choice rows (portability): the label and its description on
 * one centred line while they fit, the description below in a narrow window.
 */
const CHOICES = `${p("portability.choices")} > ${p("portability.choice")} { flex-wrap: wrap; align-items: center; column-gap: var(--nail-space-md); row-gap: 0; }
${p("portability.choice")} > ${p("form.check")} { flex: 0 1 auto; }
${p("portability.choice")} > ${p("form.hint")} { flex: 1 1 12rem; min-width: 0; }`;

/**
 * The free/busy timeline (scheduling): the hour scale starts where the
 * tracks start (the theme's 160 px name column plus the row's gap).
 */
const FREEBUSY = `${p("freebusy")} > ${p("freebusy.scale")} { margin-inline-start: calc(160px + ${T.xs}); }`;

/** The configuration status says "all valid" in the success colour, not the error one. */
const CONFIG_STATUS = `${p("config-errors")}[data-state="valid"] > ${p("config-errors.summary")} {
  color: var(--nail-color-success); font-weight: var(--nail-font-weight-medium);
}`;

/** Buttons alone in a settings card's column keep their own width. */
const CARD_BUTTONS = `${p("settings-page.card")} [data-part] > ${p("form.button")} { justify-self: start; }`;

/**
 * Keys & encryption (security): key rows in aligned columns (name, kind,
 * own key, trust or state, fingerprint) that wrap in a narrow card; the
 * filter bar as one row of inline fields; the key's facts as a two-column
 * list.
 */
const SECURITY = `${p("security-keys.list")} ${p("security-keys.key")} {
  display: grid; grid-template-columns: minmax(0, 1fr) 4.5rem 7.5rem 7.5rem 11.5rem;
  align-items: center; column-gap: ${T.sm};
}
${p("security-keys.key.name")} { grid-column: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("security-keys.key.technology")} { grid-column: 2; }
${p("security-keys.key.own")} { grid-column: 3; }
${p("security-keys.key")} > ${p("security-trust")}, ${p("security-keys.key.state")} { grid-column: 4; }
${p("security-keys.key.fingerprint")} { grid-column: 5; white-space: nowrap; }
${p("security-keys.key")} > * { grid-row: 1; }
${p("security-keys.key")} > ${p("security-keys.key.state")} ~ ${p("security-keys.key.fingerprint")} { grid-row: 1; }
@container ${FIELDS_CONTAINER} (max-width: 40rem) {
  ${p("security-keys.list")} ${p("security-keys.key")} { display: flex; flex-wrap: wrap; row-gap: ${T.xs}; }
  ${p("security-keys.key.name")} { flex: 1 0 100%; }
}
${p("security-identity")} { display: grid; gap: ${T.xs}; min-width: 0; }
${p("security-identity")} > ${p("form.check")} { justify-self: start; }
${p("security-key")} { display: flex; flex-direction: column; align-items: flex-start; gap: ${T.md}; min-width: 0; }
${p("security-key")} > * { max-width: 100%; }
${p("security-key.title")} { margin: 0; overflow-wrap: anywhere; }
${p("security-key.list")} {
  display: grid; grid-template-columns: max-content minmax(0, 1fr); column-gap: ${T.lg}; row-gap: ${T.xs}; margin: 0;
}
${p("security-key.row")} { display: contents; }
${p("security-key.label")} { color: var(--nail-color-fg-muted); }
${p("security-key.value")} { margin: 0; min-width: 0; overflow-wrap: anywhere; }
@container ${FIELDS_CONTAINER} (max-width: 26rem) {
  ${p("security-key.list")} { grid-template-columns: minmax(0, 1fr); row-gap: 0; }
  ${p("security-key.value")} { margin-block-end: ${T.xs}; }
}`;

/** Below this width of their card or window, a list and its editor stack. */
export const TWO_PANES_NARROW = "36rem";

const TWO_PANES = `${p("window.body")} ${p("rules")}, ${p("settings-page.card")} ${p("rules")} {
  grid-template-columns: minmax(10rem, 17.5rem) minmax(0, 1fr);
}
@container ${FIELDS_CONTAINER} (max-width: ${TWO_PANES_NARROW}) {
  ${p("window.body")} :is(${ps("account-settings", "rules")}), ${p("settings-page.card")} :is(${ps("account-settings", "rules")}) {
    grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr);
  }
  ${p("settings-page.card")} ${p("account-settings.sidebar")} { border-right: none; border-bottom: 1px solid ${T.border}; overflow: visible; }
  ${p("settings-page.card")} ${p("account-settings.list")} { flex-direction: row; flex-wrap: wrap; list-style: none; }
  ${p("settings-page.card")} ${p("account-settings.account")} { width: auto; }
  ${p("window.body")} ${p("rules.list")} { overflow: visible; }
}`;

export const RESPONSIVE_SKIN_CSS = [
  WINDOWS,
  FORMS,
  TEXT,
  INLINE,
  SWITCHERS,
  MERGE,
  CHOICES,
  FREEBUSY,
  CONFIG_STATUS,
  CARD_BUTTONS,
  SECURITY,
  TWO_PANES,
].join("\n");
