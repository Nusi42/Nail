import { describe, expect, it } from "vitest";
import { exactRowSize } from "./MessageList";

/** A row element whose layout box is `height` px tall (jsdom has no layout). */
function rowOf(height: number): HTMLElement {
  const row = document.createElement("div");
  row.getBoundingClientRect = () => DOMRect.fromRect({ x: 0, y: 0, width: 300, height });
  return row;
}

function entryOf(blockSize: number): ResizeObserverEntry {
  const box: ResizeObserverSize = { blockSize, inlineSize: 300 };
  return { borderBoxSize: [box] } as unknown as ResizeObserverEntry;
}

describe("exactRowSize (Q1 leftover: deterministic list scrolling)", () => {
  it("keeps the fraction of the observed border box", () => {
    expect(exactRowSize(rowOf(0), entryOf(111.22))).toBe(111.22);
  });

  it("measures the layout box without an observer entry, unrounded", () => {
    expect(exactRowSize(rowOf(109.05), undefined)).toBe(109.05);
  });

  it("falls back to the layout box when the entry has no border box", () => {
    const empty = { borderBoxSize: [] } as unknown as ResizeObserverEntry;
    expect(exactRowSize(rowOf(98.5), empty)).toBe(98.5);
  });
});
