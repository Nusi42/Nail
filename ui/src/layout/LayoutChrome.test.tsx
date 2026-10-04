/**
 * Hidden layout nodes and the layout chrome (M5d, ADR 0131): the renderer
 * skips `hidden` nodes, and a chrome provider (the layout editor in
 * customize mode) sees every node with its reference and may show hidden
 * ones.
 */
import type { ReactNode } from "react";
import type { LayoutNode, LayoutNodeRef } from "../api/generated";
import { normalizeLayout } from "../api/normalize";
import { createEnv, renderWithProviders, setWindowWidth } from "../test/harness";
import { must } from "../test/must";
import {
  LayoutChromeContext,
  PLAIN_CHROME,
  nodeRefOfPath,
  rendersNode,
  type LayoutChrome,
} from "./LayoutChrome";
import { WindowView } from "./LayoutRenderer";

afterEach(() => {
  setWindowWidth(1024);
});

/** The default layout with the mail window's folder pane (`sidebar`) hidden. */
function layoutWithHiddenSidebar() {
  const env = createEnv();
  const layout = structuredClone(env.transport.data.layout);
  const root = must(layout.windows.mail?.[0]).root;
  if (root.type !== "split") throw new Error("expected the mail split");
  const sidebar = must(root.children[0]);
  sidebar.hidden = true;
  return { env, layout };
}

describe("layout chrome (M5d)", () => {
  it("parses renderer paths into node references", () => {
    expect(nodeRefOfPath("mail/0/2/1")).toEqual({ window: "mail", breakpoint: 0, path: [2, 1] });
    expect(nodeRefOfPath("main/1")).toEqual({ window: "main", breakpoint: 1, path: [] });
    expect(nodeRefOfPath("settings/accounts/0")).toBeNull();
    expect(nodeRefOfPath("")).toBeNull();
  });

  it("renders hidden nodes only for a chrome that shows them", () => {
    const hidden = { type: "widget", kind: "spacer", hidden: true } as const;
    const node = normalizeLayout({
      windows: { w: [{ root: { type: "stack", children: [hidden] } }] },
    }).windows.w?.[0]?.root;
    const child = must(node?.type === "stack" ? node.children[0] : undefined);
    expect(child.hidden).toBe(true);
    expect(rendersNode(child, PLAIN_CHROME)).toBe(false);
    expect(rendersNode(child, { ...PLAIN_CHROME, showHidden: true })).toBe(true);
    // `hidden` stays absent when not set, like the core's JSON.
    const shown = normalizeLayout({
      windows: { w: [{ root: { type: "widget", kind: "spacer" } }] },
    }).windows.w?.[0]?.root;
    expect(shown && "hidden" in shown).toBe(false);
  });

  it("skips a hidden pane and gives its space to the others", () => {
    setWindowWidth(1200);
    const { env, layout } = layoutWithHiddenSidebar();
    const { container } = renderWithProviders(<WindowView layout={layout} name="mail" />, env);
    const split = must(container.querySelector<HTMLElement>('[data-node-id="panes"]'));
    expect(split.querySelectorAll(':scope > [data-part="split.pane"]')).toHaveLength(2);
    expect(container.querySelector('[data-node-id="sidebar"]')).toBeNull();
  });

  it("lets a chrome wrap every node and show hidden ones", () => {
    setWindowWidth(1200);
    const { env, layout } = layoutWithHiddenSidebar();
    const seen: { node: LayoutNode; ref: LayoutNodeRef }[] = [];
    const chrome: LayoutChrome = {
      showHidden: true,
      renderNode: (node, ref, content: ReactNode) => {
        seen.push({ node, ref });
        return (
          <div data-part="test-chrome" data-hidden={node.hidden === true ? "yes" : undefined}>
            {content}
          </div>
        );
      },
    };
    const { container } = renderWithProviders(
      <LayoutChromeContext.Provider value={chrome}>
        <WindowView layout={layout} name="mail" />
      </LayoutChromeContext.Provider>,
      env,
    );
    expect(container.querySelector('[data-node-id="sidebar"]')).not.toBeNull();
    expect(container.querySelectorAll('[data-part="test-chrome"][data-hidden="yes"]')).toHaveLength(
      1,
    );
    const sidebar = seen.find((s) => s.node.id === "sidebar");
    expect(sidebar?.ref).toEqual({ window: "mail", breakpoint: 0, path: [0] });
    expect(seen.find((s) => s.node.id === "panes")?.ref.path).toEqual([]);
  });
});
