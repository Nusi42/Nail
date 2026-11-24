/**
 * R7 style slot of the views and shell package (P3): toolbar search and results, quick-add field and preview chips, zone chips, the narrow-window drawer and floating New event, agenda time column and now marker, month spanning bars, dot lines and today pill, mini-month band, dots and week numbers, year heat shading, free-time axis, empty states, the Due this week panel, the calendar list and settings.
 *
 * `VIEWS_BASE_CSS` holds structure every theme needs (appended to
 * `PIM_BASE_CSS`, like `parts.ts`); `VIEWS_SKIN_CSS` the look on tokens
 * only (appended to `PIM_SKIN_CSS`, like `skin.ts`). Only that package
 * edits this file during R7 (ADR 0246), so the packages never touch the
 * same stylesheet; the integrator may fold it into `parts.ts` / `skin.ts`.
 *
 * The parts here are new in R7 (no `defaults/theme.kdl` rule styles them),
 * except `calendar.month.day` (its M5b rules live in `skin.ts`; the month
 * cell lets a spanning bar run into the next cells and drops the today
 * frame) and `calendar-list.toggle` (a larger colour square).
 */
import {
  CAPS,
  FOCUS_RING,
  ONE_LINE,
  QUIET_BUTTON,
  QUIET_HOVER,
  VISUALLY_HIDDEN,
} from "../../theme/declarations";
import { hc, p, ps } from "../../theme/selectors";
import { T } from "../../theme/tokens";

/** The container the narrow rules measure (the calendar widget, `parts.ts`). */
const CONTAINER = "nail-calendar";
/** Below this width the shell keeps only icons and the fields shrink. */
const SHELL_NARROW = "34rem";
/** The time column of the agenda. */
const AGENDA_TIME = "6.5em";
/** The calendar dots of the mini-month and the month lines. */
const DOT = "6px";
/** The agenda's and the search results' colour dot. */
const AGENDA_DOT = "10px";
/** The drawer's width (at most the calendar's, less a strip to close it by). */
const DRAWER = "min(20rem, calc(100% - 3rem))";
/** The floating New event button. */
const FAB = "56px";
/** The calendar list's colour squares. */
const SQUARE = "16px";
/** One lane (bar or line) of a month cell. */
const LANE = "1.4em";
/** A free stretch narrower than this shows only its start time ("8:00 AM"). */
const SLOT_FULL = "9em";
/** A free stretch narrower than this shows no text (its accessible name keeps the times). */
const SLOT_START = "4.5em";
/** The free-time axis' height. */
const AXIS = "1.4em";
/** The zone chip's widest and the search field's basis. */
const CHIP_MAX = "14rem";
/** The quick-add field's basis. */
const FIELD_BASIS = "12rem";
/** The narrowest basis of a shrinking field (search, the Due this week field). */
const SHRINK_BASIS = "8rem";
/** The zone popover's width. */
const POPOVER_WIDTH = "22rem";
/** The search results' width and its share of the viewport. */
const RESULTS_WIDTH = "26rem";
const RESULTS_VIEWPORT = "90vw";
/** The zone popover keeps this far from the viewport's edges. */
const VIEWPORT_WIDTH = "100vw";
/** The leading edge of a month bar. */
const BAR_EDGE = "4px";
/** The gap ring around today in the year view. */
const TODAY_RING = "3px";
/** A busy block's text line (the track's height less its borders). */
const TRACK_LINE = "30px";
/** Heat levels 1 … 4 of the year view: per cent of the accent in the day's background. */
const HEAT = [18, 34, 52, 72] as const;
/** From this heat level on the day's number takes the accent's text colour. */
const HEAT_DARK = 52;

const list = (...parts: string[]) => `${ps(...parts)} { list-style: none; margin: 0; padding: 0; }`;
const mix = (color: string, pct: number) => `color-mix(in srgb, ${color} ${String(pct)}%, ${T.bg})`;

