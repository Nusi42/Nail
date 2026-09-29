import type { KeyBinding } from "../api/generated";
import { defaultLayout } from "../api/mockData";
import { KeyMatcher, comboFromEvent, normalizeCombo } from "./keys";

const key = (
  k: string,
  mods: Partial<Record<"ctrlKey" | "altKey" | "shiftKey" | "metaKey", boolean>> = {},
) => ({
  key: k,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  metaKey: false,
  ...mods,
});

const bind = (context: string, k: string, action: string): KeyBinding => ({
  context,
  key: k,
  action,
  args: [],
});

describe("combos", () => {
  it("normalizes events", () => {
    expect(comboFromEvent(key("j"))).toBe("j");
    expect(comboFromEvent(key("ArrowDown"))).toBe("arrowdown");
    expect(comboFromEvent(key("b", { ctrlKey: true }))).toBe("ctrl+b");
    expect(comboFromEvent(key("M", { ctrlKey: true, shiftKey: true }))).toBe("ctrl+shift+m");
    expect(comboFromEvent(key("?", { shiftKey: true }))).toBe("?");
    expect(comboFromEvent(key("Shift", { shiftKey: true }))).toBeNull();
    expect(comboFromEvent(key("Control", { ctrlKey: true }))).toBeNull();
  });

  it("normalizes layout combos", () => {
    expect(normalizeCombo("Ctrl+Shift+M")).toBe("ctrl+shift+m");
    expect(normalizeCombo("shift+ctrl+m")).toBe("ctrl+shift+m");
    expect(normalizeCombo("Esc")).toBe("escape");
    expect(normalizeCombo("F5")).toBe("f5");
  });
});

describe("KeyMatcher", () => {
  it("matches bindings of the active context before global ones", () => {
    const m = new KeyMatcher([bind("global", "j", "global.j"), bind("list", "j", "mail.next")]);
    expect(m.feed("j", ["list", "global"])).toEqual({
      kind: "match",
      match: { action: "mail.next", args: [] },
    });
    expect(m.feed("j", ["reader", "global"])).toEqual({
      kind: "match",
      match: { action: "global.j", args: [] },
    });
    expect(m.feed("x", ["list", "global"])).toEqual({ kind: "none" });
  });

  it("lets later bindings win within a context", () => {
    const m = new KeyMatcher([bind("global", "e", "first"), bind("global", "e", "second")]);
    expect(m.feed("e", ["global"])).toMatchObject({ match: { action: "second" } });
  });

  it("matches sequences", () => {
    const m = new KeyMatcher(defaultLayout().keys);
    expect(m.feed("g", ["list", "global"])).toEqual({ kind: "pending" });
    expect(m.pending).toBe(true);
    expect(m.feed("r", ["list", "global"])).toEqual({
      kind: "match",
      match: { action: "view.toggle", args: ["reader"] },
    });
    expect(m.pending).toBe(false);
  });

  it("restarts when a sequence breaks", () => {
    const m = new KeyMatcher(defaultLayout().keys);
    m.feed("g", ["list", "global"]);
    expect(m.feed("j", ["list", "global"])).toMatchObject({ match: { action: "mail.next" } });
  });

  it("waits for a longer sequence and flushes the shorter match", () => {
    const m = new KeyMatcher([bind("global", "g", "short"), bind("global", "g g", "long")]);
    expect(m.feed("g", ["global"])).toEqual({ kind: "pending" });
    expect(m.flush()).toEqual({ action: "short", args: [] });
    m.feed("g", ["global"]);
    expect(m.feed("g", ["global"])).toMatchObject({ match: { action: "long" } });
  });

  it("covers the default key map", () => {
    const m = new KeyMatcher(defaultLayout().keys);
    expect(m.feed("arrowdown", ["list", "global"])).toMatchObject({
      match: { action: "mail.next" },
    });
    expect(m.feed("k", ["reader", "global"])).toMatchObject({ match: { action: "mail.previous" } });
    expect(m.feed("ctrl+b", ["folders", "global"])).toMatchObject({
      match: { action: "view.toggle", args: ["sidebar"] },
    });
    expect(m.feed("arrowdown", ["folders", "global"])).toEqual({ kind: "none" });
  });
});
