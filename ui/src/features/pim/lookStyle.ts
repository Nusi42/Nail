/**
 * R7 style slot of the quick look and editor package (P2): the quick look popover (`pim.popover.*`), the compact event editor, the date / time input, recurrence and reminder pieces, the reminder toast.
 *
 * `LOOK_BASE_CSS` holds structure every theme needs (appended to
 * `PIM_BASE_CSS`, like `parts.ts`); `LOOK_SKIN_CSS` the look on tokens
 * only (appended to `PIM_SKIN_CSS`, like `skin.ts`). Only that package
 * edits this file during R7 (ADR 0246), so the packages never touch the
 * same stylesheet; the integrator may fold it into `parts.ts` / `skin.ts`.
 *
 * The package also restyles the invitation card and the free/busy lanes
 * of the scheduling feature (S10, S11). Parts that `defaults/theme.kdl`
 * already styles are only reached from the base CSS, through a parent
 * part (the theme's own single-part rules stay valid); the skin styles new
 * parts only, so no theme rule overrides it (`skin.test.ts`).
 */
import {
  ACCENT_BAR,
  FLOAT_INSET,
  floatingWidth,
  FOCUS_RING,
  VISUALLY_HIDDEN,
} from "../../theme/declarations";
import { p, ps } from "../../theme/selectors";
import { T } from "../../theme/tokens";

/** One line of text in the strip and the lanes (their rows must not collapse). */
const ROW = "1.6em";

