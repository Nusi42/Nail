/**
 * Writing the UI state (`useSetUiState`): answers that come back while
 * newer writes are on their way never bring an older value back (R8: the
 * answer to showing the Settings view reverted the settings section for a
 * moment, and the section shown meanwhile lost its unsaved changes).
 */
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import type { Request, Response } from "../api/generated";
import { createEnv, Providers, settleQueries, type Env } from "../test/harness";
import {
  applyUiStateEvent,
  UI_STATE_SETTINGS_SECTION,
  UI_STATE_VIEW,
  useSetUiState,
  useUiState,
  type UiStateMap,
} from "./uiState";

/** Holds every `SetUiState` until `release` lets the oldest one through. */
function holdWrites(env: Env) {
  const send = env.transport.send.bind(env.transport);
  const held: (() => void)[] = [];
  env.transport.send = (request: Request): Promise<Response> => {
    if (request.type !== "SetUiState") return send(request);
    return new Promise((resolve, reject) => {
      held.push(() => {
        send(request).then(resolve, reject);
      });
    });
  };
  return {
    release: async () => {
      // The mutation sends its request a moment after `mutate`.
      await waitFor(() => {
        expect(held.length).toBeGreaterThan(0);
      });
      const next = held.shift();
      if (next === undefined) throw new Error("no write is held");
      await act(async () => {
        next();
        await Promise.resolve();
      });
    },
  };
}

function setup() {
  const env = createEnv();
  const wrapper = ({ children }: { children: ReactNode }) => (
    <Providers env={env}>{children}</Providers>
  );
  const hook = renderHook(() => ({ state: useUiState(), set: useSetUiState() }), { wrapper });
  return { env, hook };
}

const cached = (env: Env): UiStateMap =>
  env.queryClient.getQueryData<UiStateMap>(["ui-state"]) ?? {};

interface Write {
  key: string;
  value: string;
}

/** Two writes sent at once, their answers held until `writes.release()`. */
async function writeTwice(first: Write, second: Write) {
  const { env, hook } = setup();
  await settleQueries(env);
  const writes = holdWrites(env);
  act(() => {
    hook.result.current.set.mutate(first);
    hook.result.current.set.mutate(second);
  });
  return { env, writes };
}

describe("useSetUiState", () => {
  it("keeps a newer write of another key when an older write's answer comes", async () => {
    const { env, writes } = await writeTwice(
      { key: UI_STATE_VIEW, value: "settings" },
      { key: UI_STATE_SETTINGS_SECTION, value: "appearance" },
    );
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    // The view's answer is the whole state before the section was written.
    await writes.release();
    await settleOneTurn();
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    expect(cached(env)[UI_STATE_VIEW]).toBe("settings");
    await writes.release();
    await settleQueries(env);
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    expect(cached(env)[UI_STATE_VIEW]).toBe("settings");
  });

  it("keeps the newest of two writes of one key", async () => {
    const { env, writes } = await writeTwice(
      { key: UI_STATE_SETTINGS_SECTION, value: "general" },
      { key: UI_STATE_SETTINGS_SECTION, value: "appearance" },
    );
    await writes.release();
    await settleOneTurn();
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    await writes.release();
    await settleQueries(env);
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    // The core has the newest too.
    const response = await env.client.call({ type: "GetUiState" });
    expect(response.type === "UiState" && response.state[UI_STATE_SETTINGS_SECTION]).toBe(
      "appearance",
    );
  });
});

describe("applyUiStateEvent", () => {
  it("ignores an older write's event while a newer write of the key is on its way", async () => {
    const { env, writes } = await writeTwice(
      { key: UI_STATE_SETTINGS_SECTION, value: "general" },
      { key: UI_STATE_SETTINGS_SECTION, value: "appearance" },
    );
    // The first write's event comes before either answer.
    act(() => {
      applyUiStateEvent(env.queryClient, UI_STATE_SETTINGS_SECTION, "general");
    });
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    await writes.release();
    await writes.release();
    await settleQueries(env);
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("appearance");
    // Nothing on its way: events apply again (another window's change).
    act(() => {
      applyUiStateEvent(env.queryClient, UI_STATE_SETTINGS_SECTION, "keys");
    });
    expect(cached(env)[UI_STATE_SETTINGS_SECTION]).toBe("keys");
  });
});

/** Lets pending answers and their callbacks run. */
async function settleOneTurn(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}
