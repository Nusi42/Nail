import { act, renderHook } from "@testing-library/react";
import { NailError } from "../api/client";
import { useBusyAction } from "./busyAction";

describe("useBusyAction", () => {
  it("is busy while the write runs and clears the last problem", async () => {
    const { result } = renderHook(() => useBusyAction());
    act(() => {
      result.current.setProblem({ id: "old-problem" });
    });
    let finish: () => void = () => undefined;
    let done: Promise<boolean> = Promise.resolve(false);
    act(() => {
      done = result.current.run(
        () =>
          new Promise<void>((resolve) => {
            finish = resolve;
          }),
      );
    });
    expect(result.current.busy).toBe(true);
    expect(result.current.problem).toBeNull();
    await act(async () => {
      finish();
      expect(await done).toBe(true);
    });
    expect(result.current.busy).toBe(false);
    expect(result.current.problem).toBeNull();
  });

  it("keeps the core's localized refusal as the problem", async () => {
    const { result } = renderHook(() => useBusyAction());
    await act(async () => {
      const ok = await result.current.run(() =>
        Promise.reject(
          new NailError("conflict", "no", { id: "exchange-category-issue-duplicate" }),
        ),
      );
      expect(ok).toBe(false);
    });
    expect(result.current.busy).toBe(false);
    expect(result.current.problem).toEqual({ id: "exchange-category-issue-duplicate" });
  });

  it("falls back to the error code's text", async () => {
    const { result } = renderHook(() => useBusyAction());
    await act(async () => {
      await result.current.run(() => Promise.reject(new NailError("network", "down")));
    });
    expect(result.current.problem).toEqual({ id: "error-network" });
  });
});
