/**
 * Structure of the PIM parts that every theme needs (like
 * `PLUGIN_BASE_CSS`): the time grid's layers (slots behind, events
 * positioned over them), wrappers that must not take part in grid layout,
 * lists without bullets, popups and toasts. Colours, borders, spacing and
 * fonts come from the theme (`defaults/theme.kdl` for the default).
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `PLUGIN_BASE_CSS`).
 */

/** Lists that show no bullets and no indentation. */
const PLAIN_LISTS = [
  "calendar.column",
  "calendar.all-day.events",
  "calendar.month.events",
  "calendar.agenda.list",
  "agenda.list.list",
  "contact-list.items",
  "contact-card.values",
  "task-list.items",
  "calendar-list.items",
  "pim.editor.attendee-list",
  "pim.editor.suggestions",
  "pim.editor.subtask-list",
  "pim.groups.items",
  "pim.merge.groups",
  "pim.import.sample",
  "pim.import.issues",
  "pim.sources.list",
  "pim.sources.test.collections",
  "pim.toasts.list",
];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const PIM_BASE_CSS = `${sel(PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
[data-part="calendar"] { display: flex; flex-direction: column; min-height: 0; min-width: 0; container: nail-calendar / inline-size; }
[data-part="calendar"] > * { flex: 0 0 auto; }
[data-part="calendar"] > :last-child:not([data-part="calendar.toolbar"]) { flex: 1 1 0; min-height: 0; min-width: 0; }
[data-part="calendar.year"], [data-part="calendar.agenda"], [data-part="calendar.free-time"], [data-part="calendar.month.frame"] { overflow-y: auto; }
[data-part="calendar.time-grid"] { display: grid; grid-template-rows: auto auto minmax(0, 1fr); min-height: 0; }
[data-part="calendar.head"], [data-part="calendar.all-day"], [data-part="calendar.body"] { display: grid; grid-template-columns: 56px minmax(0, 1fr); }
[data-part="calendar.head"][data-zones="2"], [data-part="calendar.all-day"][data-zones="2"] { grid-template-columns: 112px minmax(0, 1fr); }
[data-part="calendar.body"][data-zones="2"] { grid-template-columns: 56px 56px minmax(0, 1fr); }
[data-part="calendar.head.days"], [data-part="calendar.all-day.days"], [data-part="calendar.slots"], [data-part="calendar.columns"] { display: grid; }
[data-part="calendar.all-day.days"] { grid-auto-rows: minmax(1.5em, auto); }
[data-part="calendar.all-day.events"], [data-part="calendar.all-day.item"], [data-part="calendar.slot-row"], [data-part="calendar.column.item"] { display: contents; }
[data-part="calendar.scroll"] { overflow-y: auto; min-height: 0; }
[data-part="calendar.gutter"] { display: flex; flex-direction: column; }
[data-part="calendar.hour"], [data-part="calendar.slot"] { box-sizing: border-box; height: 48px; }
[data-part="calendar.canvas"] { position: relative; }
[data-part="calendar.columns"] { position: absolute; inset: 0; pointer-events: none; }
[data-part="calendar.column"] { position: relative; }
[data-part="calendar.event"] { position: absolute; box-sizing: border-box; overflow: hidden; pointer-events: auto; display: flex; flex-direction: column; text-align: start; font: inherit; }
[data-part="calendar.event.resize"] { position: absolute; inset-inline: 0; bottom: 0; height: 6px; cursor: ns-resize; }
[data-part="calendar.ghost"], [data-part="calendar.now"] { position: absolute; pointer-events: none; box-sizing: border-box; }
[data-part="calendar.chip"], [data-part="calendar.agenda.event"], [data-part="agenda.list.event"] { display: flex; min-width: 0; overflow: hidden; text-align: start; font: inherit; }
[data-part="calendar.month"] { width: 100%; table-layout: fixed; border-collapse: collapse; }
[data-part="calendar.month.day"] { vertical-align: top; }
[data-part="calendar.free-time.track"] { position: relative; }
[data-part="calendar.free-time.busy"], [data-part="calendar.free-time.slot"] { position: absolute; top: 0; bottom: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
[data-part="pim.datetime"] { position: relative; }
[data-part="pim.datetime.controls"], [data-part="pim.datetime.input"] { display: inline-flex; flex-wrap: wrap; align-items: center; }
[data-part="pim.datetime.popup"] { position: absolute; top: 100%; z-index: 20; }
[data-part="pim.swatch"] { display: inline-block; }
[data-part="pim.toasts"] { position: fixed; left: 16px; bottom: 16px; z-index: 300; max-width: min(420px, calc(100% - 32px)); }
[data-part="contact-card.photo"], [data-part="pim.editor.photo"] { object-fit: cover; }
[data-part="contact-card.address"] { display: flex; flex-direction: column; font-style: normal; }
[data-part="pim.editor.main"], [data-part="pim.recurrence.weekdays"], [data-part="pim.scope.choices"] { margin: 0; min-width: 0; }
`;
