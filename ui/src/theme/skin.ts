/**
 * The M5a look of the shared UI primitives and the mail panes (WP153, ADR
 * 0105), written on the token schema of ADR 0102 only: every colour,
 * size, radius, shadow and font is a `var(--nail-…)` token, so each of the
 * five themes, both densities and the user's text-size / radius / accent
 * overrides restyle it. Values without a token in today's schema carry a
 * fallback that derives from the schema (`accent-soft` = accent mixed into
 * the background, the avatar / account palette).
 *
 * The compiler emits this right after the structural base CSS and before the
 * theme's part rules, so any theme (`theme.kdl`) still overrides any rule
 * here — this is the default, not a lock. Primitives:
 *
 * - buttons: `form.button` with `data-variant` = (none) default, `primary`,
 *   `danger`, `ghost`, `icon`; `toolbar.button` (ghost); `form.button.icon`
 * - inputs and selects: `form.input`, `form.select`, `form.textarea`,
 *   `search-bar.field`; they fill their field's column or row (ADR 0160)
 *   unless `data-width` sizes them ({@link CONTROL_WIDTHS})
 * - checks and switches: `form.check`, `switch` (+ `.thumb`)
 * - chips (`chip`, `message-list.row.chip`), tags (`tag`, `tag.dot`,
 *   `tag.icon`), badges (`badge`)
 * - menus: `context-menu` with `.item`, `.item.icon`, `.item.shortcut`,
 *   `.separator`, `.header`
 * - tabs (`tabs.tab-list`, `tabs.tab`) and segmented controls
 *   (`segmented`, `segmented.option`)
 * - cards (`card`, `card.row`, `form.section`), windows (`window`)
 *
 * Mail: list rows with avatar, unread dot and tag line (`message-list.row.*`,
 * the tag line hidden in Compact), the reader column (`message-view.column`)
 * and attachment cards (`attachment-card.*`).
 *
 * M5b (WP162, ADR 0111): toolbars that never wrap (`data-overflow=
 * "more"`) with icon-only / label-only buttons, the rank-1 button as the
 * primary one and the "More" button (`toolbar.more`); the grouped folder
 * pane (`folder-tree.compose`, `.section*`, `.favorite`, `.smart-folder`,
 * account dots); the list header (`list-header.*`, filter chips); the focus
 * ring on the active list row.
 *
 * P1 (WP190), P3 (WP-card-height, ADR 0164): the HTML body's frame is as
 * tall as its mail (a geometry rule from the measured height; until then a
 * placeholder height) and the reader scrolls as a whole; designed mail (`message-view.body[data-state~=
 * "designed"]`) widens the column to a newsletter's usual 600–700 px. The
 * reader's stack is a size container (`nail-reader`): below 640 px the column
 * and the banners above it use the pane's padding instead of the wide margins,
 * and header values wrap in their own column next to the labels.
 *
 * Everything sits on the 4 px grid (`space.*`); panes share `space.pane`.
 */
import { HC, p, ps, pss } from "./selectors";
import {
  AVATAR_FG,
  CAPS_TIGHT,
  HC_RING,
  initialsAvatar,
  INITIALS_TRACKING,
  INLINE_ROW,
  QUIET_HOVER,
  READER_MEASURE,
  READER_NARROW,
  SECTION_LABEL,
  TINY_TEXT,
  VISUALLY_HIDDEN,
} from "./declarations";
import { ACCOUNT_COLOR_SLOTS } from "../api/generated";
import { FILE_KINDS } from "../widgets/fileKinds";
import { T } from "./tokens";

/**
 * File-type badge colours of attachment cards (`data-kind`; `other` is the
 * badge's own colour): the theme's `file-<kind>` colours (`defaults/theme.kdl`).
 */
const fileColors = FILE_KINDS.filter((kind) => kind !== "other")
  .map(
    (kind) =>
      `${p("attachment-card.type")}[data-kind="${kind}"] { background-color: var(--nail-color-file-${kind}); }`,
  )
  .join("\n");

