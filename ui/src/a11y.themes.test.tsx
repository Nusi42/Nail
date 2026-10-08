/**
 * M5a: axe stays clean in every built-in theme and both densities. Key
 * screens (the main window with a message open, the composer, Settings and
 * the appearance page) render with each resolved theme (the mock core's
 * resolution of `defaults/themes/*.kdl`). jsdom has no layout, so colour
 * contrast is left to the Playwright scan and the theme contrast test
 * (WP151); this catches structure that a theme or density could break
 * (hidden labels, roles, names) and makes sure every theme compiles without
 * dropped rules.
 */
import type { Density } from "./api/generated";
import { defaultTheme } from "./api/mockData";
import { resolveMockTheme } from "./features/appearance/themes";
import { NodeView, WindowView } from "./layout/LayoutRenderer";
import { emptyComposer } from "./state/compose";
import { violations } from "./test/axe";
import {
  createEnv,
  renderWithProviders,
  widget,
  type Env,
  settleQueries,
  stubElementSizesEachTest,
} from "./test/harness";
import { compileTheme } from "./theme/compile";
import { THEME_CASES, itCoversTheBuiltInThemes } from "./test/themes";
import { INBOX } from "./test/mockIds";

stubElementSizesEachTest();

function mailEnv(): Env {
  const env = createEnv({
    state: {
      activeAccountId: "acc-personal",
      activeFolderId: INBOX,
      composer: { ...emptyComposer(), identityId: "ada@example.org" },
    },
  });
  env.store.getState().selectMessage(`${INBOX}#1`);
  return env;
}

const themeOf = (id: string, density: Density) =>
  resolveMockTheme(defaultTheme(), id, { density, overrides: {} });

describe("axe in every theme and density (M5a)", () => {
  itCoversTheBuiltInThemes();

  it.each(THEME_CASES)("%s / %s compiles without dropped rules", (id, density) => {
    const theme = themeOf(id, density);
    expect(compileTheme(theme).warnings).toEqual([]);
  });

  it.each(THEME_CASES)(
    "%s / %s: main window, composer, settings and appearance have no violations",
    async (id, density) => {
      const theme = themeOf(id, density);
      for (const name of ["main", "composer", "settings"]) {
        const env = mailEnv();
        const layout = env.transport.data.layout;
        const { container, unmount } = renderWithProviders(
          <WindowView layout={layout} name={name} />,
          env,
          { theme },
        );
        await settleQueries(env, 20);
        expect(document.documentElement.dataset.nailDensity).toBe(density);
        expect(await violations(container)).toEqual([]);
        unmount();
      }
      const env = mailEnv();
      const { container } = renderWithProviders(
        <NodeView node={widget("appearance-settings")} path="test" />,
        env,
        { theme },
      );
      await settleQueries(env, 20);
      expect(container.querySelector('[data-part="theme-gallery"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    // The first variant also loads the lazy editor chunks and axe (about
    // 45 s alone, more while the machine is busy).
    180_000,
  );
});
