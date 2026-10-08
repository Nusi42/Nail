/**
 * One source per window size (Q2, SSOT; ADR 0116's layers): a feature's
 * `windows` entry gives a size or minimum size only for windows that
 * defaults/layout.kdl does not size, because the layout's `window`
 * properties win over the feature's anyway.
 */
import { describe, expect, it } from "vitest";
import { defaultLayout } from "../api/mockData";
import { appRegistry } from "./registry";

describe("window sizes", () => {
  it("are given once: by the default layout or by the feature, never by both", () => {
    const frames = defaultLayout().window_frames;
    const twice = Object.entries(appRegistry.windows).flatMap(([kind, spec]) => {
      const frame = frames[kind];
      if (frame === undefined) return [];
      return (
        [
          ["size", spec.size !== undefined && frame.size !== null],
          ["min_size", spec.min_size !== undefined && frame.min_size !== null],
        ] as const
      )
        .filter(([, both]) => both)
        .map(([field]) => `${kind}.${field}`);
    });
    expect(twice).toEqual([]);
  });

  it("still sizes the composer, the rules and the snoozed list from the layout", () => {
    const frames = defaultLayout().window_frames;
    expect(frames.composer?.size).toEqual({ width: 820, height: 720 });
    expect(frames.composer?.min_size).toEqual({ width: 520, height: 400 });
    expect(frames.composer?.instances).toBe("multiple");
    expect(frames.rules?.size).toEqual({ width: 900, height: 640 });
    expect(frames.snoozed?.size).toEqual({ width: 720, height: 520 });
  });
});
