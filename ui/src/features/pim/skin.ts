/**
 * The M5b look of the calendar, contact and task views (WP164, ADR 0109),
 * on the token schema only (`var(--nail-…)`), like `../../theme/skin.ts`.
 * The compiler emits it right after the shared skin and before the theme's
 * part rules, so any `theme.kdl` still overrides it.
 *
 * - Calendar toolbar (`calendar.toolbar`): Today, previous / next and the
 *   range on one line that never wraps (the range is cut with an
 *   ellipsis), the segmented view switch (`segmented[field=calendar-view]`)
 *   and "New event".
 * - Event cards (`calendar.event`, `calendar.chip`, `calendar.agenda.event`,
 *   `agenda.list.event` with `event-card.title` / `.time` / `.location`):
 *   title first, the text always `fg`; the calendar colour arrives as a
 *   geometry rule (bar + light tint, `colors.ts`); High contrast drops the
 *   tint and outlines the card.
 * - Calendar list: group headers with a "+" menu button, rows with a colour
 *   square (`calendar-list.toggle`, filled = shown) and a "…" menu button
 *   that appears on hover / focus; the colour choice of the dialogs.
 * - Mini-month, contacts (avatars in the shared avatar palette) and tasks.
 *
 * Structure the views need in every theme stays in `parts.ts`
 * (`PIM_BASE_CSS`).
 */
import { AVATAR_COLORS } from "../../theme/skin";

/** `[data-part="…"]` for one part. */
const p = (part: string) => `[data-part="${part}"]`;
/** Joined selectors of several parts. */
const ps = (...parts: string[]) => parts.map(p).join(", ");
/** Joined selectors of several parts, each followed by the same suffix. */
const pss = (suffix: string, ...parts: string[]) => parts.map((x) => p(x) + suffix).join(", ");
/** Selectors under High contrast. */
const hc = (...parts: string[]) =>
  parts.map((x) => `:root[data-nail-mode="high-contrast"] ${p(x)}`).join(", ");

const T = {
  bg: "var(--nail-color-bg)",
  subtle: "var(--nail-color-bg-subtle)",
  raised: "var(--nail-color-bg-raised)",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  line: "var(--nail-color-border-strong, var(--nail-color-border))",
  accent: "var(--nail-color-accent)",
  accentFg: "var(--nail-color-accent-fg)",
  accentSoft:
    "var(--nail-color-accent-soft, color-mix(in srgb, var(--nail-color-accent) 14%, var(--nail-color-bg)))",
  selection: "var(--nail-color-selection)",
  selectionFg: "var(--nail-color-selection-fg)",
  danger: "var(--nail-color-danger)",
  focus: "var(--nail-color-focus-ring)",
  fontDisplay: "var(--nail-font-display, var(--nail-font-ui))",
  label: "var(--nail-font-size-label)",
  small: "var(--nail-font-size-small)",
  body: "var(--nail-font-size-body)",
  heading: "var(--nail-font-size-heading)",
  medium: "var(--nail-font-weight-medium)",
  semibold: "var(--nail-font-weight-semibold)",
  tight: "var(--nail-line-height-tight)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  lg: "var(--nail-space-lg)",
  pane: "var(--nail-space-pane)",
  icon: "var(--nail-size-icon)",
  control: "var(--nail-size-control)",
  avatar: "var(--nail-size-avatar)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  pill: "var(--nail-radius-pill)",
  raisedShadow: "var(--nail-shadow-raised)",
  fast: "var(--nail-motion-fast)",
  ease: "var(--nail-motion-ease)",
} as const;

const focusRing = `outline: 2px solid ${T.focus}; outline-offset: 2px;`;
/** One line, cut with an ellipsis. */
const oneLine = "min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;";