/** Structure of the package's parts (selectors through `[data-part]` only; one rule per line). */
export const VIEWS_BASE_CSS = [
  `${p("calendar")} { position: relative; }`,
  list(
    "calendar.month.lanes",
    "calendar.quick-add.chips",
    "calendar.search.group",
    "calendar.agenda.rows",
    "agenda.list.rows",
    "due-tasks.items",
  ),
  `${p("calendar.tools")} { display: flex; flex-wrap: wrap; align-items: flex-start; min-width: 0; }`,
  `${p("calendar.quick-add")} { display: flex; flex-wrap: wrap; align-items: center; flex: 1 1 ${FIELD_BASIS}; min-width: 0; }`,
  `${p("calendar.quick-add")} > ${p("form.input")} { flex: 1 1 ${FIELD_BASIS}; min-width: 0; }`,
  `${p("calendar.quick-add.chips")} { display: flex; flex-wrap: wrap; align-items: center; flex: 1 0 100%; min-width: 0; }`,
  `${p("calendar.quick-add.status")}:empty { display: none; }`,
  `${p("calendar.search")} { position: relative; display: flex; align-items: center; min-width: 0; }`,
  `${p("calendar.search")} > ${p("form.input")} { min-width: 0; }`,
  `${p("calendar.search.results")} { position: absolute; top: 100%; inset-inline-end: 0; z-index: 30; overflow-y: auto; }`,
  `${p("calendar.search.results")}[hidden] { display: none; }`,
  `${p("calendar.search.hit")} { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; align-items: baseline; cursor: pointer; }`,
  `${p("calendar.drawer")} { position: absolute; top: 0; bottom: 0; inset-inline-start: 0; z-index: 40; display: flex; flex-direction: column; overflow-y: auto; }`,
  `${p("calendar.drawer.header")} { display: flex; align-items: center; justify-content: space-between; }`,
  `${p("calendar.fab")} { position: absolute; inset-inline-end: ${T.lg}; bottom: ${T.lg}; z-index: 35; display: inline-grid; place-items: center; }`,
  `${p("calendar.month.lane")} { position: relative; height: ${LANE}; }`,
  `${p("calendar.month.bar")} { position: relative; z-index: 1; display: flex; box-sizing: border-box; height: 100%; align-items: center; text-align: start; font: inherit; ${ONE_LINE} }`,
  `${p("calendar.month.line")} { display: flex; align-items: center; text-align: start; font: inherit; ${ONE_LINE} }`,
  `${ps("calendar.agenda.day-head", "agenda.list.day-head")} { position: sticky; top: 0; z-index: 1; }`,
  `${ps("calendar.agenda.entry", "agenda.list.entry")} { display: grid; grid-template-columns: ${AGENDA_TIME} auto minmax(0, 1fr); align-items: baseline; width: 100%; text-align: start; font: inherit; }`,
  `${ps("calendar.agenda.place", "agenda.list.place")} { grid-column: 3; }`,
  `${p("calendar.free-time.axis")} { position: relative; height: ${AXIS}; }`,
  `${p("calendar.free-time.tick")} { position: absolute; top: 0; transform: translateX(-50%); white-space: nowrap; }`,
  `${p("calendar.free-time.block")} { position: absolute; top: 0; bottom: 0; box-sizing: border-box; ${ONE_LINE} }`,
  // A free stretch shows its times, only its start when narrow, nothing (but its name) when tiny: never a cut text.
  `${p("calendar.free-time.slot")} { container-type: inline-size; }`,
  `${p("calendar.free-time.slot.text")}[data-size="start"] { display: none; }`,
  `@container (max-width: ${SLOT_FULL}) { ${p("calendar.free-time.slot.text")}[data-size="full"] { display: none; } ${p("calendar.free-time.slot.text")}[data-size="start"] { display: inline; } }`,
  `@container (max-width: ${SLOT_START}) { ${p("calendar.free-time.slot.text")}[data-size="start"] { display: none; } }`,
  `${p("calendar.free-time.head")} { display: flex; align-items: baseline; justify-content: space-between; gap: 1em; }`,
  `${p("pim.empty")} { display: flex; flex-direction: column; align-items: center; text-align: center; }`,
  `${p("pim.empty.actions")} { display: flex; flex-wrap: wrap; justify-content: center; }`,
  `${p("due-tasks.item")} { display: flex; align-items: baseline; min-width: 0; }`,
  `${p("due-tasks.name")} { flex: 1 1 auto; ${ONE_LINE} }`,
  `${p("calendar-prefs.hours")} { display: flex; flex-wrap: wrap; }`,
  `${p("calendar-prefs.days")} { display: flex; flex-wrap: wrap; margin: 0; min-width: 0; }`,
].join("\n");

