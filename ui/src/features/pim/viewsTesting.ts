/**
 * Test support of the calendar views (only tests import this file): a
 * `ResizeObserver` whose callbacks a test runs after changing the sizes it
 * reports through `clientWidth`.
 */
import { act } from "@testing-library/react";
import { vi } from "vitest";

export interface ResizeStub {
  /** Runs every observer's callback (inside `act`). */
  notify: () => void;
  /** Puts `ResizeObserver` and `clientWidth` back. */
  restore: () => void;
}

/**
 * Stubs `ResizeObserver` and `HTMLElement.clientWidth` (`width(el)`, read
 * on every access) until `restore`.
 */
export function stubResize(width: (el: HTMLElement) => number): ResizeStub {
  const callbacks = new Set<() => void>();
  vi.stubGlobal(
    "ResizeObserver",
    class {
      constructor(private readonly cb: () => void) {}
      observe() {
        callbacks.add(this.cb);
      }
      disconnect() {
        callbacks.delete(this.cb);
      }
      unobserve = vi.fn();
    },
  );
  const client = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return width(this);
    },
  });
  return {
    notify: () => {
      act(() => {
        callbacks.forEach((cb) => {
          cb();
        });
      });
    },
    restore: () => {
      if (client) Object.defineProperty(HTMLElement.prototype, "clientWidth", client);
      vi.unstubAllGlobals();
    },
  };
}
