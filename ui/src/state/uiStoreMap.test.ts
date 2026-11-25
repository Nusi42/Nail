/**
 * State kept per UI store (`UiStoreMap`, R8): one entry per app store, made
 * on first use (`obtain`) and shared by the store's views (a reader card's
 * scoped store); `obtain` on any map makes an entry once.
 */
import { describe, expect, it, vi } from "vitest";
import { UiStoreMap, createUiStore, obtain, registerUiStoreView } from "./uiStore";

describe("UiStoreMap", () => {
  it("makes an entry once per app store and shares it with the store's views", () => {
    const map = new UiStoreMap<{ n: number }>();
    const app = createUiStore();
    const view = createUiStore();
    registerUiStoreView(view, app);
    const create = vi.fn(() => ({ n: 1 }));
    const first = map.obtain(app, create);
    expect(map.obtain(view, create)).toBe(first);
    expect(map.obtain(app, create)).toBe(first);
    expect(create).toHaveBeenCalledTimes(1);
    const other = createUiStore();
    expect(map.obtain(other, create)).not.toBe(first);
    expect(create).toHaveBeenCalledTimes(2);
  });

  it("obtain keeps the first value of a key in any map", () => {
    const map = new Map<string, number[]>();
    obtain(map, "a", () => []).push(1);
    obtain(map, "a", () => []).push(2);
    expect(map.get("a")).toEqual([1, 2]);
  });
});