const TOOLBAR = `${p("calendar.toolbar")} {
  display: flex; flex-wrap: nowrap; align-items: center; gap: ${T.sm};
  min-width: 0; min-height: calc(${T.control} + 2 * ${T.sm}); padding: ${T.sm} ${T.pane};
  border-bottom: 1px solid ${T.border}; background-color: ${T.bg}; overflow: hidden;
}
${p("calendar.toolbar")} > * { flex: 0 0 auto; }
${p("calendar.nav")} { display: flex; flex-wrap: nowrap; align-items: center; gap: ${T.xxs}; }
${p("calendar.title")} {
  flex: 1 1 auto; ${oneLine} margin: 0 0 0 ${T.xs};
  font-family: ${T.fontDisplay}; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight};
}
${p("calendar.toolbar")} > ${p("calendar.title")} { flex: 1 1 auto; }
${p("segmented")}[data-field="calendar-view"] { flex: 0 1 auto; min-width: 0; overflow-x: auto; scrollbar-width: none; }
${p("segmented")}[data-field="calendar-view"] ${p("segmented.option")} { flex: 0 0 auto; white-space: nowrap; }
${p("calendar.view-menu")} { display: inline-flex; align-items: center; gap: ${T.xs}; }`;

/**
 * ADR 0160: a date and time field's row — the date takes the room the row
 * offers (its segments start at the left), the "Choose" button and the time
 * (`data-width="short"`) keep their size.
 */
const DATETIME = `${p("pim.datetime.controls")} > ${p("pim.datetime.date")} { flex: 1 1 10rem; min-width: 0; }
${p("pim.datetime.date")} > ${p("pim.datetime.input")} { display: flex; width: 100%; }
${p("pim.datetime.time")} > ${p("pim.datetime.input")} { display: flex; width: 100%; }`;

const CARD_PARTS = [
  "calendar.event",
  "calendar.chip",
  "calendar.agenda.event",
  "agenda.list.event",
];

const CARDS = `${ps(...CARD_PARTS)} {
  border: none; border-left: 3px solid ${T.accent}; border-radius: ${T.rSm};
  font-size: ${T.small}; line-height: ${T.tight}; text-align: start;
  color: ${T.fg}; background-color: ${T.raised};
  transition: box-shadow ${T.fast} ${T.ease};
}
${pss(":hover", ...CARD_PARTS)} { box-shadow: ${T.raisedShadow}; }
${pss(":focus-visible", ...CARD_PARTS)} { ${focusRing} }
${pss('[data-state~="selected"]', ...CARD_PARTS)} { box-shadow: 0 0 0 2px ${T.accent}; }
${pss('[data-state~="tentative"]', ...CARD_PARTS)} { border-left-style: dashed; }
${pss('[data-state~="free"]', ...CARD_PARTS)} { border-left-style: dotted; }
${p("calendar.event")} { gap: 1px; padding: ${T.xxs} ${T.xs}; }
${pss(':not([data-state~="read-only"])', "calendar.event", "calendar.chip")} { cursor: grab; }
${p("event-card.title")} { ${oneLine} font-weight: ${T.semibold}; color: inherit; }
${p("event-card.time")} { ${oneLine} font-size: ${T.label}; font-variant-numeric: tabular-nums; color: inherit; }
${p("event-card.location")} { ${oneLine} font-size: ${T.label}; color: inherit; }
${p("event-card.badge")} { display: none; }
${p("calendar.event")} > ${p("event-card.title")} {
  flex: 0 0 auto; white-space: normal; overflow-wrap: anywhere;
  display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 3; overflow: hidden;
}
/* P1 (WP192): the time and place keep their line height (a long title in a narrow column squeezed
   them into half a line); a short event shows its title on one line so the time still fits. */
${p("calendar.event")} > ${p("event-card.time")}, ${p("calendar.event")} > ${p("event-card.location")} { flex: 0 0 auto; }
${p("calendar.event")} { container: nail-event / size; }
@container nail-event (max-height: 3.5rem) { ${p("calendar.event")} > ${p("event-card.title")} { -webkit-line-clamp: 1; } }
${ps("calendar.chip", "calendar.agenda.event", "agenda.list.event")} { align-items: baseline; gap: ${T.xs}; padding: 1px ${T.xs}; white-space: nowrap; }
${p("calendar.chip")} > ${p("event-card.time")} { order: -1; flex: 0 0 auto; }
${p("calendar.chip")} > ${p("event-card.location")} { display: none; }
${ps("calendar.agenda.event", "agenda.list.event")} { width: 100%; gap: ${T.md}; padding: ${T.xs} ${T.sm}; }
${ps("calendar.agenda.event", "agenda.list.event")} > ${p("event-card.time")} { order: -1; flex: 0 0 9em; }
${ps("calendar.agenda.event", "agenda.list.event")} > ${p("event-card.title")} { flex: 0 1 auto; }
${ps("calendar.agenda.event", "agenda.list.event")} > ${p("event-card.location")} { flex: 1 1 0; }
${hc(...CARD_PARTS)} {
  background-color: ${T.bg};
  border-top: 2px solid ${T.fg}; border-right: 2px solid ${T.fg}; border-bottom: 2px solid ${T.fg};
  border-left-width: 6px; border-left-style: solid;
}`;

