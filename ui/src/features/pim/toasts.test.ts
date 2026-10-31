/**
 * ADR 0207: a window hidden for days collects reminder and problem toasts;
 * the list stays bounded (the newest are kept).
 */
import { describe, expect, it } from "vitest";
import { createUiStore } from "../../state/uiStore";
import { MAX_PIM_TOASTS, pimStoreFor } from "./store";

describe("PIM toasts", () => {
  it("keep at most MAX_PIM_TOASTS over three days of failed changes", () => {
    const pim = pimStoreFor(createUiStore());
    // One failed change every five minutes for three days.
    const count = (3 * 24 * 60) / 5;
    for (let n = 0; n < count; n++) {
      pim.getState().pushToast({
        kind: "problem",
        id: `p${String(n)}`,
        message: { id: "pim-op-failed" },
      });
    }
    const toasts = pim.getState().toasts;
    expect(toasts).toHaveLength(MAX_PIM_TOASTS);
    expect(toasts.at(-1)?.id).toBe(`p${String(count - 1)}`);
    // The same toast again replaces itself.
    pim.getState().pushToast({ kind: "problem", id: "p0", message: { id: "pim-op-failed" } });
    expect(pim.getState().toasts).toHaveLength(MAX_PIM_TOASTS);
  });
});
