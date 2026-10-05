import type { Breakpoint } from "../api/generated";
import { defaultLayout } from "../api/mockData";
import { normalizeNode } from "../api/normalize";
import {
  defaultStackSize,
  flexForSize,
  parseSize,
  renderedStackTracks,
  resizeSplit,
  selectBreakpoint,
  selectBreakpointIndex,
  trackForSize,
} from "./engine";

const root = normalizeNode({ type: "widget", kind: "text" });
const bp = (min: number | null, max: number | null): Breakpoint => ({
  min_width: min,
  max_width: max,
  root,
});

describe("breakpoint selection", () => {
  // M5b: the three panes are the mail view's window.
  const main = defaultLayout().windows.mail ?? [];

  it("picks the wide three-pane tree from 900px", () => {
    expect(selectBreakpointIndex(main, 900)).toBe(0);
    expect(selectBreakpointIndex(main, 1920)).toBe(0);
    expect(selectBreakpoint(main, 1200)?.root).toMatchObject({ type: "split" });
  });

  it("picks the tabs tree up to 899px (inclusive bounds)", () => {
    expect(selectBreakpointIndex(main, 899)).toBe(1);
    expect(selectBreakpointIndex(main, 320)).toBe(1);
  });

  it("uses the first match and falls back to the first breakpoint", () => {
    expect(selectBreakpointIndex([bp(null, null), bp(0, 100)], 50)).toBe(0);
    expect(selectBreakpointIndex([bp(100, 200), bp(300, null)], 250)).toBe(0);
    expect(selectBreakpoint([], 500)).toBeUndefined();
  });
});

describe("sizes", () => {
  it("parses CSS track sizes", () => {
    expect(parseSize("240px")).toEqual({ value: 240, unit: "px" });
    expect(parseSize("1.4fr")).toEqual({ value: 1.4, unit: "fr" });
    expect(parseSize("calc(1px)")).toBeNull();
    expect(parseSize(null)).toBeNull();
  });

  it("maps pane sizes to flex", () => {
    expect(flexForSize("240px")).toBe("0 1 240px");
    expect(flexForSize("30%")).toBe("0 1 30%");
    expect(flexForSize("1.4fr")).toBe("1.4 1 0px");
    expect(flexForSize(undefined)).toBe("1 1 auto");
    expect(flexForSize("auto")).toBe("0 0 auto");
    expect(flexForSize("bogus;")).toBe("1 1 0px");
  });

  it("maps stack sizes to grid tracks", () => {
    expect(trackForSize("1fr")).toBe("minmax(0, 1fr)");
    expect(trackForSize("auto")).toBe("auto");
    expect(trackForSize("48px")).toBe("48px");
    expect(trackForSize("}")).toBe("auto");
  });

  it("gives tracks only to stack children that rendered something", () => {
    const tracks = ["auto", "auto", "auto", "minmax(0, 1fr)"];
    // Child 0 and 3 rendered, 1 and 2 (banners without a reason) did not.
    expect(renderedStackTracks(tracks, [0, null, 1, 2, 3, null])).toEqual([
      "auto",
      "minmax(0, 1fr)",
    ]);
    // A child rendering two elements: the second one gets `auto`.
    expect(renderedStackTracks(["48px", "1fr"], [0, null, null, 1, null])).toEqual([
      "48px",
      "auto",
      "1fr",
    ]);
    // Nothing rendered, and elements before any marker.
    expect(renderedStackTracks(tracks, [0, 1, 2, 3])).toEqual([]);
    expect(renderedStackTracks(tracks, [null])).toEqual(["auto"]);
  });

  it("defaults bar widgets to auto height", () => {
    expect(defaultStackSize(normalizeNode({ type: "widget", kind: "toolbar" }))).toBe("auto");
    expect(defaultStackSize(normalizeNode({ type: "widget", kind: "message-list" }))).toBe("1fr");
    expect(defaultStackSize(normalizeNode({ type: "stack" }))).toBe("1fr");
  });
});

describe("resizeSplit", () => {
  const base = {
    sizes: ["240px", "1fr", "1.4fr"],
    measured: [240, 400, 560],
    minSizes: ["160px", "280px", "320px"],
    maxSizes: ["480px", null, null],
  };

  it("grows a fixed pane in pixels", () => {
    expect(resizeSplit({ ...base, index: 0, delta: 40 })).toEqual(["280px", "1fr", "1.4fr"]);
  });

  it("clamps to the pane's min and max", () => {
    expect(resizeSplit({ ...base, index: 0, delta: -500 })).toEqual(["160px", "1fr", "1.4fr"]);
    expect(resizeSplit({ ...base, index: 0, delta: 1000 })[0]).toBe("360px"); // neighbour min 280px
  });

  it("redistributes fr weights between two fluid panes", () => {
    const [, a, b] = resizeSplit({ ...base, index: 1, delta: 80 });
    expect(a).toBe("1.2fr"); // 2.4 * (400 + 80) / 960
    expect(b).toBe("1.2fr");
  });

  it("changes the fixed neighbour when the leading pane is fluid", () => {
    expect(
      resizeSplit({ sizes: ["1fr", "300px"], measured: [700, 300], index: 0, delta: 50 }),
    ).toEqual(["1fr", "250px"]);
  });

  it("works without measurements for pixel panes (keyboard in jsdom)", () => {
    expect(resizeSplit({ sizes: ["240px", "1fr"], measured: [0, 0], index: 0, delta: 16 })).toEqual(
      ["256px", "1fr"],
    );
  });

  it("ignores invalid divider indexes", () => {
    expect(resizeSplit({ ...base, index: 2, delta: 10 })).toEqual(base.sizes);
  });
});
