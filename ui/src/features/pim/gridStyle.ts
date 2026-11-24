/**
 * R7 style slot of the time grid package (P1): event cards (short one-line cards, icons, hatched free time, cascaded overlaps), today circle, now line with dot, faint line and gutter label, drag-to-create ghost and inline title form, off-hours and weekend shading, week number corner, tasks in the all-day row.
 *
 * `GRID_BASE_CSS` holds structure every theme needs (appended to
 * `PIM_BASE_CSS`, like `parts.ts`); `GRID_SKIN_CSS` the look on tokens
 * only (appended to `PIM_SKIN_CSS`, like `skin.ts`). Only that package
 * edits this file during R7 (ADR 0246), so the packages never touch the
 * same stylesheet; the integrator may fold it into `parts.ts` / `skin.ts`.
 */
import { ACCENT_BAR, ONE_LINE } from "../../theme/declarations";
import { HC, p, ps, pss } from "../../theme/selectors";
import { T } from "../../theme/tokens";

/** R7 M3: the smallest height of a short event card (a 15-minute event is never a sliver). */
export const SHORT_CARD_MIN_HEIGHT = "18px";

/** The top, right and bottom border of an event card in High contrast (`skin.ts`). */
export const HC_CARD_BORDER = "2px";

/** R7 S4: below this card width the icons give their room to the title. */
const ICONS_MIN_CARD = "6rem";

/** R7 M6: the dot at the start of today's now line. */
const NOW_DOT = "8px";

/** The draft of a new event is drawn above every cascade level. */
const DRAFT_LAYER = "50";

/** Diagonal hatching in `color` (time outside the working hours, free events). */
const hatch = (color: string) =>
  `repeating-linear-gradient(135deg, ${color} 0 1px, transparent 1px 6px)`;

/** Cards that carry the R7 states (time grid events and all-day / month chips). */
const CARDS = ["calendar.event", "calendar.chip"];

/** Structure of the package's parts (selectors through `[data-part]` only). */
export const GRID_BASE_CSS = `${p("calendar.gutter")} { position: relative; }
${p("calendar.now-label")} { position: absolute; inset-inline-end: 0; transform: translateY(-50%); pointer-events: none; white-space: nowrap; }
${p("calendar.hour")}[data-state~="near-now"] { visibility: hidden; }
${ps("calendar.off-hours", "calendar.draft", "calendar.slot-range")} { position: absolute; box-sizing: border-box; pointer-events: none; }
${p("calendar.draft")} { z-index: ${DRAFT_LAYER}; overflow: hidden; }
${p("calendar.now")}[data-state~="today"]::before { content: ""; position: absolute; }
${p("calendar.tasks")} { display: contents; }
${p("calendar.task")} { display: flex; align-items: center; min-width: 0; overflow: hidden; }
${p("calendar.task.title")} { ${ONE_LINE} }
${p("calendar.event")}[data-size="short"] { flex-direction: row; align-items: baseline; min-height: ${SHORT_CARD_MIN_HEIGHT}; }
${p("event-card.icons")} { display: inline-flex; flex: 0 0 auto; align-items: center; }
${p("calendar.event")}:not([data-size="short"]) > ${p("event-card.icons")} { position: absolute; top: 0; inset-inline-end: 0; }
@container (max-width: ${ICONS_MIN_CARD}) { ${p("event-card.icons")} { display: none; } }
${p("calendar.new-event")} { display: grid; }
${p("calendar.new-event.actions")} { display: flex; justify-content: flex-end; }`;

