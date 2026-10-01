/**
 * Structure of the scheduling parts every theme needs (like
 * `PIM_BASE_CSS`): the free/busy tracks are positioning layers (blocks and
 * the planned meeting are placed by geometry), lists without bullets.
 * Colours, borders, spacing and fonts come from the theme.
 *
 * The integrator appends this to the compiled theme's base CSS
 * (`ui/src/theme/compile.ts`, next to `PIM_BASE_CSS`).
 */

/** Lists that show no bullets and no indentation. */
const PLAIN_LISTS = [
  "invitation.issues",
  "invitation.attendee-list",
  "invitation.conflict-list",
  "freebusy.rows",
  "freebusy.suggestion-list",
  "scheduling.attendee-list",
  "scheduling.suggestions",
  "scheduling.meeting.people",
  "scheduling.cancel.people",
];

const sel = (parts: readonly string[]) => parts.map((p) => `[data-part="${p}"]`).join(", ");

export const SCHEDULING_BASE_CSS = `${sel(PLAIN_LISTS)} { list-style: none; margin: 0; padding: 0; }
[data-part="invitation.details"] { margin: 0; }
[data-part="invitation.detail.value"] { margin: 0; }
[data-part="freebusy.scale"], [data-part="freebusy.track"] { position: relative; min-height: 1.5em; }
[data-part="freebusy.hour"], [data-part="freebusy.busy"], [data-part="freebusy.meeting"] { position: absolute; top: 0; bottom: 0; box-sizing: border-box; overflow: hidden; white-space: nowrap; }
[data-part="freebusy.meeting"] { pointer-events: none; }
[data-part="freebusy.busy"][data-kind="tentative"] { z-index: 1; }
[data-part="freebusy.busy"][data-kind="busy"] { z-index: 2; }
[data-part="freebusy.busy"][data-kind="unavailable"] { z-index: 3; }
[data-part="freebusy.meeting"] { z-index: 4; }
[data-part="scheduling.scope"], [data-part="scheduling.meeting.recipients"], [data-part="scheduling.cancel.scopes"], [data-part="scheduling.settings.switches"], [data-part="scheduling.attendees"] { margin: 0; min-width: 0; }
`;
