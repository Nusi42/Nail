/**
 * P1 (WP191): the install dialog's source form stacks its rows with a gap,
 * the "Install from" choices wrap as a bordered group of aligned radios,
 * and the plug-in manager's toolbar is a wrapping row.
 */
import { PLUGIN_BASE_CSS } from "./parts";

describe("plug-in parts (P1)", () => {
  it("lays out the install dialog and the manager's toolbar", () => {
    expect(PLUGIN_BASE_CSS).toContain(
      `[data-part="plugin-install.source"] { display: flex; flex-direction: column; gap: var(--nail-space-md); min-width: 0; }`,
    );
    expect(PLUGIN_BASE_CSS).toMatch(
      /\[data-part="plugin-install.kinds"\] \{ display: flex; flex-wrap: wrap;[^}]*border: 1px solid var\(--nail-color-border\)/,
    );
    expect(PLUGIN_BASE_CSS).toMatch(
      /\[data-part="plugin-install.kind"\] \{ display: inline-flex; align-items: center;/,
    );
    expect(PLUGIN_BASE_CSS).toContain(
      `[data-part="plugin-manager.toolbar"] { display: flex; flex-wrap: wrap; align-items: center; }`,
    );
  });
});