/** Stable small hash of a string (FNV-1a, 32 bit). */
export function stableHash(text: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/** The palette slot (0-based) of a sender address or account id. */
export function colorSlot(key: string): number {
  return stableHash(key.trim().toLowerCase()) % ACCOUNT_COLOR_SLOTS;
}

/**
 * The avatar palette's `background-color` rules for `parts`: slot `i`
 * (`data-color`) is the theme's `avatar-<i + 1>` colour (`defaults/theme.kdl`;
 * dark enough for white initials, checked by nail-config's contrast test).
 */
export function avatarPaletteRules(parts: readonly string[]): string {
  return Array.from(
    { length: ACCOUNT_COLOR_SLOTS },
    (_, i) =>
      `${parts.map((part) => `[data-color="${String(i)}"]${p(part)}`).join(", ")} { background-color: var(--nail-color-avatar-${String(i + 1)}); }`,
  ).join("\n");
}

/**
 * Parts drawn in the avatar palette (`data-color`): P2 (ADR 0148) adds the
 * reader's conversation avatars (the strip's, and the card feed's).
 */
const PALETTE_PARTS = [
  "message-list.row.avatar",
  "tag.dot",
  "avatar",
  "folder-tree.section.dot",
  "thread-view.item.avatar",
  "thread-view.card.avatar",
] as const;

const avatarColors = avatarPaletteRules(PALETTE_PARTS);

/** A folder-pane row (folders, favourites, smart folders): icon, name, count. */
const FOLDER_ROW = `display: grid; grid-template-columns: ${T.icon} minmax(0, 1fr) auto; align-items: center; column-gap: ${T.md};
  min-height: ${T.control}; padding: 0 ${T.sm}; border-radius: ${T.rSm}; font-size: ${T.body}; color: ${T.fg};`;

/** Menus: as wide as their items, never wider than 30em or the window. */
const MENU_MAX_WIDTH = `min(30em, calc(100vw - 2 * ${T.sm}))`;

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: 2px;`;

const BUTTONS = `${p("form.button")} {
  display: inline-flex; align-items: center; justify-content: center; gap: ${T.sm};
  min-height: ${T.control}; padding: 0 ${T.md};
  border: 1px solid ${T.border}; border-radius: ${T.rMd};
  font-family: ${T.fontUi}; font-size: ${T.body}; font-weight: ${T.medium}; line-height: ${T.tight};
  color: ${T.fg}; background-color: ${T.bg};
  white-space: nowrap; cursor: default;
  transition: background-color ${T.fast} ${T.ease}, border-color ${T.fast} ${T.ease};
}
${p("form.button")}:hover { background-color: ${T.subtle}; }
${p("form.button")}:active { background-color: ${T.raised}; }
${p("form.button")}:focus-visible { ${focusRing} }
${p("form.button")}:disabled, ${p("form.button")}[aria-disabled="true"] { opacity: ${T.disabled}; }
${p("form.button")}[data-variant~="primary"] {
  border-color: transparent; color: ${T.accentFg}; background-color: ${T.accent}; font-weight: ${T.semibold};
}
${p("form.button")}[data-variant~="primary"]:hover { background-color: color-mix(in srgb, ${T.accent} 88%, ${T.fg}); }
${p("form.button")}[data-variant~="danger"] { color: ${T.danger}; border-color: color-mix(in srgb, ${T.danger} 45%, ${T.border}); }
${p("form.button")}[data-variant~="danger"]:hover { background-color: color-mix(in srgb, ${T.danger} 10%, ${T.bg}); }
${p("form.button")}[data-variant~="ghost"] { border-color: transparent; background-color: transparent; }
${p("form.button")}[data-variant~="ghost"]:hover { background-color: ${T.raised}; }
${p("form.button")}[data-variant~="icon"] { width: ${T.control}; padding: 0; border-color: transparent; background-color: transparent; color: ${T.muted}; }
${p("form.button")}[data-variant~="icon"]:hover { ${QUIET_HOVER} }
${p("form.button")}[data-state~="selected"], ${p("form.button")}[aria-pressed="true"] {
  color: ${T.accent}; background-color: ${T.accentSoft}; border-color: transparent; font-weight: ${T.semibold};
}
${p("form.button.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("toolbar")} { display: flex; align-items: center; flex-wrap: wrap; gap: ${T.xs}; min-width: 0; min-height: calc(${T.control} + 2 * ${T.sm}); padding: ${T.sm} ${T.pane}; border-bottom: 1px solid ${T.border}; background-color: ${T.bg}; }
${p("toolbar")}[data-overflow="more"] { flex-wrap: nowrap; overflow: hidden; }
${ps("toolbar.button", "toolbar.more")} {
  display: inline-flex; align-items: center; justify-content: center; flex: 0 0 auto; gap: ${T.sm};
  min-height: ${T.control}; padding: 0 ${T.md}; border: none; border-radius: ${T.rSm};
  font-size: ${T.body}; color: ${T.fg}; background-color: transparent; white-space: nowrap;
}
${pss(":hover", "toolbar.button", "toolbar.more")} { background-color: ${T.raised}; }
${pss(":focus-visible", "toolbar.button", "toolbar.more")} { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${pss(":disabled", "toolbar.button", "toolbar.more")} { opacity: ${T.disabled}; }
${p("toolbar.button")}[data-display="icon"] { width: ${T.control}; padding: 0; }
${p("toolbar.button")}[data-rank="1"] { color: ${T.accent}; background-color: ${T.accentSoft}; font-weight: ${T.semibold}; }
${p("toolbar.button")}[data-rank="1"]:hover { background-color: ${T.accentSoft}; box-shadow: inset 0 0 0 1px ${T.accent}; }
${p("toolbar.button")}[data-rank="1"] ${p("toolbar.button.icon")} { color: ${T.accent}; }
${p("toolbar.more")}[data-state~="expanded"] { background-color: ${T.raised}; }
${ps("toolbar.button.icon", "toolbar.more.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; color: ${T.muted}; }
${p("toolbar.separator")} { align-self: center; flex: 0 0 auto; width: 1px; height: calc(${T.control} - 2 * ${T.xs}); margin: 0 ${T.xs}; background-color: ${T.border}; }
${p("toolbar.separator")}[data-wrap-edge="start"] { display: none; }
${p("toolbar.separator")}[data-wrap-edge="end"] { visibility: hidden; }
${p("window.close")} { display: inline-flex; align-items: center; justify-content: center; width: ${T.control}; height: ${T.control}; padding: 0; border: none; border-radius: ${T.rSm}; color: ${T.muted}; background-color: transparent; }
${p("window.close")}:hover { ${QUIET_HOVER} }
${p("window.close")}:focus-visible { ${focusRing} }
${p("window.close.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("composer.format.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("composer.format.icon")} + ${p("composer.format.label")} { ${VISUALLY_HIDDEN} }`;

/**
 * ADR 0160: the widths of controls that are small on purpose. A control (or
 * its picker's root) says so with `data-width`; every other text-like
 * control (`form.input`, `form.select`, `form.textarea`) fills the column its
 * field, card or block gives it (`width: 100%`). In a flex row its basis
 * stays its content (`flex-basis: content`, so a wrapping row does not put
 * each control on a line of its own, and a column flex box does not grow it
 * taller): a row of controls that should share the row is listed in
 * `FIELD_ROWS` (`./skinResponsive.ts`).
 *
 * - `short`: a number, a port, minutes, a time of day (about ten characters);
 * - `medium`: a filter or chooser in a header or toolbar (the settings
 *   cards' control column);
 * - `content`: as wide as its content (a view switcher, an option among
 *   toggles).
 */
export const CONTROL_WIDTHS = {
  short: `calc(7 * ${T.lg})`,
  medium: `calc(16 * ${T.lg})`,
  content: "auto",
} as const;

/** The custom property a `data-width` control sets for its width (`width: var(…, 100%)`). */
export const CONTROL_WIDTH_VAR = "--nail-skin-control-width";

const WIDTHS = Object.entries(CONTROL_WIDTHS)
  .map(
    ([name, width]) =>
      `[data-part][data-width="${name}"] { ${CONTROL_WIDTH_VAR}: ${width}; width: var(${CONTROL_WIDTH_VAR}); max-width: 100%; flex: 0 ${name === "medium" ? "1" : "0"} auto; }`,
  )
  .join("\n");

const INPUTS = `${ps("form.input", "form.select", "form.textarea")} {
  width: var(${CONTROL_WIDTH_VAR}, 100%); flex-basis: content; min-width: 0; max-width: 100%;
  min-height: ${T.control}; padding: 0 ${T.sm};
  border: ${T.controlBorder}; border-radius: ${T.rSm};
  font-family: ${T.fontUi}; font-size: ${T.body}; color: ${T.fg}; background-color: ${T.subtle};
  transition: border-color ${T.fast} ${T.ease};
}
${p("form.textarea")} { min-height: calc(3 * ${T.control}); padding: ${T.sm}; line-height: ${T.normal}; }
${pss(":hover", "form.input", "form.select", "form.textarea")} { border-color: color-mix(in srgb, ${T.fg} 30%, ${T.border}); }
${pss(":focus-visible", "form.input", "form.select", "form.textarea")} { outline: 2px solid ${T.focus}; outline-offset: 0; border-color: ${T.accent}; }
${pss(":disabled", "form.input", "form.select", "form.textarea")} { opacity: ${T.disabled}; }
${p("form.field")} { display: grid; gap: ${T.xs}; }
${p("form.label")} { font-size: ${T.small}; font-weight: ${T.medium}; color: ${T.fg}; }
${p("form.hint")} { font-size: ${T.small}; color: ${T.muted}; }
${p("form.check")} { display: inline-flex; align-items: center; gap: ${T.sm}; min-height: ${T.control}; font-size: ${T.body}; }
${p("form.checkbox")} { width: ${T.icon}; height: ${T.icon}; margin: 0; accent-color: ${T.accent}; }
${WIDTHS}
${p("search-bar.field")} { display: flex; align-items: center; gap: ${T.sm}; min-height: calc(${T.control} + ${T.xs}); padding: 0 ${T.md}; border: ${T.controlBorder}; border-radius: ${T.rMd}; color: ${T.muted}; background-color: ${T.subtle}; }
${p("search-bar.field")}:focus-within { outline: 2px solid ${T.focus}; outline-offset: 0; border-color: ${T.accent}; }
${p("search-bar.input")} { flex: 1 1 auto; min-width: 0; border: none; outline: none; font-size: ${T.body}; color: ${T.fg}; background: transparent; }
${p("search-bar.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("search-bar.shortcut")} {
  flex: 0 0 auto; padding: 0 ${T.xs}; border: 1px solid ${T.border}; border-radius: ${T.rSm};
  font-family: ${T.fontMono}; font-size: ${T.label}; line-height: calc(${T.label} + ${T.xs}); color: ${T.muted}; background-color: ${T.bg};
}`;

const SWITCH = `${p("switch")} {
  position: relative; display: inline-flex; align-items: center; flex: 0 0 auto;
  width: calc(2 * ${T.icon} + ${T.xs}); height: calc(${T.icon} + ${T.xs});
  padding: 0; border: ${T.controlBorder}; border-radius: ${T.pill};
  background-color: ${T.bg}; transition: background-color ${T.fast} ${T.ease};
}
${p("switch")}[aria-checked="true"] { border-color: transparent; background-color: ${T.accent}; }
${p("switch")}:focus-visible { ${focusRing} }
${p("switch")}:disabled { opacity: ${T.disabled}; }
${p("switch.thumb")} {
  position: absolute; inset-block-start: 1px; inset-inline-start: 1px;
  width: calc(${T.icon} + ${T.xxs} - 2px); height: calc(${T.icon} + ${T.xxs} - 2px);
  border-radius: 50%; background-color: ${T.muted}; box-shadow: ${T.raisedShadow};
  transition: inset-inline-start ${T.fast} ${T.ease};
}
${p("switch")}[aria-checked="true"] ${p("switch.thumb")} { inset-inline-start: calc(100% - ${T.icon} - ${T.xxs} + 1px); background-color: ${T.accentFg}; }
${p("switch-field")} { display: flex; align-items: center; justify-content: space-between; gap: ${T.lg}; min-height: ${T.control}; }
${p("switch-field.text")} { display: grid; gap: ${T.xxs}; }
${p("switch-field.label")} { font-size: ${T.body}; font-weight: ${T.medium}; }
${p("switch-field.hint")} { font-size: ${T.small}; color: ${T.muted}; }`;

const CHIPS_TAGS_BADGES = `${ps("chip", "message-list.row.chip")} {
  display: inline-flex; align-items: center; gap: ${T.xs};
  padding: ${T.xxs} ${T.md}; border: 1px solid ${T.border}; border-radius: ${T.pill};
  font-size: ${T.small}; line-height: ${T.tight}; color: ${T.muted}; background-color: ${T.bg};
}
${p("chip")}[aria-pressed="true"], ${p("chip")}[data-state~="selected"] { border-color: transparent; color: ${T.accent}; background-color: ${T.accentSoft}; font-weight: ${T.semibold}; }
${p("chip")}:focus-visible { ${focusRing} }
${p("tag")} {
  display: inline-flex; align-items: center; gap: ${T.xs}; min-width: 0;
  padding: 0 ${T.sm}; border-radius: ${T.rSm};
  font-size: ${T.label}; line-height: calc(${T.label} + ${T.xs} + ${T.xxs}); color: ${T.muted}; background-color: ${T.raised};
  white-space: nowrap;
}
${p("tag")}[data-tone="warning"] { color: ${T.warning}; background-color: color-mix(in srgb, ${T.warning} 14%, ${T.bg}); }
${p("tag")}[data-tone="danger"] { color: ${T.danger}; background-color: color-mix(in srgb, ${T.danger} 14%, ${T.bg}); }
${p("tag")}[data-tone="accent"] { color: ${T.accent}; background-color: ${T.accentSoft}; }
${p("tag.dot")} { flex: 0 0 auto; width: calc(${T.xs} + ${T.xxs}); height: calc(${T.xs} + ${T.xxs}); border-radius: 50%; }
${p("tag.icon")} { flex: 0 0 auto; width: calc(${T.icon} - ${T.xs}); height: calc(${T.icon} - ${T.xs}); }
${p("tag.text")} { overflow: hidden; text-overflow: ellipsis; }
${p("badge")} {
  display: inline-flex; align-items: center; justify-content: center;
  min-width: ${T.icon}; height: ${T.icon}; padding: 0 ${T.xs}; border-radius: ${T.pill};
  font-size: ${T.label}; font-weight: ${T.semibold}; font-variant-numeric: tabular-nums;
  color: ${T.accentFg}; background-color: ${T.accent};
}
${p("badge")}[data-tone="danger"] { color: ${T.bg}; background-color: ${T.danger}; }`;

const MENUS = `${p("context-menu")} {
  width: max-content; min-width: calc(16 * ${T.lg}); max-width: ${MENU_MAX_WIDTH};
}
${p("context-menu.list")} {
  display: grid; padding: ${T.xs}; border: 1px solid ${T.border}; border-radius: ${T.rMd};
  color: ${T.fg}; background-color: ${T.bg}; box-shadow: ${T.overlay};
}
${p("context-menu.item")} {
  display: grid; grid-template-columns: ${T.icon} minmax(0, 1fr) auto auto; align-items: center; column-gap: ${T.md};
  min-height: ${T.control}; padding: 0 ${T.md} 0 ${T.sm}; border: none; border-radius: ${T.rSm};
  font-size: ${T.body}; text-align: start; color: ${T.fg}; background-color: transparent;
}
${p("context-menu.item")}:hover, ${p("context-menu.item")}:focus { outline: none; background-color: ${T.raised}; }
${p("context-menu.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("context-menu.item")}[data-state~="disabled"] { opacity: ${T.disabled}; }
${p("context-menu.item.icon")} { grid-column: 1; width: ${T.icon}; height: ${T.icon}; color: ${T.muted}; }
${p("context-menu.item.label")} {
  grid-column: 2; padding-block: ${T.xxs}; overflow: hidden; overflow-wrap: anywhere;
  display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2; line-clamp: 2;
}
${p("context-menu.item.shortcut")} { grid-column: 3; font-family: ${T.fontMono}; font-size: ${T.label}; color: ${T.muted}; }
${p("context-menu.item.arrow")} { grid-column: 4; width: ${T.icon}; height: ${T.icon}; color: ${T.muted}; }
${p("context-menu.separator")} { height: 1px; margin: ${T.xs} ${T.xs}; background-color: ${T.border}; }
${p("context-menu.header")} {
  padding: ${T.sm} ${T.md} ${T.xxs}; ${SECTION_LABEL}
}`;

const TABS = `${p("tabs.tab-list")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; padding: 0 ${T.pane}; border-bottom: 1px solid ${T.border}; background-color: ${T.bg}; }
${p("tabs.tab")} {
  min-height: calc(${T.control} + ${T.xs}); padding: 0 ${T.md}; margin-bottom: -1px;
  border: none; border-bottom: 2px solid transparent; border-radius: 0;
  font-size: ${T.body}; color: ${T.muted}; background-color: transparent;
}
${p("tabs.tab")}:hover { color: ${T.fg}; }
${p("tabs.tab")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("tabs.tab")}[data-state~="selected"], ${p("tabs.tab")}[aria-selected="true"] { color: ${T.fg}; border-bottom-color: ${T.accent}; font-weight: ${T.semibold}; background-color: transparent; }
${p("segmented")} { display: inline-flex; gap: ${T.xxs}; padding: ${T.xxs}; border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.subtle}; }
${p("segmented.option")} {
  min-height: calc(${T.control} - ${T.sm}); padding: 0 ${T.md}; border: none; border-radius: ${T.rSm};
  font-size: ${T.small}; color: ${T.muted}; background-color: transparent;
}
${p("segmented.option")}:hover { color: ${T.fg}; }
${p("segmented.option")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${p("segmented.option")}[aria-checked="true"], ${p("segmented.option")}[aria-pressed="true"] { color: ${T.fg}; background-color: ${T.bg}; font-weight: ${T.semibold}; box-shadow: ${T.raisedShadow}; }`;

const CARDS_WINDOWS = `${ps("card", "form.section")} {
  display: grid; gap: ${T.md}; padding: ${T.lg}; margin: 0;
  border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.bg};
}
${p("card.title")}, ${p("form.section.title")} { margin: 0; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("card.row")} { display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: ${T.lg}; padding: ${T.md} 0; }
${p("card.row")} + ${p("card.row")} { border-top: 1px solid ${T.border}; }
${p("window")} { border: 1px solid ${T.border}; border-radius: ${T.rLg}; color: ${T.fg}; background-color: ${T.bg}; box-shadow: ${T.windowShadow}; }
${p("window.header")} { gap: ${T.sm}; min-height: calc(${T.control} + 3 * ${T.xs}); padding: 0 ${T.sm} 0 ${T.pane}; border-bottom: 1px solid ${T.border}; background-color: ${T.bg}; }
${p("window.title")} { font-size: ${T.heading}; font-weight: ${T.semibold}; }
${p("window.body")} { padding: ${T.pane}; }
${p("mail-confirm")} { display: grid; gap: ${T.md}; align-content: start; }
${p("mail-confirm.question")}, ${p("mail-confirm.note")} { margin: 0; overflow-wrap: anywhere; }
${p("mail-confirm.note")} { color: ${T.muted}; }
${p("mail-confirm.actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; }
${p("wizard.buttons")}, ${p("pim.editor.actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; }`;

const MESSAGE_LIST = `${p("message-list")} { background-color: ${T.bg}; }
${p("message-list.row")} {
  position: relative; display: grid; grid-template-columns: ${T.avatar} minmax(0, 1fr); column-gap: ${T.md}; align-items: start;
  padding: ${T.row} ${T.pane}; border: none; border-bottom: 1px solid ${T.border};
  color: ${T.fg}; cursor: default;
}
${p("message-list.row")}[data-view="columns"] { display: block; }
${p("message-list.row")}:hover { background-color: ${T.subtle}; }
${p("message-list.row")}[data-state~="selected"] { color: ${T.selectionFg}; background-color: ${T.selection}; }
${p("message-list.row")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("message-list")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("message-list")}:focus-visible:has(${p("message-list.row")}[aria-selected="true"]) { outline: none; }
${p("message-list")}:focus-visible ${p("message-list.row")}[aria-selected="true"] { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("message-list.row.unread-dot")} {
  position: absolute; inset-inline-start: calc((${T.pane} - ${T.xs} - ${T.xxs} - 1px) / 2); inset-block-start: calc(${T.row} + ${T.md} + 1px);
  width: calc(${T.xs} + ${T.xxs} + 1px); height: calc(${T.xs} + ${T.xxs} + 1px); border-radius: 50%; background-color: var(--nail-skin-unread-dot);
}
${p("message-list.row.avatar")} { ${initialsAvatar(T.avatar, `calc(${T.avatar} * 0.37)`)} }
${p("message-list.row.body")} { display: grid; gap: ${T.xxs}; min-width: 0; }
${p("message-list.row.line")} { display: flex; align-items: baseline; gap: ${T.sm}; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-size: ${T.body}; line-height: ${T.normal}; }
${p("message-list.row.from")}, ${p("message-list.row.subject")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
${p("message-list.row")}[data-state~="unread"] ${p("message-list.row.from")}, ${p("message-list.row")}[data-state~="unread"] ${p("message-list.row.subject")} { font-weight: ${T.semibold}; }
${p("message-list.row.date")} { flex: 0 0 auto; margin-inline-start: auto; font-size: ${T.small}; font-variant-numeric: tabular-nums; color: ${T.muted}; }
${p("message-list.row.preview")} { font-size: ${T.small}; color: ${T.muted}; }
${p("message-list.row")}[data-state~="selected"] ${p("message-list.row.date")}, ${p("message-list.row")}[data-state~="selected"] ${p("message-list.row.preview")} { color: inherit; }
${p("message-list.row.tag-line")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; margin-top: ${T.xs}; min-width: 0; }
:root[data-nail-density="compact"] ${p("message-list.row.tag-line")}, ${p("app")}[data-density="compact"] ${p("message-list.row.tag-line")} { display: none; }
${p("message-list.empty")} { padding: ${T.xl} ${T.pane}; text-align: center; color: ${T.muted}; }`;

/** The reader's banners (security, remote content, junk, body status) above the column. */
export const READER_SLOTS = [
  "security-message",
  "security-warnings",
  "remote-content",
  "junk-verdict",
  "sync-message-body",
] as const;

const READER = `${p("message-view")} { display: flex; flex-direction: column; height: 100%; min-height: 0; overflow-x: hidden; overflow-y: auto; scrollbar-gutter: stable both-edges; background-color: ${T.bg}; }
${p("stack")}:has(> ${p("message-view")}) { container: nail-reader / inline-size; }
${ps(...READER_SLOTS)} {
  box-sizing: border-box; width: min(${READER_MEASURE}, 100% - 2 * ${T.xxl}); margin-inline: auto; margin-block-start: ${T.sm};
  font-family: ${T.fontUi};
}
${p("message-view.column")} {
  display: flex; flex-direction: column; flex: 1 0 auto; gap: ${T.lg}; min-height: 0;
  width: 100%; max-width: calc(${READER_MEASURE} + 2 * ${T.xxl}); margin-inline: auto; padding: ${T.xl} ${T.xxl} 0;
  font-family: ${T.fontUi};
}
${p("message-view.header")} { display: grid; gap: ${T.xs}; padding: 0; border: none; }
${p("message-view.header.subject")} { margin: 0 0 ${T.xs}; font-family: ${T.fontDisplay}; font-size: ${T.title}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("message-view.header.field")} { display: grid; grid-template-columns: calc(4 * ${T.lg}) minmax(0, 1fr); column-gap: ${T.sm}; font-size: ${T.small}; color: ${T.muted}; }
${p("message-view.header.label")} { min-width: 0; }
${p("message-view.header.value")} { min-width: 0; overflow-wrap: anywhere; }
${p("message-view.text")} { max-width: ${READER_MEASURE}; margin: 0; padding-bottom: ${T.xl}; overflow: visible; color: ${T.fg}; }
${p("message-view.body")} { display: block; flex: 0 0 auto; box-sizing: border-box; width: 100%; height: calc(12 * ${T.lg}); border: none; background-color: ${T.bg}; }
${p("message-view.column")}:has(> ${p("message-view.body")}[data-state~="designed"]) { max-width: calc(45 * ${T.lg} + 2 * ${T.xxl}); }
@container nail-reader (max-width: ${READER_NARROW}) {
  ${p("message-view.column")} { padding: ${T.lg} ${T.pane} 0; }
  ${ps(...READER_SLOTS)} { width: calc(100% - 2 * ${T.pane}); }
}
${p("message-view.attachments")} { display: flex; flex-wrap: wrap; gap: ${T.sm}; margin: 0; padding: 0; list-style: none; }
${p("message-view.attachment-list")} { display: grid; justify-items: start; gap: ${T.sm}; min-width: 0; }
${p("message-view.attachment-list")} > ${p("message-view.attachments")} { justify-self: stretch; }
${p("message-view.attachments.toggle")} {
  display: inline-flex; align-items: center; gap: ${T.xs}; min-height: ${T.control}; padding: 0 ${T.sm};
  border: none; border-radius: ${T.rMd}; background-color: transparent; cursor: default;
  font-family: ${T.fontUi}; font-size: ${T.small}; font-weight: ${T.medium}; color: ${T.accent};
}
${p("message-view.attachments.toggle")}:hover { background-color: ${T.accentSoft}; }
${p("message-view.attachments.toggle")}:focus-visible { ${focusRing} }
${p("message-view.attachments.toggle.icon")} { flex: 0 0 auto; width: calc(${T.icon} - ${T.xs}); height: calc(${T.icon} - ${T.xs}); }
${p("attachment-card")} {
  display: grid; grid-template-columns: calc(${T.control} + ${T.xs}) minmax(0, 1fr) auto; align-items: center; column-gap: ${T.md};
  min-width: calc(14 * ${T.lg}); max-width: 100%; padding: ${T.sm} ${T.sm} ${T.sm} ${T.sm};
  border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.subtle};
}
${p("attachment-card.type")} {
  display: grid; place-items: center; width: calc(${T.control} + ${T.xs}); height: calc(${T.control} + ${T.xs}); border-radius: ${T.rSm};
  font-size: ${TINY_TEXT}; font-weight: ${T.semibold}; ${INITIALS_TRACKING} color: ${AVATAR_FG}; background-color: var(--nail-color-file-other);
}
${fileColors}
${p("attachment-card.text")} { display: grid; grid-template-columns: auto minmax(0, 1fr); column-gap: ${T.sm}; min-width: 0; }
${p("attachment-card.name")} { grid-column: 1 / -1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: ${T.small}; font-weight: ${T.semibold}; }
${p("attachment-card.size")} { font-size: ${T.small}; font-variant-numeric: tabular-nums; color: ${T.muted}; }
${p("attachment-card.status")} { grid-column: 2; display: flex; align-items: center; gap: ${T.xxs}; min-width: 0; overflow: hidden; white-space: nowrap; font-size: ${T.small}; color: ${T.muted}; }
${p("attachment-card.status")}[data-state~="empty"] { display: none; }
${p("attachment-card.status.icon")} { flex: 0 0 auto; width: calc(${T.icon} - ${T.xs}); height: calc(${T.icon} - ${T.xs}); }
${p("attachment-card.status.text")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
${p("attachment-card")}[data-state~="saved"] ${p("attachment-card.status")}, ${p("attachment-card")}[data-state~="downloaded"] ${p("attachment-card.status")} { color: ${T.success}; }
${p("attachment-card")}[data-state~="failed"] ${p("attachment-card.status")}, ${p("attachment-card")}[data-state~="unsupported"] ${p("attachment-card.status")} { color: ${T.danger}; }
${p("attachment-card.actions")} { display: flex; align-items: center; gap: ${T.xxs}; }`;

/** Folder pane, account switcher and status bar on the same grid. */
const PANES = `${p("account-switcher")} { display: grid; gap: ${T.xxs}; padding: ${T.sm}; border-bottom: 1px solid ${T.border}; background-color: ${T.subtle}; }
${p("account-switcher.item")} {
  display: flex; align-items: center; gap: ${T.sm}; min-height: ${T.control}; padding: 0 ${T.sm};
  border: none; border-radius: ${T.rSm}; font-size: ${T.body}; text-align: start; color: ${T.fg}; background-color: transparent;
}
${p("account-switcher.item")}:hover { background-color: ${T.raised}; }
${p("account-switcher.item")}[data-state~="selected"] { color: ${T.selectionFg}; background-color: ${T.selection}; font-weight: ${T.semibold}; }
${p("account-switcher.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("folder-tree")} { padding: ${T.sm}; background-color: ${T.subtle}; }
${p("folder-tree.item")} {
  ${FOLDER_ROW}
}
${p("folder-tree.item")}:hover { background-color: ${T.raised}; }
${p("folder-tree.item")}[data-state~="selected"] { color: ${T.selectionFg}; background-color: ${T.selection}; font-weight: ${T.semibold}; }
${p("folder-tree.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("folder-tree.item.icon")} { width: ${T.icon}; height: ${T.icon}; color: ${T.muted}; }
${p("folder-tree.item")}[data-state~="selected"] ${p("folder-tree.item.icon")} { color: ${T.accent}; }
${p("folder-tree.item.name")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("folder-tree.item.unread-count")} { font-size: ${T.small}; font-variant-numeric: tabular-nums; color: ${T.muted}; }
${p("folder-tree.item")}[data-state~="selected"] ${p("folder-tree.item.unread-count")} { color: ${T.accent}; font-weight: ${T.semibold}; }
${p("folder-tree")}[data-mode="sections"] { display: flex; flex-direction: column; gap: ${T.md}; padding: ${T.md} ${T.sm}; }
${p("folder-tree.compose")} {
  display: inline-flex; align-items: center; justify-content: center; gap: ${T.sm}; flex: 0 0 auto;
  min-height: calc(${T.control} + ${T.sm}); margin: 0 ${T.xs}; padding: 0 ${T.md};
  border: none; border-radius: ${T.rMd}; font-size: ${T.body}; font-weight: ${T.semibold};
  color: ${T.accentFg}; background-color: ${T.accent};
}
${p("folder-tree.compose")}:hover { background-color: color-mix(in srgb, ${T.accent} 88%, ${T.fg}); }
${p("folder-tree.compose")}:focus-visible { ${focusRing} }
${p("folder-tree.compose")}:disabled { opacity: ${T.disabled}; }
${p("folder-tree.compose.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; }
${p("folder-tree")}[data-mode="sections"] > ${p("folder-tree.list")} { display: grid; gap: ${T.lg}; align-content: start; }
${p("folder-tree.section")} { display: grid; gap: ${T.xxs}; outline: none; }
${p("folder-tree.group")} { display: grid; gap: 1px; margin: 0; padding: 0; list-style: none; }
${ps("folder-tree.section.header", "folder-tree.account")} {
  display: flex; align-items: center; gap: ${T.sm}; min-width: 0; min-height: calc(${T.control} - ${T.sm}); padding: 0 ${T.sm};
  border-radius: ${T.rSm}; font-size: ${T.label}; font-weight: ${T.semibold}; ${CAPS_TIGHT}
  color: ${T.muted}; user-select: none;
}
${p("folder-tree.section")}:focus-visible > ${p("folder-tree.section.header")}, ${p("folder-tree.section")}:focus-visible > ${p("folder-tree.account")} { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("folder-tree.section")}[aria-expanded] > ${p("folder-tree.section.header")}:hover, ${p("folder-tree.section")}[aria-expanded] > ${p("folder-tree.account")}:hover { ${QUIET_HOVER} }
${p("folder-tree.section.chevron")} { flex: 0 0 auto; width: calc(${T.icon} - ${T.xs}); height: calc(${T.icon} - ${T.xs}); }
${p("folder-tree.section.dot")} { flex: 0 0 auto; width: ${T.sm}; height: ${T.sm}; border-radius: 50%; }
${p("folder-tree.section.title")} { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
${p("folder-tree.section.unread-count")} { margin-inline-start: auto; font-size: ${T.small}; font-weight: ${T.semibold}; letter-spacing: 0; text-transform: none; font-variant-numeric: tabular-nums; color: ${T.accent}; }
${ps("folder-tree.favorite", "folder-tree.smart-folder")} {
  ${FOLDER_ROW}
}
${pss(":hover", "folder-tree.favorite", "folder-tree.smart-folder")} { background-color: ${T.raised}; }
${pss(":focus-visible", "folder-tree.favorite", "folder-tree.smart-folder")} { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("folder-tree.favorite")}[data-state~="selected"], ${p("folder-tree.smart-folder")}[data-state~="selected"] { color: ${T.selectionFg}; background-color: ${T.selection}; font-weight: ${T.semibold}; }
${p("folder-tree.favorite")}[data-state~="selected"] ${p("folder-tree.item.icon")}, ${p("folder-tree.smart-folder")}[data-state~="selected"] ${p("folder-tree.item.icon")} { color: ${T.accent}; }
${p("folder-tree.favorite")}[data-state~="selected"] ${p("folder-tree.item.unread-count")} { color: ${T.accent}; font-weight: ${T.semibold}; }
${p("status-bar")} { display: flex; align-items: center; gap: ${T.md}; min-height: calc(${T.control} - ${T.xs}); padding: 0 ${T.pane}; font-size: ${T.small}; color: ${T.muted}; border-top: 1px solid ${T.border}; background-color: ${T.subtle}; }
${p("search-bar")} { padding: ${T.md} ${T.pane} ${T.sm}; }`;

/** The message list header (M5b): search, title with counts, filter chips. */
const LIST_HEADER = `${p("list-header")} { display: grid; gap: ${T.md}; padding: ${T.md} ${T.pane} ${T.sm}; border-bottom: 1px solid ${T.border}; background-color: ${T.bg}; }
${p("list-header")} ${p("search-bar")} { padding: 0; }
${p("list-header.heading")} { display: flex; align-items: baseline; gap: ${T.sm}; min-width: 0; }
${p("list-header.title")} { margin: 0; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: ${T.fontDisplay}; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("list-header.counts")} { flex: 0 0 auto; font-size: ${T.small}; font-variant-numeric: tabular-nums; color: ${T.muted}; white-space: nowrap; }
${p("list-header.chips")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; }
${p("list-header.chips")} ${p("chip")} { min-height: calc(${T.control} - ${T.sm}); cursor: default; }
${p("list-header.chips")} ${p("chip")}:hover { color: ${T.fg}; }`;

/** Settings → Appearance. */
const APPEARANCE = `${p("appearance")} { display: grid; gap: ${T.section}; align-content: start; max-width: calc(48 * ${T.lg}); padding: ${T.pane}; overflow: auto; }
${p("appearance.pair")} { display: grid; grid-template-columns: repeat(auto-fit, minmax(calc(18 * ${T.lg}), 1fr)); gap: ${T.md}; }
${p("theme-gallery")} { display: grid; grid-template-columns: repeat(auto-fill, minmax(calc(10 * ${T.lg}), 1fr)); gap: ${T.md}; }
${p("theme-card")} {
  position: relative; display: grid; gap: ${T.xxs}; padding: 0 0 ${T.sm}; overflow: hidden; text-align: start;
  border: 2px solid ${T.border}; border-radius: ${T.rMd}; color: ${T.fg}; background-color: ${T.bg};
}
${p("theme-card")}[aria-checked="true"] { border-color: ${T.accent}; }
${p("theme-card")}:focus-visible { ${focusRing} }
${p("theme-card")}:disabled { opacity: ${T.disabled}; }
${p("theme-card.preview")} { display: grid; grid-template-columns: 1fr 2fr 1fr 1fr; height: calc(4 * ${T.lg}); margin-bottom: ${T.xs}; }
${p("theme-card.name")} { padding: 0 ${T.md}; font-size: ${T.small}; font-weight: ${T.semibold}; }
${p("theme-card.kind")}, ${p("theme-card.problem")} { padding: 0 ${T.md}; font-size: ${T.label}; color: ${T.muted}; }
${p("theme-card.problem")} { color: ${T.danger}; }
${p("theme-card.check")} { position: absolute; inset-block-end: ${T.sm}; inset-inline-end: ${T.sm}; width: ${T.icon}; height: ${T.icon}; color: ${T.accent}; }
${p("appearance.overrides")} { display: grid; gap: ${T.md}; }
${p("appearance.accent")} { display: flex; align-items: center; gap: ${T.sm}; }
${p("appearance.accent-picker")} { width: calc(${T.control} + ${T.md}); height: ${T.control}; padding: ${T.xxs}; border: 1px solid ${T.border}; border-radius: ${T.rSm}; background: none; }
${p("appearance.actions")} { display: flex; justify-content: flex-end; gap: ${T.sm}; }
${p("form.error")} { margin: 0; font-size: ${T.small}; color: ${T.danger}; }`;

/**
 * High contrast (any theme in `high-contrast` mode) for the M5b parts, like
 * the built-in theme does for the M5a ones: everything inside a selected
 * item uses `selection-fg`, hover is the soft accent, focus rings are 3px,
 * buttons get the thin (2px) border.
 */
const HIGH_CONTRAST = `${HC} ${p("folder-tree.favorite")}[data-state~="selected"] ${p("folder-tree.item.icon")}, ${HC} ${p("folder-tree.favorite")}[data-state~="selected"] ${p("folder-tree.item.unread-count")}, ${HC} ${p("folder-tree.smart-folder")}[data-state~="selected"] ${p("folder-tree.item.icon")} { color: inherit; }
${HC} ${p("folder-tree.favorite")}:hover, ${HC} ${p("folder-tree.smart-folder")}:hover, ${HC} ${p("toolbar.more")}:hover { background-color: ${T.accentSoft}; }
${HC} ${p("folder-tree.favorite")}[data-state~="selected"]:hover, ${HC} ${p("folder-tree.smart-folder")}[data-state~="selected"]:hover { background-color: ${T.selection}; }
${HC} ${p("folder-tree.favorite")}:focus-visible, ${HC} ${p("folder-tree.smart-folder")}:focus-visible, ${HC} ${p("toolbar.more")}:focus-visible, ${HC} ${p("folder-tree.compose")}:focus-visible, ${HC} ${p("list-header.chips")} ${p("chip")}:focus-visible { outline: ${HC_RING} solid ${T.focus}; }
${HC} ${p("folder-tree.section")}:focus-visible > ${p("folder-tree.section.header")}, ${HC} ${p("folder-tree.section")}:focus-visible > ${p("folder-tree.account")} { outline: ${HC_RING} solid ${T.focus}; }
${HC} ${p("toolbar.more")} { border: var(--nail-border-thin, 2px solid ${T.border}); }
${HC} ${p("message-list")}:focus-visible ${p("message-list.row")}[aria-selected="true"] { outline: ${HC_RING} solid ${T.focus}; }
${HC} ${p("segmented.option")}[aria-checked="true"], ${HC} ${p("segmented.option")}[aria-pressed="true"] {
  color: ${T.accentFg}; background-color: ${T.accent}; box-shadow: none; outline: 2px solid ${T.fg}; outline-offset: -2px;
}
@media (forced-colors: active) {
  ${p("segmented.option")}[aria-checked="true"], ${p("segmented.option")}[aria-pressed="true"] { forced-color-adjust: none; color: HighlightText; background-color: Highlight; outline: 2px solid CanvasText; outline-offset: -2px; }
}`;

/**
 * M5b integration (ADR 0114): the one-time offer of the new default layout
 * (a small card at the bottom right, never modal), and the composer's Sign /
 * Encrypt / Technology row on one line (the Technology label beside its
 * select).
 */
const M5B_INTEGRATION = `${p("layout-offer")} {
  position: fixed; inset-inline-end: ${T.xl}; inset-block-end: ${T.xl}; z-index: 30;
  display: grid; gap: ${T.sm}; max-width: calc(24 * ${T.lg}); padding: ${T.lg};
  border: 1px solid ${T.border}; border-radius: ${T.rLg}; color: ${T.fg}; background-color: ${T.bg}; box-shadow: ${T.overlay};
}
${p("layout-offer.title")} { margin: 0; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("layout-offer.text")} { margin: 0; font-size: ${T.body}; line-height: ${T.normal}; color: ${T.muted}; }
${p("layout-offer.actions")} { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: ${T.sm}; }
${p("security-compose.toggles")} > ${p("form.field")} { ${INLINE_ROW} gap: ${T.sm}; }
${p("security-compose.toggles")} > ${p("form.field")} > ${p("form.label")} {
  /* P1 verify: "Technology" reads like the "Sign" / "Encrypt" check labels beside it, not like a bold field label. */
  font-size: ${T.body}; font-weight: ${T.regular};
}
${p("folder-tree.section.status")} {
  flex: 0 0 auto; padding: 0 ${T.xs}; border: 1px solid ${T.border}; border-radius: ${T.pill};
  font-size: ${T.small}; font-weight: ${T.regular}; line-height: 1.4; text-transform: none; letter-spacing: 0;
  white-space: nowrap; color: ${T.muted};
}
${p("folder-tree.section.status")} { margin-inline-start: auto; }
${p("folder-tree.section.status")} + ${p("folder-tree.section.unread-count")} { margin-inline-start: ${T.xs}; }
${p("folder-tree.section.status")}[data-state="auth-error"] { color: ${T.danger}; }`;

/** Tokens used above that are not in the schema yet (derived defaults). */
const DERIVED = `:root { --nail-skin-unread-dot: var(--nail-color-unread, var(--nail-color-accent)); }`;

/** Avatar initials palette, account dots, file-type badges. */
const PALETTES = avatarColors;

/** The whole M5a skin. */
export const SKIN_CSS = [
  DERIVED,
  BUTTONS,
  INPUTS,
  SWITCH,
  CHIPS_TAGS_BADGES,
  MENUS,
  TABS,
  CARDS_WINDOWS,
  PANES,
  APPEARANCE,
  LIST_HEADER,
  MESSAGE_LIST,
  READER,
  PALETTES,
  M5B_INTEGRATION,
  HIGH_CONTRAST,
].join("\n");