const TOOLBAR = `${p("calendar.zone-chip")} {
  display: inline-flex; align-items: center; gap: ${T.xxs}; max-width: ${CHIP_MAX}; min-height: ${T.control};
  padding: 0 ${T.sm}; border: 1px solid ${T.border}; border-radius: ${T.pill};
  font: inherit; font-size: ${T.label}; color: ${T.muted}; background-color: ${T.bg};
}
${p("calendar.zone-chip")}:hover { color: ${T.fg}; background-color: ${T.raised}; }
${p("calendar.zone-chip")}:focus-visible { ${FOCUS_RING} }
${ps("calendar.zone-chip.icon", "calendar.search.icon", "calendar.quick-add.icon")} { width: ${T.icon}; height: ${T.icon}; flex: 0 0 auto; color: ${T.muted}; }
${p("calendar.zone-chip.label")} { ${ONE_LINE} }
${p("calendar.zone-popover")} {
  display: flex; flex-direction: column; gap: ${T.sm}; width: min(${POPOVER_WIDTH}, calc(${VIEWPORT_WIDTH} - 2 * ${T.md}));
  padding: ${T.md}; border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.bg}; box-shadow: ${T.overlay};
}
${p("calendar.zone-popover.body")} { display: flex; flex-direction: column; gap: ${T.sm}; }
${p("calendar.zone-popover.title")} { margin: 0; font-size: ${T.body}; font-weight: ${T.semibold}; }
${p("calendar.zone-popover.hint")} { margin: 0; font-size: ${T.small}; color: ${T.muted}; }
${p("calendar.search")} { gap: ${T.xxs}; flex: 0 1 ${CHIP_MAX}; }
${p("calendar.search")} > ${p("form.input")} { flex: 1 1 auto; width: 100%; }
${p("calendar.search.results")} {
  width: min(${RESULTS_WIDTH}, ${RESULTS_VIEWPORT}); max-height: 24rem; margin-top: ${T.xxs}; padding: ${T.xs} 0;
  border: 1px solid ${T.border}; border-radius: ${T.rMd}; background-color: ${T.bg}; box-shadow: ${T.overlay};
}
${p("calendar.search.group.title")} { padding: ${T.xs} ${T.md} ${T.xxs}; ${CAPS} font-size: ${T.label}; font-weight: ${T.semibold}; color: ${T.muted}; }
${p("calendar.search.hit")} { gap: ${T.xs} ${T.sm}; padding: ${T.xs} ${T.md}; font-size: ${T.body}; }
${p("calendar.search.hit")}:hover, ${p("calendar.search.hit")}[data-state~="active"] { color: ${T.selectionFg}; background-color: ${T.selection}; }
${p("calendar.search.hit.dot")} { width: ${AGENDA_DOT}; height: ${AGENDA_DOT}; border-radius: 50%; background-color: ${T.accent}; }
${p("calendar.search.hit.title")} { ${ONE_LINE} }
${p("calendar.search.hit.when")} { font-size: ${T.small}; opacity: 0.8; font-variant-numeric: tabular-nums; }
${p("calendar.search.hit.detail")} { grid-column: 2 / -1; font-size: ${T.small}; opacity: 0.8; }
${p("calendar.search.empty")} { margin: 0; padding: ${T.sm} ${T.md}; color: ${T.muted}; }
${p("calendar.tools")} { gap: ${T.xs} ${T.md}; padding: ${T.xs} ${T.sm}; border-bottom: 1px solid ${T.border}; }
${p("calendar.quick-add")} { gap: ${T.xs} ${T.sm}; }
${p("calendar.quick-add.chips")} { gap: ${T.xs}; }
${p("calendar.quick-add.chip")} {
  display: inline-flex; align-items: baseline; gap: ${T.xxs}; padding: 1px ${T.sm}; border: 1px solid ${T.border};
  border-radius: ${T.pill}; font-size: ${T.label}; background-color: ${T.subtle};
}
${p("calendar.quick-add.chip")}[data-kind="title"] { border-color: ${T.accent}; background-color: ${T.accentSoft}; }
${p("calendar.quick-add.chip.kind")} { color: ${T.muted}; }
${p("calendar.quick-add.chip.text")} { font-weight: ${T.semibold}; color: ${T.fg}; }
${p("calendar.quick-add.hint")} { font-size: ${T.label}; color: ${T.muted}; }
${p("calendar.quick-add.status")} { flex: 1 0 100%; margin: 0; font-size: ${T.small}; color: ${T.muted}; }
@container ${CONTAINER} (max-width: ${SHELL_NARROW}) {
  ${p("calendar.zone-chip.label")} { ${VISUALLY_HIDDEN} }
  ${p("calendar.search")} { flex: 1 1 ${SHRINK_BASIS}; }
  ${p("calendar.quick-add.hint")} { ${VISUALLY_HIDDEN} }
}`;