const GRIDS = `${p("calendar.day-head")} { display: flex; flex-direction: column; align-items: center; gap: 0; padding: ${T.xs}; border: 0; line-height: ${T.tight}; font-size: ${T.label}; color: ${T.muted}; background-color: transparent; }
${p("calendar.day-head.date")} { font-family: ${T.fontDisplay}; font-size: ${T.heading}; font-weight: ${T.semibold}; color: ${T.fg}; }
${p("calendar.day-head")}[aria-current="date"] ${p("calendar.day-head.date")} { color: ${T.accent}; }
${p("calendar.hour")} { font-size: ${T.label}; color: ${T.muted}; font-variant-numeric: tabular-nums; }
${p("calendar.month.day")}[aria-current="date"] ${p("calendar.month.date")} {
  display: inline-grid; place-items: center; min-width: 1.75em; border-radius: ${T.pill};
  font-weight: ${T.semibold}; color: ${T.accentFg}; background-color: ${T.accent};
}
${p("calendar.now")} { height: 2px; background-color: ${T.danger}; }
${p("calendar.month")} { height: 100%; }
${p("calendar.month.weekday")} {
  height: calc(${T.label} * 2); padding: ${T.xs}; border-bottom: 1px solid ${T.line};
  font-size: ${T.label}; font-weight: ${T.semibold}; color: ${T.muted}; text-align: center;
}
${p("calendar.month.day")} {
  height: 6em; padding: ${T.xxs}; border: 1px solid ${T.border}; overflow: hidden;
  background-color: ${T.bg};
}
${p("calendar.month.day")}[data-state~="outside"] { color: ${T.muted}; background-color: ${T.subtle}; }
${p("calendar.month.day")}[data-state~="today"] { box-shadow: inset 0 0 0 2px ${T.accent}; }
${p("calendar.month.day")}[data-state~="drop-target"] { color: ${T.selectionFg}; background-color: ${T.selection}; }
${p("calendar.month.day")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("calendar.month.date")} { display: block; margin-bottom: ${T.xxs}; padding: 0 ${T.xxs}; font-size: ${T.small}; font-variant-numeric: tabular-nums; }
${p("calendar.month.events")} { display: grid; gap: 1px; min-width: 0; }
${p("calendar.month.events")} > * { ${oneLine} }
${p("calendar.month.item")} > ${p("calendar.chip")} { width: 100%; max-width: 100%; }
${p("calendar.month.item")} { container-type: inline-size; }
${p("calendar.month.item")} > ${p("calendar.chip")} > ${p("event-card.title")} { flex: 0 1 auto; }
${p("calendar.month.item")} > ${p("calendar.chip")} > ${p("event-card.time")} { order: 0; flex: 0 0 auto; }
@container (max-width: 8.5rem) { ${p("calendar.month.item")} > ${p("calendar.chip")} > ${p("event-card.time")} { display: none; } }
${p("calendar.month.more")} {
  padding: 0 ${T.xs}; border: none; border-radius: ${T.rSm}; font-size: ${T.label};
  color: ${T.muted}; background-color: transparent;
}
${p("calendar.month.more")}:hover { color: ${T.fg}; background-color: ${T.raised}; }
${p("calendar.month.more")}:focus-visible { ${focusRing} }
${p("calendar.year.month.title")} {
  caption-side: top; padding: 0 ${T.xxs} ${T.xs}; text-align: start;
  font-family: ${T.fontDisplay}; font-size: ${T.body}; font-weight: ${T.semibold}; color: ${T.fg};
}`;