/** Structure of the package's parts (one rule per line; selectors through `[data-part]` only). */
export const LOOK_BASE_CSS = [
  `${p("pim.look.hidden")} { ${VISUALLY_HIDDEN} }`,
  // The quick look: fixed beside its event (the shared Popover places it), at most the viewport.
  `${p("pim.popover")} { position: fixed; z-index: 1000; box-sizing: border-box; width: ${floatingWidth("26em")}; max-height: calc(100% - 2 * ${FLOAT_INSET}); overflow: auto; }`,
  `${p("pim.popover.body")} { display: flex; flex-direction: column; gap: ${T.sm}; min-width: 0; }`,
  `${p("pim.popover.header")} { display: flex; align-items: center; gap: ${T.sm}; min-width: 0; }`,
  `${p("pim.popover.title")} { margin: 0; min-width: 0; overflow-wrap: anywhere; }`,
  `${p("pim.popover.facts")} { display: flex; flex-direction: column; gap: ${T.xs}; margin: 0; }`,
  `${p("pim.popover.fact.label")} { ${VISUALLY_HIDDEN} }`,
  `${p("pim.popover.place")} { display: flex; align-items: center; gap: ${T.xs}; min-width: 0; }`,
  `${p("pim.popover.fact.value")} { display: flex; flex-direction: column; gap: 2px; margin: 0; min-width: 0; overflow-wrap: anywhere; }`,
  `${p("pim.popover.people")} { list-style: none; margin: 0; padding: 0; }`,
  `${p("pim.popover.person")} { display: flex; flex-wrap: wrap; gap: ${T.xs}; }`,
  `${p("pim.popover.actions")}, ${p("pim.popover.keys")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; }`,
  `${p("pim.popover.hint")} { display: inline-flex; align-items: center; gap: ${T.xxs}; }`,
  `${p("pim.popover.done")}:empty { display: none; }`,
  // The answer: the segments and the card's secondary actions on one row.
  `${p("scheduling.answer.row")}, ${p("scheduling.answer.options")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.sm}; }`,
  // The compact event editor.
  `${p("pim.compact.title")} { display: flex; min-width: 0; }`,
  `${p("pim.compact.title.input")} { flex: 1 1 auto; min-width: 0; box-sizing: border-box; }`,
  `${p("pim.compact.when")}, ${p("pim.compact.chips")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; min-width: 0; }`,
  `${p("pim.compact.when")} > ${p("pim.datetime")} { flex-direction: row; align-items: center; }`,
  `${p("pim.compact.chip")} { display: inline-flex; align-items: center; gap: ${T.xxs}; min-width: 0; max-width: 100%; box-sizing: border-box; }`,
  `${p("pim.compact.select")} { min-width: 0; max-width: 16em; }`,
  `${p("pim.compact.place")} { display: flex; align-items: center; gap: ${T.xs}; min-width: 0; }`,
  `${p("pim.compact.place.input")} { flex: 1 1 auto; min-width: 0; box-sizing: border-box; }`,
  `${p("pim.compact.more.body")} { display: flex; flex-direction: column; gap: ${T.sm}; padding-top: ${T.sm}; }`,
  `${p("pim.recurrence.details")} { display: flex; flex-direction: column; gap: ${T.xs}; min-width: 0; margin: 0; padding: 0; border: 0; }`,
  // The reminder toast: the calendar's colour as an edge, head and actions in rows.
  `${p("pim.toast.body")} { position: relative; display: flex; flex-direction: column; gap: ${T.xxs}; padding-inline-start: ${T.sm}; }`,
  `${p("pim.reminder.edge")} { position: absolute; top: 0; bottom: 0; left: 0; width: ${ACCENT_BAR}; }`,
  `${p("pim.reminder.head")} { display: flex; flex-wrap: wrap; align-items: baseline; justify-content: space-between; gap: ${T.sm}; }`,
  `${p("pim.reminder.actions")} { display: flex; flex-wrap: wrap; align-items: center; gap: ${T.xs}; margin-top: ${T.xs}; }`,
  `${p("pim.reminder.split")} { display: inline-flex; align-items: stretch; }`,
  // The invitation card's mini day strip: an hour axis over one lane of blocks.
  `${p("invitation.strip")} { display: flex; flex-direction: column; gap: 2px; min-width: 0; }`,
  `${p("invitation.strip.axis")}, ${p("invitation.strip.lane")} { position: relative; margin: 0; padding: 0; list-style: none; }`,
  `${p("invitation.strip.axis")} { min-height: ${ROW}; }`,
  `${p("invitation.strip.lane")} { min-height: calc(2 * ${ROW}); }`,
  `${p("invitation.strip.hour")}, ${p("invitation.strip.block")} { position: absolute; box-sizing: border-box; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }`,
  `${p("invitation.strip.hour")} { top: 0; bottom: 0; }`,
  `${p("invitation.strip.block")} { top: 0; height: 50%; }`,
  `${p("invitation.strip.block")}[data-state="invitation"] { top: 50%; }`,
  `${p("invitation.strip.time")} { ${VISUALLY_HIDDEN} }`,
  `${p("invitation.attendees.summary")} > ${p("invitation.attendees.line")} { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }`,
  // The free/busy lanes (S11): the axis is a row of its own, lanes taller, the frame movable.
  `${p("freebusy.lanes")} { display: flex; flex-direction: column; gap: ${T.xxs}; min-width: 0; }`,
  `${p("freebusy.row")} > ${p("freebusy.scale")} { grid-column: 2; margin-left: 0; min-width: 0; }`,
  `${p("freebusy.row")} > ${p("freebusy.track")} { min-height: 2.25em; }`,
  `${p("freebusy.row")} > ${p("freebusy.slot")} { grid-column: 2; position: relative; min-width: 0; min-height: ${ROW}; }`,
  `${p("freebusy.none")}, ${p("freebusy.frame")} { position: absolute; top: 0; bottom: 0; box-sizing: border-box; overflow: hidden; white-space: nowrap; }`,
  `${p("freebusy.frame")} { z-index: 5; touch-action: none; }`,
  `${p("freebusy.lanes")}[data-state="movable"] ${p("freebusy.meeting")} { pointer-events: auto; touch-action: none; }`,
  `${p("freebusy.busy.text")} { display: block; overflow: hidden; text-overflow: ellipsis; }`,
].join("\n");