const SHELL = `${p("calendar.drawer")} {
  width: ${DRAWER}; gap: ${T.sm}; padding: ${T.sm}; border-inline-end: 1px solid ${T.border};
  background-color: ${T.bg}; box-shadow: ${T.overlay};
}
${p("calendar.drawer.title")} { margin: 0; font-size: ${T.body}; font-weight: ${T.semibold}; }
${p("calendar.fab")} {
  width: ${FAB}; height: ${FAB}; padding: 0; border: none; border-radius: 50%;
  color: ${T.accentFg}; background-color: ${T.accent}; box-shadow: ${T.overlay}; cursor: pointer;
}
${p("calendar.fab")}:hover { filter: brightness(1.08); }
${p("calendar.fab")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: ${TODAY_RING}; }
${p("calendar.fab.icon")} { width: calc(${T.icon} * 1.4); height: calc(${T.icon} * 1.4); }
${hc("calendar.fab")} { border: 2px solid ${T.fg}; }`;

const MONTH = `${p("calendar.month.day")} { overflow-x: visible; overflow-y: clip; }
${p("calendar.month.lanes")} { margin-bottom: 1px; }
${p("calendar.month.bar")} { gap: ${T.xs}; padding: 0 ${T.xs}; border: 0; border-left: ${BAR_EDGE} solid ${T.accent}; border-radius: ${T.rSm}; font-size: ${T.label}; color: ${T.fg}; background-color: ${T.accentSoft}; cursor: pointer; }
${p("calendar.month.bar")}[data-state~="continues-before"] { border-start-start-radius: 0; border-end-start-radius: 0; }
${p("calendar.month.bar")}[data-state~="continues-after"] { border-start-end-radius: 0; border-end-end-radius: 0; }
${ps("calendar.month.bar")} > ${p("event-card.time")}, ${p("calendar.month.bar")} > ${p("event-card.location")}, ${p("calendar.month.bar")} > ${p("event-card.badge")} { display: none; }
${p("calendar.month.bar")} > ${p("event-card.title")} { ${ONE_LINE} font-weight: ${T.medium}; }
${p("calendar.month.bar")}:focus-visible, ${p("calendar.month.line")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("calendar.month.line")}[data-geom] { gap: ${T.xs}; width: 100%; padding: 0 ${T.xxs}; border-style: none; border-width: 0; border-radius: ${T.rSm}; font-size: ${T.label}; color: ${T.fg}; background-color: transparent; cursor: pointer; }
${p("calendar.month.line")}::before { content: ""; order: -2; flex: 0 0 auto; width: 0; height: ${DOT}; border-left: ${DOT} solid ${T.accent}; border-left-color: inherit; border-radius: 50%; }
${p("calendar.month.line")}:hover { background-color: ${T.raised}; }
${p("calendar.month.line")} > ${p("event-card.time")} { order: -1; flex: 0 0 auto; color: ${T.muted}; font-variant-numeric: tabular-nums; }
${p("calendar.month.line")} > ${p("event-card.title")} { ${ONE_LINE} }
${p("calendar.month.line")} > ${p("event-card.location")}, ${p("calendar.month.line")} > ${p("event-card.badge")} { display: none; }
${hc("calendar.month.bar")} { border: 1px solid ${T.fg}; border-left-width: ${BAR_EDGE}; background-color: ${T.bg}; }`;