const MINI_MONTH = `${p("mini-month")} { padding: ${T.sm}; }
${p("mini-month.header")} { display: flex; align-items: center; gap: ${T.xxs}; margin-bottom: ${T.xs}; }
${p("mini-month.title")} { flex: 1; margin: 0; ${oneLine} font-size: ${T.small}; font-weight: ${T.semibold}; text-align: center; }
${p("mini-month.grid")} { width: 100%; border-collapse: separate; border-spacing: 2px; font-size: ${T.label}; font-variant-numeric: tabular-nums; }
${p("mini-month.grid.weekday")} { font-weight: ${T.semibold}; color: ${T.muted}; }
${p("mini-month.grid.day")} { padding: ${T.xxs} 0; border-radius: ${T.rSm}; text-align: center; }
${p("mini-month.grid.day")}:hover { background-color: ${T.raised}; }
${p("mini-month.grid.day")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("mini-month.grid.day")}[data-state~="outside"] { color: ${T.muted}; }
${p("mini-month.grid.day")}[data-state~="busy"] { font-weight: ${T.semibold}; }
${p("mini-month.grid.day")}[data-state~="selected"] { color: ${T.selectionFg}; background-color: ${T.selection}; }
${p("mini-month.grid.day")}[data-state~="today"] { color: ${T.accentFg}; background-color: ${T.accent}; font-weight: ${T.semibold}; }`;

const MENU_BUTTONS = ["calendar-list.add", "calendar-list.menu", "pim.menu-button"];

const CALENDAR_LIST = `${p("calendar-list")} { display: flex; flex-direction: column; gap: ${T.md}; padding: ${T.sm}; }
${p("calendar-list.source")} { display: grid; gap: 1px; }
${p("calendar-list.source.header")} { display: flex; align-items: center; justify-content: space-between; gap: ${T.xs}; padding: 0 0 0 ${T.xs}; }
${p("calendar-list.source.name")} {
  margin: 0; ${oneLine} font-size: ${T.label}; font-weight: ${T.semibold};
  letter-spacing: 0.05em; text-transform: uppercase; color: ${T.muted};
}
${ps(...MENU_BUTTONS)} {
  display: inline-grid; place-items: center; flex: 0 0 auto; width: calc(${T.control} - ${T.sm}); height: calc(${T.control} - ${T.sm});
  padding: 0; border: none; border-radius: ${T.rSm}; color: ${T.muted}; background-color: transparent;
}
${pss(":hover", ...MENU_BUTTONS)} { color: ${T.fg}; background-color: ${T.raised}; }
${pss(":focus-visible", ...MENU_BUTTONS)} { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${pss('[aria-expanded="true"]', ...MENU_BUTTONS)} { color: ${T.fg}; background-color: ${T.raised}; }
${ps("calendar-list.add.icon", "calendar-list.menu.icon", "pim.menu-button.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("calendar-list.item")} {
  display: grid; grid-template-columns: minmax(0, 1fr) auto; align-items: center; gap: ${T.xs};
  min-height: calc(${T.control} - ${T.xs}); padding: 0 0 0 ${T.xs}; border-radius: ${T.rSm};
}
${p("calendar-list.item")}:hover { background-color: ${T.raised}; }
${p("calendar-list.label")} { display: flex; align-items: center; gap: ${T.sm}; min-width: 0; font-size: ${T.body}; }
${p("calendar-list.toggle")} {
  appearance: none; flex: 0 0 auto; width: 14px; height: 14px; margin: 0;
  border: 2px solid ${T.accent}; border-radius: 4px; background-color: ${T.accent};
}
${p("calendar-list.toggle")}:not(:checked) { background-color: transparent; }
${p("calendar-list.toggle")}:focus-visible { ${focusRing} }
${p("calendar-list.name")} { ${oneLine} }
${p("calendar-list.item")}[data-state~="hidden"] ${p("calendar-list.name")} { color: ${T.muted}; }
${p("calendar-list.kind")} {
  flex: 0 0 auto; padding: 0 ${T.xs}; border-radius: ${T.rSm};
  font-size: ${T.label}; color: ${T.muted}; background-color: ${T.subtle};
}
${p("calendar-list.buttons")} { display: inline-flex; gap: ${T.xxs}; }
${p("calendar-list.item")} ${p("calendar-list.menu")} { opacity: 0; }
${p("calendar-list.item")}:hover ${p("calendar-list.menu")}, ${p("calendar-list.item")}:focus-within ${p("calendar-list.menu")}, ${p("calendar-list.menu")}[aria-expanded="true"] { opacity: 1; }
@media (hover: none) { ${p("calendar-list.item")} ${p("calendar-list.menu")} { opacity: 1; } }
${p("context-menu.item.swatch")} { grid-column: 1; width: 12px; height: 12px; border-radius: 4px; background-color: ${T.accent}; }
${p("context-menu.item.check")} { grid-column: 3; width: ${T.icon}; height: ${T.icon}; color: ${T.accent}; }
${p("context-menu.item")}[data-tone="danger"] { color: ${T.danger}; }
${p("context-menu.item")}[data-tone="danger"] ${p("context-menu.item.icon")} { color: ${T.danger}; }`;

