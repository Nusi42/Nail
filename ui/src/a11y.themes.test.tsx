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
import { act, waitFor } from "@testing-library/react";
import type { Density } from "./api/generated";
import { defaultTheme } from "./api/mockData";
import { BUILTIN_THEMES } from "./features/appearance";
import { resolveMockTheme } from "./features/appearance/themes";
import { NodeView, WindowView } from "./layout/LayoutRenderer";
import { emptyComposer } from "./state/compose";
import { violations } from "./test/axe";
import { createEnv, renderWithProviders, stubElementSizes, widget, type Env } from "./test/harness";
import { compileTheme } from "./theme/compile";

const INBOX = "acc-personal/Inbox";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
});

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

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

const DENSITIES: readonly Density[] = ["comfortable", "compact"];
const CASES = BUILTIN_THEMES.flatMap(([id]) => DENSITIES.map((d) => [id, d] as const));
const themeOf = (id: string, density: Density) =>
  resolveMockTheme(defaultTheme(), id, { density, overrides: {} });

describe("axe in every theme and density (M5a)", () => {
  it("covers the five built-in themes", () => {
    expect(BUILTIN_THEMES.map(([id]) => id)).toEqual([
      "frost",
      "sage",
      "graphite",
      "nocturne",
      "high-contrast",
    ]);
  });

  it.each(CASES)("%s / %s compiles without dropped rules", (id, density) => {
    const theme = themeOf(id, density);
    expect(compileTheme(theme).warnings).toEqual([]);
  });

  it.each(CASES)(
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
        await settle(env);
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
      await settle(env);
      expect(container.querySelector('[data-part="theme-gallery"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    // The first variant also loads the lazy editor chunks and axe (about
    // 45 s alone, more while the machine is busy).
    180_000,
  );
});
