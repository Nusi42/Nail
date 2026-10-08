/**
 * The UI's window model (M5c, ADR 0116): the ports of `nail_api::windows`
 * over the vectors generated from the Rust rules (`windows.json`), and the
 * UI-only helpers.
 */
import type { WindowGeometry, WindowState } from "../api/generated";
import windowsVectors from "../../../crates/nail-api/fixtures/ts-vectors/windows.json";
import { describeVectors } from "../test/vectors";
import {
  CONFIRMATION_WINDOW_SPEC,
  DEFAULT_WINDOW_SPEC,
  MAX_WINDOW_COORDINATE,
  MAX_WINDOW_EDGE,
  MAX_WINDOW_KIND_CHARS,
  MAX_WINDOW_STATES,
  MIN_WINDOW_EDGE,
  checkWindowStateWrite,
  isValidGeometry,
  isWindowInstance,
  isWindowKind,
  newWindowInstance,
  normalizeSpec,
  parsePopOutRoute,
  parseWindowId,
  popOutLabel,
  popOutRoute,
  resolveWindowSpec,
  windowId,
  windowKind,
  withFrame,
} from "./windows";

const geometry = (x: number, y: number, width: number, height: number): WindowGeometry => ({
  rect: { x, y, width, height },
  maximized: false,
  snap: null,
});

const state = (page: WindowGeometry | null): WindowState => ({
  page,
  pop_out: null,
  popped_out: false,
});

