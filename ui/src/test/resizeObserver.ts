/**
 * A `ResizeObserver` stand-in whose callbacks a test calls: `installFakeResizeObserver()`
 * in `beforeEach` (with `vi.unstubAllGlobals()` after), `resized(el)` when the size of
 * `el` changed.
 */
import { vi } from "vitest";

interface Entry {
  callback: () => void;
  targets: Element[];
  live: boolean;
}

const observers: Entry[] = [];

class FakeResizeObserver {
  private readonly entry: Entry;
  constructor(callback: () => void) {
    this.entry = { callback, targets: [], live: true };
    observers.push(this.entry);
  }
  observe(target: Element) {
    this.entry.targets.push(target);
  }
  unobserve(target: Element) {
    this.entry.targets = this.entry.targets.filter((t) => t !== target);
  }
  disconnect() {
    this.entry.live = false;
  }
}

/** Replaces `ResizeObserver` with the stand-in and forgets earlier observers. */
export function installFakeResizeObserver(): void {
  observers.length = 0;
  vi.stubGlobal("ResizeObserver", FakeResizeObserver);
}

/** Calls every live observer of `el` (its size changed). */
export function resized(el: Element): void {
  for (const o of observers) if (o.live && o.targets.includes(el)) o.callback();
}
