import { act, renderHook } from "@testing-library/react";
import {
  LIVE_SEARCH_PAUSE_MS,
  TYPING_PAUSE_MS,
  useDebounced,
  useDebouncedAction,
} from "./debounce";

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

describe("useDebouncedAction", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  const render = (action: (v: string) => void) =>
    renderHook(({ v, on }) => useDebouncedAction(v, 100, action, on), {
      initialProps: { v: "", on: true },
    });

  it("acts on the last value after the pause, never on the first one", () => {
    const action = vi.fn<(v: string) => void>();
    const { rerender } = render(action);
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(action).not.toHaveBeenCalled();
    rerender({ v: "a", on: true });
    act(() => {
      vi.advanceTimersByTime(60);
    });
    rerender({ v: "ab", on: true });
    act(() => {
      vi.advanceTimersByTime(99);
    });
    expect(action).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(action.mock.calls).toEqual([["ab"]]);
  });

  it("acts at once on flush and drops the pending call", () => {
    const action = vi.fn<(v: string) => void>();
    const { result, rerender } = render(action);
    rerender({ v: "from:ada", on: true });
    act(() => {
      result.current();
    });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(action.mock.calls).toEqual([["from:ada"]]);
  });

  it("drops the pending call while disabled", () => {
    const action = vi.fn<(v: string) => void>();
    const { rerender } = render(action);
    rerender({ v: "x", on: true });
    rerender({ v: "x", on: false });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(action).not.toHaveBeenCalled();
  });

  it("waits longer before a live search than before a suggestion", () => {
    expect(LIVE_SEARCH_PAUSE_MS).toBeGreaterThan(TYPING_PAUSE_MS);
  });
});
