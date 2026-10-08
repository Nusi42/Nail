/**
 * The settings page stays axe-clean in every built-in theme and both
 * densities (M5b WP163): the navigation, a section with its cards, the
 * Appearance section (Density control, theme gallery) and the search with
 * highlighted results. jsdom has no layout, so colour contrast is left to
 * the Playwright scan; this catches names, roles and structure, and makes
 * sure every theme compiles the page's skin without dropped rules.
 */
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { defaultTheme } from "../../api/mockData";
import { UI_STATE_SETTINGS_SECTION } from "../../state/uiState";
import { violations } from "../../test/axe";
import {
  createEnv,
  renderNode,
  settleQueries,
  widget,
  stubElementSizesEachTest,
} from "../../test/harness";
import { compileTheme } from "../../theme/compile";
import { resolveMockTheme } from "../appearance/themes";
import { SETTINGS_SKIN_CSS } from "./skin";
import { THEME_CASES } from "../../test/themes";

stubElementSizesEachTest();

describe("settings page: axe in every theme and density", () => {
  it.each(THEME_CASES)(
    "%s / %s",
    async (id, density) => {
      const theme = resolveMockTheme(defaultTheme(), id, { density, overrides: {} });
      const compiled = compileTheme(theme);
      expect(compiled.warnings).toEqual([]);
      expect(compiled.css).toContain(SETTINGS_SKIN_CSS);

      const env = createEnv();
      await env.client.call({
        type: "SetUiState",
        key: UI_STATE_SETTINGS_SECTION,
        value: "appearance",
      });
      const user = userEvent.setup();
      const { container } = renderNode(widget("settings-page"), env, { theme });
      await settleQueries(env);
      expect(container.querySelector('[data-part="theme-gallery"]')).not.toBeNull();
      expect(container.querySelector('[data-part="segmented"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);

      await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "key");
      const results = screen.getByRole("list", { name: "Search results" });
      expect(within(results).getAllByRole("button").length).toBeGreaterThan(1);
      expect(results.querySelector('[data-part="settings-page.mark"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
      await settleQueries(env);
    },
    120_000,
  );
});
