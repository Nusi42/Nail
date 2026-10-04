/**
 * The mock core's UI state (M5b) behaves like the core: checks, answers
 * with the whole state, `UiStateChanged` only on a change; the shared hook
 * follows events.
 */
import { QueryClient } from "@tanstack/react-query";
import type { Event as NailEvent } from "./generated";
import {
  checkUiStateWrite,
  isUiStateKey,
  MAX_UI_STATE_KEYS,
  MAX_UI_STATE_VALUE_BYTES,
} from "./uiStateMock";
import { createEnv } from "../test/harness";
import { applyUiStateChange, stringList, uiStateKey } from "../state/uiState";

describe("UI state (M5b)", () => {
  it("checks keys and writes like nail_api::ui_state", () => {
    for (const good of ["folder-pane.collapsed", "shell.view", "window.composer-1", "a"]) {
      expect(isUiStateKey(good)).toBe(true);
    }
    for (const bad of [
      "",
      "Shell.view",
      "shell..view",
      ".x",
      "x.",
      "1x",
      "a-",
      "a b",
      "x".repeat(65),
    ]) {
      expect(isUiStateKey(bad)).toBe(false);
    }
    expect(checkUiStateWrite({}, "Bad", 1)).toBe("invalid-key");
    expect(checkUiStateWrite({}, "Bad", null)).toBe("invalid-key");
    expect(checkUiStateWrite({}, "a.b", "x".repeat(MAX_UI_STATE_VALUE_BYTES))).toBe("too-large");
    const full = Object.fromEntries(
      Array.from({ length: MAX_UI_STATE_KEYS }, (_v, i) => [`k.k${String(i)}`, i]),
    );
    expect(checkUiStateWrite(full, "new.key", 1)).toBe("too-many-keys");
    expect(checkUiStateWrite(full, "k.k0", 2)).toBeNull();
    expect(checkUiStateWrite(full, "new.key", null)).toBeNull();
  });

  it("answers GetUiState / SetUiState and sends UiStateChanged", async () => {
    const env = createEnv();
    const events: NailEvent[] = [];
    env.client.subscribe((event) => events.push(event));
    expect(await env.client.call({ type: "GetUiState" })).toEqual({ type: "UiState", state: {} });
    const set = { type: "SetUiState", key: "shell.view", value: "calendar" } as const;
    expect(await env.client.call(set)).toEqual({
      type: "UiState",
      state: { "shell.view": "calendar" },
    });
    await env.client.call(set);
    await env.client.call({ type: "SetUiState", key: "shell.view", value: null });
    const changes = events.filter((e) => e.type === "UiStateChanged");
    expect(changes).toEqual([
      { type: "UiStateChanged", key: "shell.view", value: "calendar" },
      { type: "UiStateChanged", key: "shell.view", value: null },
    ]);
    const bad = await env.client.call({ type: "SetUiState", key: "Nope", value: 1 });
    expect(bad).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "ui-state-issue-invalid-key" },
    });
  });

  it("applies changes to the cached state", () => {
    const queryClient = new QueryClient();
    applyUiStateChange(queryClient, "folder-pane.collapsed", ["favorites"]);
    applyUiStateChange(queryClient, "shell.view", "mail");
    applyUiStateChange(queryClient, "shell.view", null);
    expect(queryClient.getQueryData(uiStateKey)).toEqual({
      "folder-pane.collapsed": ["favorites"],
    });
    expect(stringList(["a", 1, "b"])).toEqual(["a", "b"]);
    expect(stringList("a")).toEqual([]);
  });
});