describe("window model (M5c)", () => {
  it("parses window ids like nail_api::windows", () => {
    for (const good of ["composer", "pim-event-editor", "a1", "x"]) {
      expect(isWindowKind(good)).toBe(true);
    }
    for (const bad of ["", "Composer", "1x", "a-", "a--b", "a b", "a.b", "a:b"]) {
      expect(isWindowKind(bad)).toBe(false);
    }
    expect(isWindowKind("a".repeat(MAX_WINDOW_KIND_CHARS + 1))).toBe(false);
    expect(parseWindowId("composer:d42")).toEqual({ kind: "composer", instance: "d42" });
    expect(parseWindowId("settings")).toEqual({ kind: "settings", instance: null });
    expect(parseWindowId("composer:Draft_1-x")?.instance).toBe("Draft_1-x");
    for (const bad of ["", "composer:", ":x", "composer:a:b", "composer:a b", "Composer:x"]) {
      expect(parseWindowId(bad)).toBeNull();
    }
    expect(windowId("composer", "d1")).toBe("composer:d1");
    expect(windowId("settings")).toBe("settings");
    expect(windowKind("composer:d1")).toBe("composer");
    expect(windowKind("settings")).toBe("settings");
    expect(newWindowInstance("tool", [])).toBe("tool:w1");
    expect(newWindowInstance("tool", ["tool:w1", "tool:w3", "other:w2"])).toBe("tool:w2");
  });

  it("builds pop-out routes and labels", () => {
    expect(popOutRoute("composer:d1")).toBe("#/window/composer:d1");
    expect(parsePopOutRoute("#/window/composer:d1")).toBe("composer:d1");
    expect(parsePopOutRoute("#/window/Bad")).toBeNull();
    expect(parsePopOutRoute("#/print/x")).toBeNull();
    expect(parsePopOutRoute("")).toBeNull();
    expect(popOutLabel("settings")).toBe("popout-settings");
  });

  it("resolves specs: defaults, then registrations, then the layout", () => {
    expect(DEFAULT_WINDOW_SPEC.modal).toBe(false);
    expect(CONFIRMATION_WINDOW_SPEC.modal).toBe(true);
    expect(normalizeSpec(DEFAULT_WINDOW_SPEC)).toEqual(DEFAULT_WINDOW_SPEC);
    expect(normalizeSpec(CONFIRMATION_WINDOW_SPEC)).toEqual(CONFIRMATION_WINDOW_SPEC);
    const frame = {
      modal: null,
      resizable: null,
      min_size: null,
      size: { width: 760, height: 600 },
      pop_out: null,
      instances: "multiple" as const,
    };
    const spec = withFrame(DEFAULT_WINDOW_SPEC, frame);
    expect(spec.size).toEqual({ width: 760, height: 600 });
    expect(spec.instances).toBe("multiple");
    expect(spec.min_size).toEqual(DEFAULT_WINDOW_SPEC.min_size);
    // The default size never ends up below the minimum.
    expect(
      withFrame(DEFAULT_WINDOW_SPEC, {
        ...frame,
        size: null,
        min_size: { width: 800, height: 700 },
      }).size,
    ).toEqual({ width: 800, height: 700 });
    const tiny = normalizeSpec({
      ...DEFAULT_WINDOW_SPEC,
      min_size: { width: 1, height: 1 },
      size: { width: 1, height: 99_999 },
    });
    expect(tiny.min_size).toEqual({ width: MIN_WINDOW_EDGE, height: MIN_WINDOW_EDGE });
    expect(tiny.size).toEqual({ width: MIN_WINDOW_EDGE, height: MAX_WINDOW_EDGE });

    const registry = {
      "gmail-label-delete": { confirmation: true },
      "pim-event-editor": { size: { width: 640, height: 560 } },
      composer: { modal: true },
    };
    const layout = { window_frames: { composer: { ...frame, modal: false } } };
    expect(resolveWindowSpec("gmail-label-delete", layout, registry)).toEqual(
      CONFIRMATION_WINDOW_SPEC,
    );
    expect(resolveWindowSpec("pim-event-editor", null, registry)).toEqual({
      ...DEFAULT_WINDOW_SPEC,
      size: { width: 640, height: 560 },
    });
    // The layout wins over the registration; instance ids resolve by kind.
    expect(resolveWindowSpec("composer:d1", layout, registry)).toEqual({
      ...DEFAULT_WINDOW_SPEC,
      size: { width: 760, height: 600 },
      instances: "multiple",
    });
    expect(resolveWindowSpec("unknown", layout, registry)).toEqual(DEFAULT_WINDOW_SPEC);
  });

  it("checks geometry and writes like the core", () => {
    const g = geometry(10, -20, 700, 500);
    expect(isValidGeometry(g)).toBe(true);
    expect(isValidGeometry({ ...g, maximized: true })).toBe(true);
    expect(isValidGeometry({ ...g, maximized: true, snap: "left" })).toBe(false);
    expect(isValidGeometry(geometry(0, 0, 0, 10))).toBe(false);
    expect(isValidGeometry(geometry(0, 0, 10, MAX_WINDOW_EDGE + 1))).toBe(false);
    expect(isValidGeometry(geometry(MAX_WINDOW_COORDINATE + 1, 0, 10, 10))).toBe(false);
    expect(isValidGeometry(geometry(-MAX_WINDOW_COORDINATE, 0, 10, 10))).toBe(true);
    expect(isValidGeometry(geometry(0.5, 0, 10, 10))).toBe(false);

    const ok = state(geometry(0, 0, 600, 400));
    expect(checkWindowStateWrite({}, "composer", ok)).toBeNull();
    expect(checkWindowStateWrite({}, "composer:d1", ok)).toBe("invalid-window");
    expect(checkWindowStateWrite({}, "Bad", null)).toBe("invalid-window");
    expect(checkWindowStateWrite({}, "composer", { ...ok, pop_out: geometry(0, 0, 0, 400) })).toBe(
      "invalid-geometry",
    );
    const full = Object.fromEntries(
      Array.from({ length: MAX_WINDOW_STATES }, (_v, i) => [`w${String(i)}`, ok]),
    );
    expect(checkWindowStateWrite(full, "new-window", ok)).toBe("too-many-windows");
    expect(checkWindowStateWrite(full, "w0", ok)).toBeNull();
    expect(checkWindowStateWrite(full, "new-window", null)).toBeNull();
    expect(checkWindowStateWrite(full, "new-window", state(null))).toBeNull();
  });
});

describeVectors("window model", windowsVectors, {
  isWindowKind,
  isWindowInstance,
  parseWindowId,
  windowKind,
  windowId,
  popOutRoute,
  popOutLabel,
  parsePopOutRoute,
  normalizeSpec,
  withFrame,
  isValidGeometry,
  checkWindowStateWrite: (kinds: string[], window: string, write: WindowState | null) =>
    checkWindowStateWrite(Object.fromEntries(kinds.map((k) => [k, state(null)])), window, write),
});
