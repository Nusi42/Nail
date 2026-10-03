/**
 * The mock core's window state and pop-outs (M5c, ADR 0116) behave like the
 * core (`crates/nail-core/src/windows.rs` tests); the shared hooks follow
 * the events.
 */
import { QueryClient } from "@tanstack/react-query";
import type { Event as NailEvent, WindowGeometry, WindowState } from "./generated";
import { checkPopOut, enableMockPopOuts, simulatePopOutReport } from "./windowStateMock";
import { createEnv } from "../test/harness";
import {
  applyPopOuts,
  applyWindowStateChange,
  popOutsKey,
  windowStatesKey,
} from "../state/windowState";
import { MAX_POP_OUTS } from "../state/windows";

const geometry = (x: number): WindowGeometry => ({
  rect: { x, y: 20, width: 760, height: 600 },
  maximized: false,
  snap: null,
});
const page = (x: number): WindowState => ({ page: geometry(x), pop_out: null, popped_out: false });

function recorder(env: ReturnType<typeof createEnv>): NailEvent[] {
  const events: NailEvent[] = [];
  env.client.subscribe((event) => {
    if (event.type === "WindowStateChanged" || event.type === "PopOutChanged") events.push(event);
  });
  return events;
}

describe("window state (M5c)", () => {
  it("answers GetWindowStates / SetWindowState / ResetWindowStates", async () => {
    const env = createEnv();
    const events = recorder(env);
    expect(await env.client.call({ type: "GetWindowStates" })).toEqual({
      type: "WindowStates",
      states: {},
    });
    const set = { type: "SetWindowState", window: "composer", state: page(10) } as const;
    expect(await env.client.call(set)).toEqual({
      type: "WindowStates",
      states: { composer: page(10) },
    });
    await env.client.call(set);
    await env.client.call({ type: "SetWindowState", window: "settings", state: page(5) });
    expect(events).toEqual([
      { type: "WindowStateChanged", window: "composer", state: page(10) },
      { type: "WindowStateChanged", window: "settings", state: page(5) },
    ]);
    expect(
      await env.client.call({ type: "SetWindowState", window: "composer:d1", state: page(1) }),
    ).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "window-state-issue-invalid-window" },
    });
    expect(await env.client.call({ type: "ResetWindowStates" })).toEqual({
      type: "WindowStates",
      states: {},
    });
    expect(events.slice(2)).toEqual([
      { type: "WindowStateChanged", window: "composer", state: null },
      { type: "WindowStateChanged", window: "settings", state: null },
    ]);
  });

  it("has no pop-outs without a desktop host", async () => {
    const env = createEnv();
    expect(await env.client.call({ type: "ListPopOuts" })).toEqual({
      type: "PopOuts",
      pop_outs: [],
    });
    expect(await env.client.call({ type: "PopOutWindow", window: "composer" })).toMatchObject({
      type: "Error",
      code: "unsupported",
      localized: { id: "desktop-unavailable" },
    });
  });

  it("opens, focuses, moves, returns and closes pop-outs with a simulated host", async () => {
    const env = createEnv();
    enableMockPopOuts(env.transport);
    await env.client.call({
      type: "SetWindowState",
      window: "composer",
      state: { page: null, pop_out: geometry(300), popped_out: false },
    });
    const events = recorder(env);
    const opened = await env.client.call({
      type: "PopOutWindow",
      window: "composer:d1",
      title: "New message",
      context: { draft: "d1" },
    });
    expect(opened).toEqual({
      type: "PopOuts",
      pop_outs: [
        { window: "composer:d1", geometry: geometry(300), context: { draft: "d1" }, focused: true },
      ],
    });
    await env.client.call({ type: "PopOutWindow", window: "settings" });
    simulatePopOutReport(env.transport, "composer:d1", "moved", geometry(500));
    simulatePopOutReport(env.transport, "composer:d1", "focused");
    const states = await env.client.call({ type: "GetWindowStates" });
    expect(states).toMatchObject({
      states: { composer: { pop_out: geometry(500), popped_out: true } },
    });
    expect(await env.client.call({ type: "ReturnPopOut", window: "composer:d1" })).toMatchObject({
      pop_outs: [{ window: "settings" }],
    });
    expect(await env.client.call({ type: "ReturnPopOut", window: "composer:d1" })).toMatchObject({
      type: "Error",
      code: "conflict",
      localized: { id: "pop-out-issue-not-open" },
    });
    simulatePopOutReport(env.transport, "settings", "closed");
    // Reports about windows that are not open are ignored.
    simulatePopOutReport(env.transport, "settings", "moved", geometry(1));
    expect(await env.client.call({ type: "ListPopOuts" })).toEqual({
      type: "PopOuts",
      pop_outs: [],
    });
    const changes = events.flatMap((e) =>
      e.type === "PopOutChanged" ? [`${e.window} ${e.change}`] : [],
    );
    expect(changes).toEqual([
      "composer:d1 opened",
      "settings opened",
      "composer:d1 moved",
      "composer:d1 focused",
      "composer:d1 returned",
      "settings closed",
    ]);
    const final = await env.client.call({ type: "GetWindowStates" });
    expect(final).toMatchObject({
      states: {
        composer: { popped_out: false, pop_out: geometry(500) },
        settings: { popped_out: true },
      },
    });
  });

  it("checks pop-outs like the core", () => {
    expect(checkPopOut(0, false, "composer:d1", "Hi", null)).toBeNull();
    expect(checkPopOut(0, false, "composer:", null, null)).toBe("invalid-window");
    expect(checkPopOut(0, false, "composer", null, "x".repeat(16 * 1024))).toBe("too-large");
    expect(checkPopOut(0, false, "composer", "t".repeat(257), null)).toBe("too-large");
    expect(checkPopOut(MAX_POP_OUTS, false, "composer", null, null)).toBe("too-many");
    expect(checkPopOut(MAX_POP_OUTS, true, "composer", null, null)).toBeNull();
  });

  it("applies changes to the cached state and pop-outs", () => {
    const queryClient = new QueryClient();
    applyWindowStateChange(queryClient, "composer", page(1));
    applyWindowStateChange(queryClient, "settings", page(2));
    applyWindowStateChange(queryClient, "settings", null);
    expect(queryClient.getQueryData(windowStatesKey)).toEqual({ composer: page(1) });
    applyPopOuts(queryClient, [
      { window: "settings", geometry: null, context: null, focused: true },
    ]);
    expect(queryClient.getQueryData(popOutsKey)).toHaveLength(1);
  });
});
