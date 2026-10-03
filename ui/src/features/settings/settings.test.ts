/**
 * The settings slot's contract-stage action (M5b): `settings.section`
 * remembers the section in the UI state and opens the settings window.
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { ActionContext } from "../../actions/registry";
import { normalizeLayout } from "../../api/normalize";
import { createEnv } from "../../test/harness";
import { actions } from ".";

describe("settings actions (M5b contract stage)", () => {
  it("opens the settings window at a section", async () => {
    const env = createEnv();
    const ctx: ActionContext = {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      layout: normalizeLayout(defaultLayoutJson),
      t: (id) => id,
      locale: "en",
    };
    const action = actions["settings.section"];
    expect(action?.enabled?.(ctx, ["appearance"])).toBe(true);
    expect(action?.enabled?.(ctx, ["nope"])).toBe(false);
    expect(action?.enabled?.(ctx, [])).toBe(false);
    await action?.run(ctx, ["appearance"]);
    expect(env.store.getState().windows).toEqual(["settings"]);
    await vi.waitFor(async () => {
      expect(await env.client.call({ type: "GetUiState" })).toEqual({
        type: "UiState",
        state: { "settings.section": "appearance" },
      });
    });
  });
});