/** Look of the package's parts (tokens only, `var(--nail-…)`; new parts only). */
export const LOOK_SKIN_CSS = `${p("pim.popover")} {
  padding: ${T.md}; border: 1px solid ${T.border}; border-radius: ${T.rLg};
  color: ${T.fg}; background-color: ${T.bg}; box-shadow: ${T.overlay};
  font-family: ${T.fontUi}; font-size: ${T.body}; line-height: ${T.normal};
}
${p("pim.popover.title")} { font-size: ${T.heading}; font-weight: ${T.semibold}; line-height: ${T.tight}; }
${p("pim.popover.dot")}, ${p("pim.compact.dot")} { flex: 0 0 auto; width: 0.75em; height: 0.75em; border-radius: 50%; }
${p("pim.popover.time")}, ${p("pim.popover.summary")}, ${p("pim.popover.person.status")}, ${p("pim.popover.loading")} { color: ${T.muted}; font-size: ${T.small}; }
${p("pim.popover.read-only")} { margin-inline-start: ${T.xs}; color: ${T.muted}; font-size: ${T.small}; }
${p("pim.popover.conflicts")} { margin: 0; color: ${T.warning}; font-size: ${T.small}; font-weight: ${T.medium}; }
${p("pim.popover.body")}[data-status="needs-action"] > ${p("pim.popover.header")} { padding-bottom: ${T.xs}; border-bottom: 1px dashed ${T.line}; }
${p("pim.popover.done")} { margin: 0; color: ${T.success}; font-size: ${T.small}; }
${p("pim.popover.keys")} { margin: 0; color: ${T.muted}; font-size: ${T.label}; }
${p("pim.popover.key")} { padding: 0 ${T.xxs}; border: 1px solid ${T.border}; border-radius: ${T.rSm}; font-family: ${T.fontMono}; background-color: ${T.subtle}; }
${p("pim.compact.title.input")} {
  padding: ${T.xs} 0; border: none; border-bottom: 1px solid ${T.line};
  font-family: ${T.fontUi}; font-size: ${T.title}; font-weight: ${T.semibold}; color: ${T.fg}; background-color: transparent;
}
${p("pim.compact.title.input")}:focus-visible { outline: none; border-bottom: 2px solid ${T.focus}; }
${p("pim.compact.arrow")}, ${p("pim.compact.length")} { color: ${T.muted}; font-size: ${T.small}; }
${p("pim.compact.chip")} {
  min-height: ${T.control}; padding: 0 ${T.sm}; border: 1px solid ${T.border}; border-radius: ${T.pill};
  font-family: ${T.fontUi}; font-size: ${T.small}; color: ${T.fg}; background-color: ${T.subtle};
}
${p("pim.compact.chip")}:focus-within, ${p("pim.compact.chip")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${p("pim.compact.chip")}[aria-expanded="true"] { color: ${T.accent}; background-color: ${T.accentSoft}; }
${ps("pim.compact.chip.icon", "pim.compact.place.icon", "pim.popover.place.icon")} { flex: 0 0 auto; width: ${T.icon}; height: ${T.icon}; color: ${T.muted}; }
${p("pim.compact.select")} { border: none; font: inherit; color: inherit; background-color: transparent; }
${p("pim.compact.select")}:focus-visible { outline: none; }
${p("pim.compact.place.input")} {
  min-height: ${T.control}; padding: 0 ${T.sm}; border: ${T.controlBorder}; border-radius: ${T.rMd};
  font: inherit; color: ${T.fg}; background-color: ${T.bg};
}
${p("pim.compact.place.input")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${p("pim.compact.more")} { border-top: 1px solid ${T.border}; padding-top: ${T.xs}; }
${p("pim.compact.more.summary")} { cursor: default; color: ${T.muted}; font-size: ${T.small}; }
${p("pim.compact.more.summary")}:focus-visible { ${FOCUS_RING} }
${p("pim.datetime.day")} {
  min-height: ${T.control}; padding: 0 ${T.sm}; border: ${T.controlBorder}; border-radius: ${T.rMd};
  font-family: ${T.fontUi}; font-size: ${T.body}; color: ${T.fg}; background-color: ${T.bg}; white-space: nowrap;
}
${p("pim.datetime.day")}:hover { background-color: ${T.subtle}; }
${p("pim.datetime.day")}:focus-visible { outline: 2px solid ${T.focus}; outline-offset: 1px; }
${p("pim.recurrence.details")} { padding: ${T.sm}; border-radius: ${T.rMd}; background-color: ${T.subtle}; }
${p("scheduling.people-summary")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${p("pim.reminder.when")} { color: ${T.warning}; font-weight: ${T.semibold}; font-size: ${T.small}; white-space: nowrap; }
${p("pim.reminder.meta")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${p("pim.reminder.edge")} { border-radius: ${T.rSm}; }
${p("pim.reminder.split")} > ${p("pim.reminder.more")} {
  display: inline-flex; align-items: center; justify-content: center; min-height: ${T.control}; padding: 0 ${T.xs};
  border: 1px solid ${T.border}; border-inline-start: none; border-radius: 0 ${T.rMd} ${T.rMd} 0; color: ${T.fg}; background-color: ${T.bg};
}
${p("pim.reminder.more")}:hover { background-color: ${T.subtle}; }
${p("pim.reminder.more")}:focus-visible { ${FOCUS_RING} }
${p("pim.reminder.more.icon")} { width: ${T.icon}; height: ${T.icon}; }
${p("invitation.more")} {
  display: inline-flex; align-items: center; gap: ${T.xxs}; min-height: ${T.control}; padding: 0 ${T.sm};
  border: none; border-radius: ${T.rMd}; font: inherit; color: ${T.fg}; background-color: transparent;
}
${p("invitation.more")}:hover { background-color: ${T.raised}; }
${p("invitation.more")}:focus-visible { ${FOCUS_RING} }
${p("invitation.more.icon")} { order: 1; width: ${T.icon}; height: ${T.icon}; }
${p("invitation.from")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${p("invitation.strip.axis")} { color: ${T.muted}; font-size: ${T.label}; }
${p("invitation.strip.hour")} { padding-inline-start: ${T.xxs}; border-inline-start: 1px solid ${T.border}; }
${p("invitation.strip.lane")} { border: 1px solid ${T.border}; border-radius: ${T.rSm}; background-color: ${T.bg}; }
${p("invitation.strip.block")} { padding: 0 ${T.xxs}; border-radius: ${T.rSm}; font-size: ${T.label}; line-height: ${ROW}; color: ${T.fg}; background-color: ${T.accentSoft}; }
${p("invitation.strip.block")}[data-state="tentative"] { border: 1px dashed ${T.accent}; background-color: transparent; }
${p("invitation.strip.block")}[data-state="invitation"] { border: 1px dashed ${T.accent}; background-color: ${T.bg}; font-weight: ${T.semibold}; }
${p("invitation.strip.all-day")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }
${p("invitation.attendees.line")} { color: ${T.muted}; font-size: ${T.small}; }
${p("segmented")}[data-field="answer"][data-state="open"] > ${p("segmented.option")}[data-action="accept"] { font-weight: ${T.semibold}; color: ${T.accentFg}; background-color: ${T.accent}; }
${p("scheduling.answer.row")} > ${p("form.button")}[data-action="propose-new-time"] { font-weight: ${T.regular}; color: ${T.muted}; }
${p("segmented")}[data-field="answer"] > ${p("segmented.option")}[data-action="accept"][aria-pressed="true"] { color: ${T.accentFg}; background-color: ${T.success}; }
${p("freebusy.frame")} { border: 2px solid ${T.accent}; border-radius: ${T.rSm}; background-color: color-mix(in srgb, ${T.accent} 12%, transparent); cursor: grab; }
${p("freebusy.frame")}[data-state="dragging"] { cursor: grabbing; }
${p("freebusy.frame")}:focus-visible { ${FOCUS_RING} }
${p("freebusy.none")} { color: ${T.muted}; font-size: ${T.label}; background-image: repeating-linear-gradient(135deg, ${T.subtle} 0 0.4em, transparent 0.4em 0.8em); }
${p("freebusy.busy.text")} { padding: 0 ${T.xxs}; font-size: ${T.label}; color: ${T.accentFg}; }
${p("freebusy.unknown")}, ${p("freebusy.hint")} { margin: 0; color: ${T.muted}; font-size: ${T.small}; }`;