const DIALOGS = `${p("pim.calendar-dialog")} { display: flex; flex-direction: column; gap: ${T.md}; min-width: min(360px, 100%); padding: ${T.lg}; }
${p("pim.calendar-dialog.question")} { margin: 0; max-width: 60ch; }
${p("pim.colors")} { display: grid; gap: ${T.sm}; margin: 0; padding: 0; border: none; min-width: 0; }
${p("pim.colors.options")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; }
${p("pim.colors.option")} { position: relative; display: inline-grid; place-items: center; width: ${T.control}; height: ${T.control}; border-radius: ${T.pill}; }
${p("pim.colors.input")} { position: absolute; inset: 0; width: 100%; height: 100%; margin: 0; opacity: 0; }
${p("pim.colors.swatch")} { width: calc(${T.control} - ${T.md}); height: calc(${T.control} - ${T.md}); border-radius: ${T.pill}; background-color: ${T.accent}; }
${p("pim.colors.option")}:has(:checked) { box-shadow: 0 0 0 2px ${T.fg}; }
${p("pim.colors.option")}:has(:focus-visible) { outline: 2px solid ${T.focus}; outline-offset: 2px; }
${p("pim.colors.name")} { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
${p("pim-settings")} { display: grid; gap: ${T.lg}; }`;

const avatars = AVATAR_COLORS.map(
  (color, i) =>
    `[data-color="${String(i)}"]${p("contact-list.avatar")}, [data-color="${String(i)}"]${p("contact-card.avatar")} { background-color: var(--nail-color-avatar-${String(i + 1)}, ${color}); }`,
).join("\n");