/** Look of the package's parts (tokens only, `var(--nail-…)`). */
export const GRID_SKIN_CSS = `${p("calendar.day-head")}[aria-current="date"] ${p("calendar.day-head.weekday")} { color: ${T.accent}; font-weight: ${T.semibold}; }
${p("calendar.day-head")}[aria-current="date"] ${p("calendar.day-head.date")} {
  display: inline-grid; place-items: center; box-sizing: border-box; min-width: 2em; height: 2em; padding: 0 ${T.xxs};
  border-radius: ${T.pill}; color: ${T.accentFg}; background-color: ${T.accent};
}
${p("calendar.day-head")}[data-state~="day-off"], ${p("calendar.off-hours")}[data-state~="day-off"] { background-color: color-mix(in srgb, ${T.fg} 4%, transparent); }
${p("calendar.off-hours")}:not([data-state~="day-off"]) { background-image: ${hatch(`color-mix(in srgb, ${T.fg} 9%, transparent)`)}; }
${p("calendar.week-number")} { padding: ${T.xxs}; font-weight: ${T.semibold}; font-variant-numeric: tabular-nums; }
${p("calendar.now")}[data-state~="week"] { height: 1px; background-color: color-mix(in srgb, ${T.danger} 35%, transparent); }
${p("calendar.now")}[data-state~="today"] { height: 2px; margin-top: -1px; background-color: ${T.danger}; }
${p("calendar.now")}[data-state~="today"]::before {
  top: calc((2px - ${NOW_DOT}) / 2); inset-inline-start: calc(${NOW_DOT} / -2);
  width: ${NOW_DOT}; height: ${NOW_DOT}; border-radius: 50%; background-color: ${T.danger};
}
${p("calendar.now-label")} {
  padding: 0 ${T.xxs}; border-radius: ${T.rSm}; font-size: ${T.label}; font-weight: ${T.semibold};
  font-variant-numeric: tabular-nums; color: ${T.danger}; background-color: ${T.bg};
}
${p("calendar.slot-range")} { border: 2px solid ${T.accent}; border-radius: ${T.rSm}; background-color: color-mix(in srgb, ${T.accent} 15%, transparent); }
${p("calendar.draft")} {
  padding: ${T.xxs} ${T.xs}; border: 2px solid ${T.accent}; border-radius: ${T.rSm};
  font-size: ${T.label}; font-variant-numeric: tabular-nums; color: ${T.fg};
  background-color: color-mix(in srgb, ${T.accent} 20%, ${T.bg}); box-shadow: ${T.raisedShadow};
}
${p("calendar.draft.time")} { display: block; overflow-wrap: anywhere; }
${p("calendar.event")}[data-size="short"] { gap: ${T.xs}; padding-block: 0; }
${p("calendar.event")}[data-size="short"] > ${p("event-card.title")} { display: block; flex: 0 0 auto; max-width: 100%; white-space: nowrap; }
${p("calendar.event")}[data-size="short"] > ${p("event-card.time")} { flex: 0 1 auto; min-width: 0; }
${HC} ${p("calendar.event")}[data-size="short"] { min-height: calc(${SHORT_CARD_MIN_HEIGHT} + 2 * ${HC_CARD_BORDER}); }
${p("calendar.event")}[data-size="short"] > ${p("event-card.icons")}, ${p("calendar.chip")} > ${p("event-card.icons")} { margin-inline-start: auto; }
${p("event-card.icons")} { gap: 2px; padding: 1px; border-radius: ${T.rSm}; color: ${T.muted}; background-color: inherit; }
${p("event-card.icon")} { width: 0.9em; height: 0.9em; flex: 0 0 auto; }
${pss('[data-state~="stacked"]', ...CARDS)} { box-shadow: 0 0 0 1.5px ${T.bg}; }
${pss('[data-state~="stacked"]:hover', ...CARDS)} { box-shadow: 0 0 0 1.5px ${T.bg}, ${T.raisedShadow}; }
${pss('[data-state~="stacked"][data-state~="selected"]', ...CARDS)} { box-shadow: 0 0 0 1.5px ${T.bg}, 0 0 0 3.5px ${T.accent}; }
${pss('[data-state~="free"]', ...CARDS)} { background-image: ${hatch(`color-mix(in srgb, ${T.fg} 12%, transparent)`)}; }
${pss('[data-state~="needs-action"]', ...CARDS)} {
  background-color: ${T.bg}; background-image: none;
  border-top: 1px dashed ${T.muted}; border-right: 1px dashed ${T.muted}; border-bottom: 1px dashed ${T.muted};
  border-left-style: dashed; border-left-width: ${ACCENT_BAR};
}
${CARDS.map((c) => `${HC} ${p(c)}[data-state~="needs-action"]`).join(", ")} { border-style: dashed; }
${pss('[data-state~="declined"]', ...CARDS)} { color: ${T.muted}; }
${CARDS.map((c) => `${p(c)}[data-state~="declined"] > ${p("event-card.title")}`).join(", ")} { text-decoration: line-through; }
${p("calendar.task")} {
  gap: ${T.xxs}; padding: 1px ${T.xs}; border: 1px solid ${T.border}; border-radius: ${T.rSm};
  font-size: ${T.small}; color: ${T.fg}; background-color: ${T.bg}; cursor: grab;
}
${p("calendar.task")}[data-state~="read-only"] { cursor: default; }
${p("calendar.task.check")} { margin: 0; accent-color: ${T.accent}; }
${p("calendar.new-event")} { gap: ${T.sm}; }
${ps("calendar.new-event.when", "calendar.new-event.calendar")} { font-size: ${T.small}; color: ${T.muted}; }
${p("calendar.new-event.actions")} { gap: ${T.xs}; }`;
