/**
 * M5d WP182: the toolbar customizer and its settings card are axe-clean in
 * every built-in theme (light and dark, High contrast) — with an item
 * chosen, a drag target shown and a status message — and the skin is
 * token-only and well-formed.
 */
import { act, fireEvent, waitFor } from "@testing-library/react";
import { BUILTIN_THEMES } from "../appearance";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { violations } from "../../test/axe";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { compileTheme } from "../../theme/compile";
import {
  CUSTOMIZER_CONTAINER,
  CUSTOMIZER_NARROW,
  CUSTOMIZER_SHORT,
  TOOLBAR_CUSTOMIZER_SKIN_CSS,
} from "./skin";
import { builtinTheme } from "../../test/themes";

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
    expect(env.queryClient.isMutating()).toBe(0);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

const THEMES = BUILTIN_THEMES.map(([id]) => id);

describe("toolbar customizer accessibility (M5d)", () => {
  it.each(THEMES)(
    "%s: the customizer window and the settings card have no violations",
    async (id) => {
      const theme = builtinTheme(id);
      // The theme still compiles with the customizer's skin after it.
      expect(compileTheme(theme).warnings).toEqual([]);
      const env = createEnv();
      act(() => {
        env.store.getState().openWindow("toolbar-customizer:reader");
      });
      const layout = env.transport.data.layout;
      const { container, unmount } = renderWithProviders(
        <>
          <NodeView node={widget("toolbar-editor-settings")} path="test" />
          <WindowLayer layout={layout} />
        </>,
        env,
        { theme },
      );
      const dialog = await waitFor(() =>
        must(container.ownerDocument.querySelector<HTMLElement>('[data-part="window"]')),
      );
      await settle(env);
      // An item chosen and a drop target shown.
      const first = await waitFor(() =>
        must(dialog.querySelector<HTMLElement>('[data-list="bar"] [role="option"]')),
      );
      fireEvent.click(first);
      const dataTransfer = { setData: () => undefined, effectAllowed: "", dropEffect: "" };
      fireEvent.dragStart(first, { dataTransfer });
      fireEvent.dragOver(must(dialog.querySelector('[data-list="more"]')), { dataTransfer });
      expect(dialog.querySelector('[data-list="more"][data-drop="end"]')).not.toBeNull();
      expect(await violations(container.ownerDocument.body)).toEqual([]);
      fireEvent.dragEnd(first, { dataTransfer });
      unmount();
    },
    120_000,
  );
});

describe("toolbar customizer skin", () => {
  it("closes every block it opens", () => {
    let depth = 0;
    for (const char of TOOLBAR_CUSTOMIZER_SKIN_CSS) {
      if (char === "{") depth += 1;
      if (char === "}") depth -= 1;
      expect(depth).toBeGreaterThanOrEqual(0);
    }
    expect(depth).toBe(0);
  });

  it("takes colours only from tokens", () => {
    const css = TOOLBAR_CUSTOMIZER_SKIN_CSS;
    expect(css).not.toMatch(/#[0-9a-f]{3,8}\b/i);
    expect(css).not.toMatch(/\brgba?\(|\bhsla?\(/i);
    expect(css).not.toMatch(/url\(|@import|!important/);
    for (const color of css.matchAll(
      /(?:^|[\s;{])(?:color|background-color|border-color):\s*([^;]+);/g,
    )) {
      expect(must(color[1])).toMatch(
        /^(var\(--nail-|color-mix\(|transparent|inherit|currentColor)/,
      );
    }
  });

  it("styles the parts the customizer renders", () => {
    for (const part of [
      "toolbar-customizer",
      "toolbar-customizer.list",
      "toolbar-customizer.item",
      "toolbar-customizer.item.badge",
      "toolbar-customizer.status",
      "toolbar-customizer.footer",
      "toolbar-customizer-settings.item",
    ]) {
      expect(TOOLBAR_CUSTOMIZER_SKIN_CSS).toContain(`[data-part="${part}"]`);
    }
    for (const state of ['[data-drop="before"]', '[data-drop="end"]', '[data-state="selected"]']) {
      expect(TOOLBAR_CUSTOMIZER_SKIN_CSS).toContain(state);
    }
    expect(TOOLBAR_CUSTOMIZER_SKIN_CSS).toContain('[data-part="toolbar"][data-customizing]');
  });

  // P1 (WP192): the narrow layout follows the window, not the page; the
  // lists keep a few rows and the body scrolls below that; the italic
  // Separator / Flexible space labels keep room for their overhang.
  it("adapts to the window's width and height and does not cut italic labels", () => {
    const css = TOOLBAR_CUSTOMIZER_SKIN_CSS;
    expect(css).not.toContain("@media");
    expect(css).toContain(`container: ${CUSTOMIZER_CONTAINER} / size;`);
    expect(css).toContain(`@container ${CUSTOMIZER_CONTAINER} (max-width: ${CUSTOMIZER_NARROW})`);
    const start = css.indexOf(
      `@container ${CUSTOMIZER_CONTAINER} (max-height: ${CUSTOMIZER_SHORT}) {`,
    );
    expect(start).toBeGreaterThanOrEqual(0);
    const short = css.slice(start, css.indexOf("\n}", start));
    expect(short).toMatch(/\[data-part="toolbar-customizer\.body"\] \{ overflow: auto; \}/);
    expect(short).toMatch(/min-height: auto;/);
    expect(css).toMatch(
      /\[data-kind="separator"\] \[data-part="toolbar-customizer\.item\.label"\][^{]*\{[^}]*padding-inline-end: 0\.2em;/,
    );
  });
});