const CONTACTS = `${ps("contact-list.avatar", "contact-card.avatar")} {
  display: grid; place-items: center; border-radius: 50%; font-weight: ${T.semibold}; line-height: 1;
  color: var(--nail-color-avatar-fg, #ffffff); user-select: none;
}
${p("contact-list.avatar")} { width: ${T.avatar}; height: ${T.avatar}; font-size: calc(${T.avatar} * 0.37); }
${p("contact-card.avatar")} { width: 72px; height: 72px; flex: 0 0 auto; font-size: 28px; }
${avatars}
${p("contact-list.items")} { display: flex; flex-direction: column; gap: 1px; }
${p("contact-list.item")} {
  display: grid; grid-template-columns: ${T.avatar} minmax(0, 1fr); column-gap: ${T.md}; align-items: center;
  padding: ${T.sm} ${T.md}; border-radius: ${T.rMd};
}
${p("contact-list.item")} > :not(${p("contact-list.avatar")}) { grid-column: 2; ${oneLine} }
${p("contact-list.item")}:hover { background-color: ${T.subtle}; }
${p("contact-list.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("contact-list.item")}[aria-selected="true"] { color: ${T.selectionFg}; background-color: ${T.selection}; }
${p("contact-list.name")} { font-weight: ${T.semibold}; }
${ps("contact-list.email", "contact-list.organization", "contact-list.book", "contact-list.linked")} { font-size: ${T.small}; color: ${T.muted}; }
${p("contact-list.item")}[aria-selected="true"] > :not(${p("contact-list.avatar")}) { color: inherit; }
${p("contact-card.header")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.lg}; padding-bottom: ${T.md}; border-bottom: 1px solid ${T.border}; }
${p("contact-card.name")} { margin: 0; font-family: ${T.fontDisplay}; font-weight: ${T.semibold}; }
${p("contact-card.heading")} { margin: 0 0 ${T.xs}; font-size: ${T.label}; font-weight: ${T.semibold}; letter-spacing: 0.05em; text-transform: uppercase; color: ${T.muted}; }
${p("contact-card.section")} { display: grid; gap: ${T.xs}; }`;

const VIEW_HEADER = `${p("pim.view-header")} {
  display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm} ${T.md};
  padding-bottom: ${T.sm}; border-bottom: 1px solid ${T.border};
}
${p("pim.view-header.title")} {
  flex: 1 1 auto; margin: 0; ${oneLine}
  font-family: ${T.fontDisplay}; font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight};
}
${p("pim.view-header")} > [data-field="search"] { flex: 1 1 100%; order: 3; display: grid; gap: ${T.xxs}; }
${p("pim.view-header.actions")} { display: flex; flex: 0 0 auto; align-items: center; gap: ${T.xs}; }`;

const TASKS = `${p("task-list.items")} { display: flex; flex-direction: column; gap: 1px; }
${p("task-list.item")} {
  display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm};
  min-height: ${T.control}; padding: ${T.xxs} ${T.sm}; border-radius: ${T.rMd};
}
${p("task-list.item")}:hover { background-color: ${T.subtle}; }
${p("task-list.item")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("task-list.check")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; margin: 0; accent-color: ${T.accent}; }
${p("task-list.title")} { flex: 1 1 12em; min-width: 0; padding: 0; border: 0; text-align: start; overflow-wrap: anywhere; color: inherit; background-color: transparent; }
${ps("task-list.due", "task-list.priority", "task-list.subtasks", "task-list.badge")} {
  flex: 0 0 auto; padding: 0 ${T.sm}; border-radius: ${T.pill};
  font-size: ${T.label}; line-height: 1.6; color: ${T.muted}; background-color: ${T.subtle};
}
${p("task-list.due")}[data-state~="overdue"] { color: ${T.danger}; font-weight: ${T.semibold}; }
${p("task-list.priority")}[data-priority="high"] { color: ${T.danger}; }
${p("task-list.toggle")} { display: inline-grid; place-items: center; padding: 0; border: none; border-radius: ${T.rSm}; color: ${T.muted}; background-color: transparent; }
${p("task-list.toggle.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("task-list.item")}[data-level="2"] { padding-left: calc(${T.sm} + 28px); }
${p("task-list.item")}[data-level="3"] { padding-left: calc(${T.sm} + 56px); }
${p("task-list.item")}[data-level="4"] { padding-left: calc(${T.sm} + 84px); }
${p("task-list.item")}[data-state~="current"] { background-color: ${T.subtle}; box-shadow: inset 3px 0 0 ${T.accent}; }
${p("task-list.item")}[data-state~="done"] ${p("task-list.title")} { color: ${T.muted}; text-decoration: line-through; }
${p("task-list.add")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; padding: ${T.xs} 0; }
${p("task-list.add")} > ${p("form.label")} { flex: 1 0 100%; margin: 0; }
${p("task-list.add")} > ${p("form.input")} { flex: 1 1 12em; min-width: 0; }`;