const MINI_YEAR = `${p("mini-month.grid.day")}[data-band] { background-color: ${T.accentSoft}; border-radius: 0; }
${p("mini-month.grid.day")}[data-band="start"] { border-start-start-radius: ${T.pill}; border-end-start-radius: ${T.pill}; }
${p("mini-month.grid.day")}[data-band="end"] { border-start-end-radius: ${T.pill}; border-end-end-radius: ${T.pill}; }
${p("mini-month.grid.day")}[data-band="single"] { border-radius: ${T.pill}; }
${p("mini-month.grid.day")}[data-state~="today"] { color: ${T.accentFg}; background-color: ${T.accent}; }
${p("mini-month.grid.number")} { display: block; line-height: ${T.tight}; }
${p("mini-month.grid.dots")} { display: flex; justify-content: center; gap: 2px; height: ${DOT}; margin-top: 1px; }
${p("mini-month.grid.dot")} { width: ${DOT}; height: ${DOT}; border-radius: 50%; background-color: ${T.accent}; }
${p("mini-month.grid.day")}[data-state~="today"] ${p("mini-month.grid.dot")} { box-shadow: 0 0 0 1px ${T.accentFg}; }
${ps("mini-month.grid.week-number", "calendar.year.grid.week-number")} { padding: 0 ${T.xxs}; font-size: ${T.label}; font-weight: ${T.regular}; color: ${T.muted}; text-align: center; font-variant-numeric: tabular-nums; }
${ps("mini-month.grid.corner", "calendar.year.grid.corner")} { font-size: ${T.label}; font-weight: ${T.regular}; color: ${T.muted}; }
${p("calendar.year.grid")} { border-collapse: separate; border-spacing: 2px; font-size: ${T.label}; font-variant-numeric: tabular-nums; }
${p("calendar.year.grid.title")} { caption-side: top; padding: 0 ${T.xxs} ${T.xxs}; text-align: start; ${CAPS} font-size: ${T.label}; font-weight: ${T.semibold}; color: ${T.accent}; }
${p("calendar.year.grid.weekday")} { font-weight: ${T.semibold}; color: ${T.muted}; }
${p("calendar.year.grid.day")} { padding: ${T.xxs}; border-radius: ${T.rSm}; text-align: center; cursor: pointer; }
${p("calendar.year.grid.day")}:hover { outline: 1px solid ${T.border}; }
${p("calendar.year.grid.day")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p("calendar.year.grid.day")}[data-state~="outside"] { visibility: hidden; }
${HEAT.map(
  (pct, i) =>
    `${p("calendar.year.grid.day")}[data-heat="${String(i + 1)}"] { background-color: ${mix(T.accent, pct)};${pct >= HEAT_DARK ? ` color: ${T.accentFg};` : ""} }`,
).join("\n")}
${p("calendar.year.grid.day")}[data-state~="today"] { box-shadow: inset 0 0 0 2px ${T.accent}, inset 0 0 0 ${TODAY_RING} ${T.bg}; font-weight: ${T.semibold}; }
${hc("calendar.year.grid.day")}[data-heat]:not([data-heat="0"]) { outline: 1px solid ${T.fg}; }`;

