/**
 * P1 (WP191): the free/busy rows put the person and their calendars'
 * source in the name column and the track beside both; the day navigation
 * looks like buttons; the cancel-meeting confirmation keeps room for its
 * choices when it is made small.
 */
import { resolveWindowSpec } from "../../state/windows";
import { windows } from "./index";
import { CANCEL_DIALOG } from "./names";
import { SCHEDULING_BASE_CSS } from "./parts";

describe("scheduling parts (P1)", () => {
  it("places the source under the person and the track beside both", () => {
    expect(SCHEDULING_BASE_CSS).toContain(
      `[data-part="freebusy.row"] > [data-part="freebusy.source"] { grid-column: 1; grid-row: 2; min-width: 0; }`,
    );
    expect(SCHEDULING_BASE_CSS).toContain(
      `[data-part="freebusy.row"] > [data-part="freebusy.track"] { grid-column: 2; grid-row: 1 / span 2; min-width: 0; }`,
    );
  });

  it("styles the day navigation as buttons", () => {
    expect(SCHEDULING_BASE_CSS).toMatch(
      /\[data-part="freebusy.previous"\], \[data-part="freebusy.next"\] \{[^}]*min-height: var\(--nail-size-control\);[^}]*border: 1px solid var\(--nail-color-border\)/,
    );
  });

  it("gives the cancel-meeting confirmation a usable minimum size", () => {
    expect(windows[CANCEL_DIALOG]?.min_size).toEqual({ width: 400, height: 340 });
    const spec = resolveWindowSpec(CANCEL_DIALOG, null, windows);
    expect(spec.modal).toBe(true);
    expect(spec.resizable).toBe(true);
    expect(spec.min_size).toEqual({ width: 400, height: 340 });
  });
});
