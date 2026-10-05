import { describe, expect, it } from "vitest";
import {
  childKey,
  collapseMove,
  expandMove,
  messageOrderOf,
  messageThreadsOf,
  selectedIndexOf,
  siblingPosition,
  stepIndex,
  toggled,
  visibleRows,
  type RowRef,
} from "./messageListModel";

const conversation = (key: string, latest: string, expandable = true): RowRef => ({
  key,
  kind: "conversation",
  messageId: latest,
  threadId: key,
  parentKey: null,
  expandable,
});
const child = (parent: string, id: string): RowRef => ({
  key: childKey(parent, id),
  kind: "child",
  messageId: id,
  threadId: null,
  parentKey: parent,
  expandable: false,
});

/** Two conversations: t1 (m1, m2, m3; m3 newest) and t2 (one message m9). */
const BASE = [conversation("t1", "m3"), conversation("t2", "m9", false)];
const CHILDREN: Record<string, RowRef[]> = {
  t1: [child("t1", "m1"), child("t1", "m2"), child("t1", "m3")],
  t2: [child("t2", "m9")],
};
const open = (keys: string[]) => visibleRows(BASE, new Set(keys), (row) => CHILDREN[row.key]);

describe("visibleRows (Design B)", () => {
  it("adds an expanded conversation's children below it, oldest first", () => {
    expect(open([]).map((r) => r.key)).toEqual(["t1", "t2"]);
    expect(open(["t1"]).map((r) => r.key)).toEqual(["t1", "t1/m1", "t1/m2", "t1/m3", "t2"]);
  });

  it("never opens a conversation of one message, and waits for children to load", () => {
    expect(open(["t2"]).map((r) => r.key)).toEqual(["t1", "t2"]);
    expect(visibleRows(BASE, new Set(["t1"]), () => undefined)).toHaveLength(2);
  });
});

describe("messageOrderOf", () => {
  it("keeps each message once, in row order", () => {
    // t1 selects m3, which its last child shows too.
    expect(messageOrderOf(open(["t1"]))).toEqual(["m3", "m1", "m2", "m9"]);
  });
});

describe("messageThreadsOf", () => {
  it("maps each message to the conversation of its first row", () => {
    expect(messageThreadsOf(open(["t1"]))).toEqual({ m3: "t1", m9: "t2" });
  });
});

describe("selectedIndexOf", () => {
  const rows = open(["t1"]);

  it("keeps the chosen row while it shows the selected message", () => {
    expect(selectedIndexOf(rows, "t1/m3", "m3")).toBe(3);
    expect(selectedIndexOf(rows, "t1", "m3")).toBe(0);
  });

  it("falls back to the first row of the message when the selection moved elsewhere", () => {
    expect(selectedIndexOf(rows, "t1/m3", "m2")).toBe(2);
    expect(selectedIndexOf(rows, null, "m3")).toBe(0);
    expect(selectedIndexOf(rows, "gone", "m9")).toBe(4);
  });

  it("falls back to the selected conversation's row for a message without a row", () => {
    expect(selectedIndexOf(rows, null, "sent-reply", "t1")).toBe(0);
    expect(selectedIndexOf(rows, null, "sent-reply", "other")).toBe(-1);
    // A message with a row of its own wins over the conversation.
    expect(selectedIndexOf(rows, null, "m9", "t1")).toBe(4);
  });

  it("is -1 without a selection or when no row shows it", () => {
    expect(selectedIndexOf(rows, "t1", null)).toBe(-1);
    expect(selectedIndexOf(rows, null, "elsewhere")).toBe(-1);
  });
});

describe("stepIndex (j / k)", () => {
  it("moves one row and stops at the ends", () => {
    expect(stepIndex(5, 0, 1)).toBe(1);
    expect(stepIndex(5, 4, 1)).toBe(4);
    expect(stepIndex(5, 0, -1)).toBe(0);
  });

  it("starts at the first (j) or last (k) row without a selection", () => {
    expect(stepIndex(5, -1, 1)).toBe(0);
    expect(stepIndex(5, -1, -1)).toBe(4);
    expect(stepIndex(0, -1, 1)).toBe(-1);
  });
});

describe("→ / ← (tree keys)", () => {
  it("→ opens a closed conversation, then moves to its first child", () => {
    expect(expandMove(open([]), 0, new Set())).toEqual({ kind: "expand", key: "t1" });
    expect(expandMove(open(["t1"]), 0, new Set(["t1"]))).toEqual({ kind: "select", index: 1 });
  });

  it("→ does nothing on a child, a single message or before the children load", () => {
    expect(expandMove(open(["t1"]), 2, new Set(["t1"]))).toEqual({ kind: "none" });
    expect(expandMove(open([]), 1, new Set())).toEqual({ kind: "none" });
    expect(expandMove(open([]), 0, new Set(["t1"]))).toEqual({ kind: "none" });
    expect(expandMove(open([]), -1, new Set())).toEqual({ kind: "none" });
  });

  it("← moves from a child to its conversation, then closes it", () => {
    const rows = open(["t1"]);
    expect(collapseMove(rows, 3, new Set(["t1"]))).toEqual({ kind: "select", index: 0 });
    expect(collapseMove(rows, 0, new Set(["t1"]))).toEqual({ kind: "collapse", key: "t1" });
  });

  it("← does nothing on a closed conversation or without a selection", () => {
    expect(collapseMove(open([]), 0, new Set())).toEqual({ kind: "none" });
    expect(collapseMove(open([]), -1, new Set())).toEqual({ kind: "none" });
  });
});

describe("toggled", () => {
  it("adds and removes keys, keeping the set when nothing changes", () => {
    const keys = new Set(["a"]);
    expect(toggled(keys, "a", true)).toBe(keys);
    expect([...toggled(keys, "b", true)]).toEqual(["a", "b"]);
    expect([...toggled(keys, "a", false)]).toEqual([]);
    expect(keys.has("a")).toBe(true);
  });
});

describe("siblingPosition", () => {
  it("numbers the children of one conversation", () => {
    const rows = open(["t1"]);
    expect(siblingPosition(rows, 1)).toEqual({ posinset: 1, setsize: 3 });
    expect(siblingPosition(rows, 3)).toEqual({ posinset: 3, setsize: 3 });
    expect(siblingPosition(rows, 0)).toBeNull();
  });
});
