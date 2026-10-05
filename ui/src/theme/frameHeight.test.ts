/** The frame-height contract (P3, ADR 0164). */
import {
  clampFrameHeight,
  FRAME_HEIGHT_MESSAGE,
  frameHeightOf,
  MAX_FRAME_HEIGHT,
  REMOTE_SANDBOX,
  SRCDOC_SANDBOX,
} from "./frameHeight";

const SANDBOX = "https://nail.local:4781";

describe("frame heights", () => {
  const frame = {} as Window;
  const event = (data: unknown, source: unknown = frame, origin = SANDBOX) =>
    ({ data, source, origin }) as Pick<MessageEvent, "data" | "origin" | "source">;

  it("accepts only its own frame's message from the sandbox origin", () => {
    expect(
      frameHeightOf(event({ type: FRAME_HEIGHT_MESSAGE, height: 812.4 }), frame, SANDBOX),
    ).toBe(813);
    expect(
      frameHeightOf(event({ type: FRAME_HEIGHT_MESSAGE, height: 10 }, {}), frame, SANDBOX),
    ).toBe(null);
    expect(
      frameHeightOf(
        event({ type: FRAME_HEIGHT_MESSAGE, height: 10 }, frame, "https://evil"),
        frame,
        SANDBOX,
      ),
    ).toBe(null);
    expect(frameHeightOf(event({ type: "other", height: 10 }), frame, SANDBOX)).toBe(null);
    expect(frameHeightOf(event({ type: FRAME_HEIGHT_MESSAGE, height: "9" }), frame, SANDBOX)).toBe(
      null,
    );
    expect(frameHeightOf(event("nail-frame-height"), frame, SANDBOX)).toBe(null);
    expect(frameHeightOf(event({ type: FRAME_HEIGHT_MESSAGE, height: 1 }), null, SANDBOX)).toBe(
      null,
    );
  });

  it("clamps heights", () => {
    expect(clampFrameHeight(-5)).toBe(0);
    expect(clampFrameHeight(Number.NaN)).toBe(0);
    expect(clampFrameHeight(1e9)).toBe(MAX_FRAME_HEIGHT);
  });

  it("never combines scripts with the app's origin", () => {
    expect(SRCDOC_SANDBOX.split(" ")).not.toContain("allow-scripts");
    expect(REMOTE_SANDBOX.split(" ")).not.toContain("allow-same-origin");
  });
});