/** The container the calendar's narrow rules measure (P1 WP192). */
export const CALENDAR_CONTAINER = "nail-calendar";
/** Below this width the calendar toolbar takes two rows. */
export const CALENDAR_NARROW = "40rem";

/** The widest a contact card's or the task list's content gets (Edit / Delete stay near the name). */
const READING = "56rem";

/** Visually hidden, still read (the accessible name stays). */
const srOnly =
  "position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap;";

/**
 * P1 (WP192, ADR 0142): the panes of the calendar window's split give way
 * down to their `min-sizes` through the layout renderer (`flexForSize`, ADR
 * 0144). No `:has()` with a descendant search here: costly in jsdom's style
 * matching, which the axe tests run. The
 * calendar is a size container (`parts.ts`), so its content (the toolbar's
 * buttons, the time grid's columns) never widens the pane it sits in: the
 * panes keep their place at 800 px and the grids
 * fit the pane. In a narrow pane the toolbar takes two rows — the buttons, then the range
 * on its own line instead of a cut-off title — the view menu loses its
 * visible "View" label and "New event" shows its icon only (both keep
 * their names). All-day chips leave out their "All day" (the row says it).
 * The contact filters share the list's width; a contact card's and the
 * task list's content stop at a reading width in a wide window.
 */
const RESPONSIVE = `@container ${CALENDAR_CONTAINER} (max-width: ${CALENDAR_NARROW}) {
  ${p("calendar.toolbar")} { flex-wrap: wrap; row-gap: ${T.xs}; }
  ${p("calendar.toolbar")} > ${p("calendar.title")} { order: 99; flex: 1 0 100%; margin: 0; }
  ${p("calendar.toolbar")} > [data-field="calendar-new-event"] { margin-inline-start: auto; }
  ${p("calendar.view-menu")} > ${p("form.label")} { ${srOnly} }
  [data-field="calendar-new-event"] > ${p("form.button.label")} { ${srOnly} }
}
${p("calendar.all-day.item")} > ${p("calendar.chip")} > ${p("event-card.time")} { display: none; }
${p("contact-list")} > ${p("contact-list.toolbar")} > ${p("form.field")} { flex: 1 1 10rem; min-width: 0; }
${p("contact-list")} > ${p("contact-list.toolbar")} > ${p("form.field")} > ${p("form.select")} { width: 100%; min-width: 0; }
${p("contact-card")} > ${p("contact-card.header")}, ${p("contact-card")} > ${p("contact-card.section")} { width: 100%; max-width: ${READING}; box-sizing: border-box; }
${p("task-list")} > * { width: 100%; max-width: ${READING}; box-sizing: border-box; }`;

/** The PIM skin (empty sections are dropped). */
export const PIM_SKIN_CSS = [
  TOOLBAR,
  RESPONSIVE,
  CARDS,
  GRIDS,
  MINI_MONTH,
  CALENDAR_LIST,
  DIALOGS,
  VIEW_HEADER,
  CONTACTS,
  TASKS,
  DATETIME,
].join("\n");

/**
 * Parts whose rules in `defaults/theme.kdl` predate this skin and override
 * it (the theme's part rules come after the skin). The integration removes
 * those rules so the M5b look shows; a user theme can still restyle them.
 */
export const PIM_SKIN_PARTS: readonly string[] = [
  ...new Set(
    // The responsive and date / time rules select through two parts (or
    // more), so they win over any single-part theme rule anyway.
    [
      ...PIM_SKIN_CSS.replace(RESPONSIVE, "")
        .replace(DATETIME, "")
        .matchAll(/\[data-part="([^"]+)"\]/g),
    ].map((m) => m[1] ?? ""),
  ),
].sort();
