/**
 * The rail and every hosted view stay axe-clean in all five built-in themes
 * and both densities (M5b WP161). jsdom has no layout, so colour contrast is
 * left to the Playwright scan; this catches names, roles and landmarks.
 * Also checks the skin reaches the compiled theme.
 */
import type { Density } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { WindowView } from "../../layout/LayoutRenderer";
import { violations } from "../../test/axe";
import {
  createEnv,
  renderWithProviders,
  settleQueries,
  stubElementSizes,
} from "../../test/harness";
import { compileTheme } from "../../theme/compile";
import { BUILTIN_THEMES, resolveMockTheme } from "../appearance";
import { seedAgentsDemo } from "../agents";
import { SHELL_SKIN_CSS } from "./skin";
import { hostLayout } from "./testing";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => restoreSizes());

const DENSITIES: readonly Density[] = ["comfortable", "compact"];
/** Every theme and density, each with one of the views (every view twice). */
const VIEWS = ["mail", "calendar", "contacts", "tasks", "settings"] as const;
const CASES = BUILTIN_THEMES.flatMap(([id]) => DENSITIES.map((d) => [id, d] as const)).map(
  ([id, density], i) => [id, density, VIEWS[i % VIEWS.length] ?? "mail"] as const,
);

describe("shell accessibility", () => {
  it.each(CASES)(
    "%s / %s: the rail and the %s view have no violations",
    async (id, density, view) => {
      const theme = resolveMockTheme(defaultTheme(), id, { density, overrides: {} });
      const env = createEnv({
        state: {
          activeAccountId: "acc-personal",
          activeFolderId: "acc-personal/Inbox",
          activeView: view,
        },
      });
      env.store.getState().selectMessage("acc-personal/Inbox#1");
      seedAgentsDemo(env.transport);
      const layout = hostLayout({ "show-labels": density === "comfortable" });
      const { container } = renderWithProviders(<WindowView layout={layout} name="main" />, env, {
        theme,
        layout,
      });
      await settleQueries(env);
      expect(
        container.querySelector(`[data-part="view-host"][data-view="${view}"]`),
      ).not.toBeNull();
      expect(container.querySelector('[data-part="nav-rail.badge"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    60_000,
  );

  it("emits the shell skin in every theme", () => {
    expect(SHELL_SKIN_CSS).toContain('[data-part="nav-rail"]');
    for (const part of [
      "nav-rail.item",
      "nav-rail.badge",
      "nav-rail.logo",
      "nav-rail.label",
      "nav-rail.spacer",
      "nav-rail.separator",
      "nav-rail.account",
      "view-host",
      "view-host.view",
    ]) {
      expect(SHELL_SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
    for (const [id] of BUILTIN_THEMES) {
      const theme = resolveMockTheme(defaultTheme(), id, { density: "comfortable", overrides: {} });
      expect(compileTheme(theme).css).toContain(SHELL_SKIN_CSS);
    }
  });

  it("styles only with tokens: no colours except the avatar-fg fallback", () => {
    const hex = SHELL_SKIN_CSS.match(/#[0-9a-f]{3,8}\b/gi) ?? [];
    expect(hex).toEqual(["#ffffff"]);
    // A hidden view must stay hidden although the skin sets `display`.
    expect(SHELL_SKIN_CSS).toContain('[data-part="view-host.view"][hidden] { display: none; }');
    // The active view is marked by aria-current, not only by a state.
    expect(SHELL_SKIN_CSS).toContain('[data-part="nav-rail.item"][aria-current="page"]');
  });
});
