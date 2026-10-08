import { act, renderHook } from "@testing-library/react";
import { useDebounced } from "./debounce";

describe("useDebounced", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it("settles on the last value once it stopped changing", () => {
    const { result, rerender } = renderHook(({ v }) => useDebounced(v, 100), {
      initialProps: { v: "a" },
    });
    rerender({ v: "ab" });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    rerender({ v: "abc" });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    expect(result.current).toBe("a");
    act(() => {
      vi.advanceTimersByTime(40);
    });
    expect(result.current).toBe("abc");
  });

  it("keeps the settled value for an equal one", () => {
    const same = (a: readonly string[], b: readonly string[]) => a.join() === b.join();
    const first = ["x"];
    const { result, rerender } = renderHook(({ v }) => useDebounced(v, 100, same), {
      initialProps: { v: first },
    });
    rerender({ v: ["x"] });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(result.current).toBe(first);
  });
});
