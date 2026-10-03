/**
 * The settings slot's `settings.section` action (M5b): it remembers the
 * section in the UI state and shows the settings view — as the view host's
 * view in the default layout (WP165), as the settings window in a layout
 * whose main window hosts no views (the contract's fallback).
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { ActionContext } from "../../actions/registry";
import type { ResolvedLayout } from "../../api/generated";
import { normalizeLayout } from "../../api/normalize";
import { createEnv, type Env } from "../../test/harness";
import { fallbackLayout } from "../shell/testing";
import { actions } from ".";

function context(env: Env, layout: ResolvedLayout): ActionContext {
  return {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    layout,
    t: (id) => id,
    locale: "en",
  };
}

async function expectRemembered(env: Env, section: string) {
  await vi.waitFor(async () => {
    expect(await env.client.call({ type: "GetUiState" })).toEqual({
      type: "UiState",
      state: { "settings.section": section },
    });
  });
}

describe("settings actions", () => {
  it("shows the settings view at a section in the default layout", async () => {
    const env = createEnv();
    const ctx = context(env, normalizeLayout(defaultLayoutJson));
    const action = actions["settings.section"];
    expect(action?.enabled?.(ctx, ["appearance"])).toBe(true);
    expect(action?.enabled?.(ctx, ["nope"])).toBe(false);
    expect(action?.enabled?.(ctx, [])).toBe(false);
    await action?.run(ctx, ["appearance"]);
    expect(env.store.getState().activeView).toBe("settings");
    expect(env.store.getState().windows).toEqual([]);
    await expectRemembered(env, "appearance");
  });

  it("opens the settings window when the main window hosts no views", async () => {
    const env = createEnv();
    const ctx = context(env, fallbackLayout());
    await actions["settings.section"]?.run(ctx, ["calendar"]);
    expect(env.store.getState().windows).toEqual(["settings"]);
    await expectRemembered(env, "calendar");
  });
});