const agenda = (part: string) => `${p(`${part}.day`)} + ${p(`${part}.day`)} { margin-top: ${T.sm}; }
${p(`${part}.day-head`)} { margin: 0; padding: ${T.xs}; border-bottom: 1px solid ${T.border}; font-size: ${T.small}; font-weight: ${T.semibold}; color: ${T.muted}; background-color: ${T.bg}; }
${p(`${part}.day`)}[data-state="today"] > ${p(`${part}.day-head`)} { color: ${T.accent}; }
${p(`${part}.rows`)} { display: flex; flex-direction: column; gap: 1px; padding-top: ${T.xxs}; }
${p(`${part}.entry`)} { gap: ${T.sm}; padding: ${T.xs}; border: 0; border-radius: ${T.rSm}; color: ${T.fg}; background-color: transparent; cursor: pointer; }
${p(`${part}.entry`)}:hover { background-color: ${T.raised}; }
${p(`${part}.entry`)}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: -2px; }
${p(`${part}.entry`)}[data-state~="past"] { opacity: 0.55; }
${p(`${part}.entry`)}[data-state~="declined"] ${p(`${part}.title`)} { color: ${T.muted}; text-decoration: line-through; }
${p(`${part}.entry`)}[data-state~="needs-action"] ${p(`${part}.dot`)} { border-style: dashed; background-color: transparent; }
${p(`${part}.time`)} { display: flex; flex-direction: column; font-size: ${T.small}; font-variant-numeric: tabular-nums; }
${p(`${part}.time.end`)} { color: ${T.muted}; }
${p(`${part}.entry`)}[data-time="until"] ${p(`${part}.time`)}, ${p(`${part}.entry`)}[data-time="all-day"] ${p(`${part}.time`)} { color: ${T.muted}; }
${p(`${part}.dot`)} { align-self: center; width: ${AGENDA_DOT}; height: ${AGENDA_DOT}; box-sizing: border-box; border: 2px solid ${T.accent}; border-radius: 50%; background-color: ${T.accent}; }
${p(`${part}.title`)} { ${ONE_LINE} font-weight: ${T.medium}; }
${p(`${part}.place`)} { ${ONE_LINE} font-size: ${T.small}; color: ${T.muted}; }
${p(`${part}.now`)} { display: flex; align-items: center; gap: ${T.xs}; margin: 1px 0; font-size: ${T.label}; font-weight: ${T.semibold}; color: ${T.danger}; }
${p(`${part}.now`)}::after { content: ""; flex: 1 1 auto; height: 2px; border-radius: 1px; background-color: ${T.danger}; }`;

const FREE_TIME = `${p("calendar.free-time.axis")} { margin: 0 0 calc(-1 * ${T.xs}); font-size: ${T.label}; color: ${T.muted}; font-variant-numeric: tabular-nums; }
${p("calendar.free-time.tick")}:first-child { transform: none; }
${p("calendar.free-time.tick")}:last-child { transform: translateX(-100%); }
${p("calendar.free-time.total")} { font-size: ${T.small}; color: ${T.success}; font-variant-numeric: tabular-nums; }
${p("calendar.free-time.total")}[data-state~="none"] { color: ${T.muted}; }
${p("calendar.free-time.block")} { padding: 0 ${T.xxs}; border-inline-start: 2px solid ${T.muted}; font-size: ${T.label}; line-height: ${TRACK_LINE}; color: ${T.fg}; background-color: ${T.raised}; }
@container ${CONTAINER} (max-width: ${SHELL_NARROW}) { ${p("calendar.free-time.tick")}:nth-child(even) { visibility: hidden; } }`;

