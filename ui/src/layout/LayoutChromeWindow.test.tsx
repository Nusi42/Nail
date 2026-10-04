/**
 * Chromes registered per window (M5d WP181): `WindowView` provides the
 * chrome registered for its window, lets it frame the window, keeps it
 * from leaking into nested windows of other names, and gives hidden nodes
 * the chrome shows only the room of their header.
 */
import { act, cleanup } from "@testing-library/react";
import type { ReactNode } from "react";
import type { LayoutNode, ResolvedLayout } from "../api/generated";
import { normalizeLayout } from "../api/normalize";
import { createEnv, renderWithProviders, setWindowWidth } from "../test/harness";
import { must } from "../test/must";
import {
  LayoutChromeContext,
  PLAIN_CHROME,
  chromeForWindow,
  registeredWindowChrome,
  setWindowChrome,
  type LayoutChrome,
} from "./LayoutChrome";
import { WindowView } from "./LayoutRenderer";

/** The geometry rule applied to an element, as CSS text. */
function geometryOf(element: Element): string {
  const id = element.getAttribute("data-geom") ?? "";
  const rules =
    document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet?.cssRules ?? [];
  return (
    [...rules].map((r) => r.cssText).find((text) => text.startsWith(`[data-geom="${id}"]`)) ?? ""
  );
}

afterEach(() => {
  cleanup();
  setWindowChrome("mail", null);
  setWindowChrome("w", null);
  setWindowWidth(1024);
});

function framing(window: string, seen: string[] = []): LayoutChrome {
  return {
    window,
    showHidden: true,
    renderNode: (node, ref, content: ReactNode) => {
      seen.push(`${ref.window}:${node.id ?? node.type}`);
      return content;
    },
    renderWindow: (name, breakpoint, content) => (
      <div data-part="test-frame" data-window={name} data-breakpoint={breakpoint}>
        {content}
      </div>
    ),
  };
}

describe("window chromes (M5d)", () => {
  it("keeps a registry of chromes per window", () => {
    const chrome = framing("w");
    expect(registeredWindowChrome("w")).toBeNull();
    setWindowChrome("w", chrome);
    expect(registeredWindowChrome("w")).toBe(chrome);
    setWindowChrome("w", chrome);
    setWindowChrome("w", null);
    setWindowChrome("w", null);
    expect(registeredWindowChrome("w")).toBeNull();
  });

  it("chooses the registered chrome, else the inherited one for its own window", () => {
    const own = framing("a");
    const other = framing("b");
    expect(chromeForWindow("a", own, PLAIN_CHROME)).toBe(own);
    expect(chromeForWindow("a", null, own)).toBe(own);
    expect(chromeForWindow("a", null, other)).toBe(PLAIN_CHROME);
    const any: LayoutChrome = { ...PLAIN_CHROME, showHidden: true };
    expect(chromeForWindow("a", null, any)).toBe(any);
  });

  it("frames the registered window and re-renders when the chrome changes", () => {
    setWindowWidth(1200);
    const env = createEnv();
    const layout = env.transport.data.layout;
    const seen: string[] = [];
    const { container } = renderWithProviders(<WindowView layout={layout} name="mail" />, env);
    expect(container.querySelector('[data-part="test-frame"]')).toBeNull();
    act(() => {
      setWindowChrome("mail", framing("mail", seen));
    });
    const frame = must(container.querySelector('[data-part="test-frame"]'));
    expect(frame).toHaveAttribute("data-window", "mail");
    expect(frame).toHaveAttribute("data-breakpoint", "0");
    expect(seen).toContain("mail:reader");
    act(() => {
      setWindowChrome("mail", null);
    });
    expect(container.querySelector('[data-part="test-frame"]')).toBeNull();
  });

  it("does not hand a window's chrome to a nested window of another name", () => {
    const env = createEnv();
    const layout: ResolvedLayout = normalizeLayout({
      windows: {
        outer: [{ root: { type: "stack", id: "outer-root", children: [] } }],
        inner: [{ root: { type: "widget", kind: "spacer", id: "inner-spacer" } }],
      },
    });
    const seen: string[] = [];
    renderWithProviders(
      <LayoutChromeContext.Provider value={framing("outer", seen)}>
        <WindowView layout={layout} name="outer" />
        <WindowView layout={layout} name="inner" />
      </LayoutChromeContext.Provider>,
      env,
    );
    expect(seen).toEqual(["outer:outer-root"]);
  });

  it("gives a hidden node the chrome shows only the room of its content", () => {
    setWindowWidth(1200);
    const env = createEnv();
    const layout = structuredClone(env.transport.data.layout);
    const root = must(layout.windows.mail?.[0]).root;
    if (root.type !== "split") throw new Error("split");
    must(root.children[0]).hidden = true;
    const listPane = must(root.children[1]);
    if (listPane.type !== "stack") throw new Error("stack");
    const list: LayoutNode = must(listPane.children[4]);
    list.hidden = true;
    const { container } = renderWithProviders(
      <LayoutChromeContext.Provider value={{ ...PLAIN_CHROME, showHidden: true }}>
        <WindowView layout={layout} name="mail" />
      </LayoutChromeContext.Provider>,
      env,
    );
    const split = must(container.querySelector('[data-node-id="panes"]'));
    const first = must(split.querySelector(':scope > [data-part="split.pane"]'));
    // A hidden pane is as wide as its content, without its limits.
    expect(geometryOf(first)).toContain("flex: 0 0 auto");
    expect(geometryOf(first)).not.toContain("min-width");
    // A hidden widget of a stack gets an `auto` row (the list's was `1fr`).
    const stack = must(container.querySelector('[data-node-id="list-pane"]'));
    expect(geometryOf(stack)).toMatch(/grid-template-rows: auto/);
    expect(geometryOf(stack)).not.toContain("1fr");
  });
});
