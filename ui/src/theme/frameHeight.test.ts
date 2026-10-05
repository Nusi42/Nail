/** The frame-height contract (P3, ADR 0164). */
import {
  clampFrameHeight,
  contentHeight,
  FrameHeightGuard,
  OPAQUE_ORIGIN,
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

  it("accepts its own frame's message from the opaque origin the sandbox gives it", () => {
    expect(
      frameHeightOf(
        event({ type: FRAME_HEIGHT_MESSAGE, height: 300 }, frame, OPAQUE_ORIGIN),
        frame,
        SANDBOX,
      ),
    ).toBe(300);
    // Another window with an opaque origin is still not the frame.
    expect(
      frameHeightOf(
        event({ type: FRAME_HEIGHT_MESSAGE, height: 300 }, {}, OPAQUE_ORIGIN),
        frame,
        SANDBOX,
      ),
    ).toBe(null);
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

describe("content heights", () => {
  it("takes the root's height when the document fits, so a frame can shrink", () => {
    expect(
      contentHeight({ clientHeight: 400, scrollHeight: 400, offsetHeight: 120, innerHeight: 400 }),
    ).toBe(120);
  });

  it("takes the overflow when the document is taller than the frame", () => {
    expect(
      contentHeight({ clientHeight: 120, scrollHeight: 980, offsetHeight: 960, innerHeight: 120 }),
    ).toBe(980);
  });

  it("adds a horizontal scrollbar's thickness", () => {
    expect(
      contentHeight({ clientHeight: 385, scrollHeight: 400, offsetHeight: 400, innerHeight: 400 }),
    ).toBe(415);
  });

  it("clamps the very long ones (they scroll inside)", () => {
    expect(
      contentHeight({ clientHeight: 100, scrollHeight: 9e6, offsetHeight: 9e6, innerHeight: 100 }),
    ).toBe(MAX_FRAME_HEIGHT);
  });
});

describe("the frame height guard", () => {
  it("follows the content up and down", () => {
    const guard = new FrameHeightGuard();
    expect(guard.accept(100)).toBe(100);
    expect(guard.accept(100)).toBe(null);
    expect(guard.accept(400)).toBe(400);
    expect(guard.accept(250)).toBe(250);
  });

  it("settles a flip-flop at the larger height", () => {
    const guard = new FrameHeightGuard();
    expect(guard.accept(500)).toBe(500);
    expect(guard.accept(300)).toBe(300);
    expect(guard.accept(500)).toBe(500);
    // It would shrink again: it stays.
    expect(guard.accept(300)).toBe(null);
    expect(guard.accept(600)).toBe(600);
    // A new width or document starts over.
    guard.reset();
    expect(guard.accept(300)).toBe(300);
  });

  it("stops a runaway (content that grows with its frame)", () => {
    const guard = new FrameHeightGuard();
    let height = 200;
    expect(guard.accept(height)).toBe(height);
    let applied = height;
    for (let step = 0; step < 200; step += 1) {
      height += 24;
      applied = guard.accept(height) ?? applied;
    }
    expect(applied).toBeLessThan(200 + 24 * 45);
    // Different steps (images loading one after another) are no runaway.
    const images = new FrameHeightGuard();
    let last = 0;
    for (const h of [100, 180, 400, 460, 900, 1010]) last = images.accept(h) ?? last;
    expect(last).toBe(1010);
  });
});