const EMPTY = `${p("pim.empty")} { gap: ${T.sm}; margin: auto; padding: ${T.xl} ${T.md}; max-width: 28rem; color: ${T.muted}; }
${p("pim.empty.icon")} { width: calc(${T.icon} * 2.5); height: calc(${T.icon} * 2.5); color: ${T.accent}; opacity: 0.7; }
${p("pim.empty.text")} { margin: 0; font-size: ${T.body}; color: ${T.fg}; }
${p("pim.empty.actions")} { gap: ${T.sm}; }
${p("pim.empty.nearest")} { padding: ${T.xxs} ${T.xs}; border: 0; border-radius: ${T.rSm}; font: inherit; font-size: ${T.small}; color: ${T.accent}; background-color: transparent; text-decoration: underline; cursor: pointer; }
${p("pim.empty.nearest")}:focus-visible { ${FOCUS_RING} }`;

const SIDEBAR = `${p("due-tasks")} { display: flex; flex-direction: column; gap: ${T.xs}; padding: ${T.sm}; border-top: 1px solid ${T.border}; }
${p("due-tasks.title")} { margin: 0; padding: 0 ${T.xs}; ${CAPS} font-size: ${T.label}; font-weight: ${T.semibold}; color: ${T.muted}; }
${p("due-tasks.items")} { display: flex; flex-direction: column; gap: 1px; }
${p("due-tasks.item")} { gap: ${T.sm}; padding: ${T.xxs} ${T.xs}; border-radius: ${T.rSm}; font-size: ${T.body}; cursor: grab; }
${p("due-tasks.item")}:hover { background-color: ${T.raised}; }
${p("due-tasks.due")} { flex: 0 0 auto; font-size: ${T.label}; color: ${T.muted}; font-variant-numeric: tabular-nums; }
${p("due-tasks.item")}[data-state~="overdue"] ${p("due-tasks.due")} { color: ${T.danger}; }
${ps("due-tasks.plan", "calendar.task.plan")} { display: inline-grid; place-items: center; flex: 0 0 auto; margin-inline-start: auto; padding: 1px; ${QUIET_BUTTON} cursor: pointer; }
${ps("due-tasks.plan", "calendar.task.plan")}:hover { ${QUIET_HOVER} }
${ps("due-tasks.plan", "calendar.task.plan")}:focus-visible { ${FOCUS_RING} }
${ps("due-tasks.plan.icon", "calendar.task.plan.icon")} { width: 1em; height: 1em; }
${p("due-tasks.empty")} { margin: 0; padding: 0 ${T.xs}; font-size: ${T.small}; color: ${T.muted}; }
${p("calendar-list.toggle")} { width: ${SQUARE}; height: ${SQUARE}; }
${p("calendar-list.toggle")}:checked { box-shadow: inset 0 0 0 2px ${T.bg}; }
${p("calendar-prefs")} { display: flex; flex-direction: column; gap: ${T.md}; }
${p("calendar-prefs.hours")} { gap: ${T.md}; }
${p("calendar-prefs.days")} { gap: ${T.xs} ${T.md}; padding: 0; border: 0; }
${p("calendar-prefs.days")} > ${p("form.label")} { flex: 1 0 100%; padding: 0; }
${p("calendar-prefs.notice")} { margin: 0; font-size: ${T.small}; color: ${T.success}; }`;

/** Look of the package's parts (tokens only, `var(--nail-…)`). */
export const VIEWS_SKIN_CSS = [
  TOOLBAR,
  SHELL,
  MONTH,
  MINI_YEAR,
  agenda("calendar.agenda"),
  agenda("agenda.list"),
  FREE_TIME,
  EMPTY,
  SIDEBAR,
].join("\n");
