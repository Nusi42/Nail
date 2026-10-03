import { act, fireEvent, screen, within } from "@testing-library/react";
import { normalizeLayout } from "../api/normalize";
import { createEnv, renderWithProviders, setWindowWidth } from "../test/harness";
import { WindowView } from "./LayoutRenderer";

const styleText = () =>
  [
    ...(document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet?.cssRules ??
      []),
  ]
    .map((r) => r.cssText)
    .join("\n");

/** The geometry rule applied to an element, as CSS text. */
function geometryOf(element: Element): string {
  const id = element.getAttribute("data-geom") ?? "";
  const rule = [
    ...(document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet?.cssRules ??
      []),
  ]
    .map((r) => r.cssText)
    .find((text) => text.startsWith(`[data-geom="${id}"]`));
  return rule ?? "";
}

afterEach(() => setWindowWidth(1024));

describe("layout engine", () => {
  it("renders the wide default layout as stack → split with three panes", async () => {
    setWindowWidth(1200);
    const env = createEnv();
    const { container } = renderWithProviders(
      <WindowView layout={env.transport.data.layout} />,
      env,
    );
    const root = container.querySelector('[data-node-id="root"]');
    expect(root).toHaveAttribute("data-part", "stack");
    const split = container.querySelector('[data-node-id="panes"]') as HTMLElement;
    expect(split).toHaveAttribute("data-part", "split");
    expect(split).toHaveAttribute("data-orientation", "horizontal");
    const panes = split.querySelectorAll(':scope > [data-part="split.pane"]');
    expect(panes).toHaveLength(3);
    expect(split.querySelectorAll(':scope > [data-part="split.divider"]')).toHaveLength(2);
    expect(geometryOf(panes[0] as Element)).toContain("flex: 0 0 240px");
    expect(geometryOf(panes[0] as Element)).toContain("min-width: 160px");
    expect(geometryOf(panes[0] as Element)).toContain("max-width: 480px");
    expect(geometryOf(panes[2] as Element)).toContain("flex: 1.4 1 0px");
    // The config errors render nothing while every file is valid: no track.
    expect(container.querySelector('[data-part="config-errors"]')).toBeNull();
    expect(geometryOf(root as Element)).toContain(
      "grid-template-rows: auto minmax(0, 1fr) auto auto;",
    );
    // Widgets from the registry.
    for (const kind of [
      "account-switcher",
      "folder-tree",
      "message-list",
      "toolbar",
      "message-view",
      "status-bar",
    ]) {
      expect(container.querySelector(`[data-part="${kind}"]`)).toBeInTheDocument();
    }
    expect(await screen.findByRole("region", { name: "Folders" })).toBe(
      container.querySelector('[data-node-id="sidebar"]'),
    );
  });

  it("switches to the tabs tree below 900px and back on resize", () => {
    setWindowWidth(1200);
    const env = createEnv();
    const { container } = renderWithProviders(
      <WindowView layout={env.transport.data.layout} />,
      env,
    );
    expect(container.querySelector('[data-part="split"]')).toBeInTheDocument();

    setWindowWidth(600);
    expect(container.querySelector('[data-part="split"]')).not.toBeInTheDocument();
    const tabs = screen.getAllByRole("tab");
    expect(tabs.map((t) => t.textContent)).toEqual(["Folders", "Messages", "Message"]);
    expect(tabs[1]).toHaveAttribute("aria-selected", "true");
    expect(tabs[1]).toHaveAttribute("data-state", "selected");
    const panels = container.querySelectorAll('[data-part="tabs.panel"]');
    expect(panels[0]).not.toBeVisible();
    expect(panels[1]).toBeVisible();

    setWindowWidth(1000);
    expect(container.querySelector('[data-part="tabs"]')).not.toBeInTheDocument();
    expect(container.querySelector('[data-part="split"]')).toBeInTheDocument();
  });

  it("switches tabs by click and arrow keys", () => {
    setWindowWidth(600);
    const env = createEnv();
    renderWithProviders(<WindowView layout={env.transport.data.layout} />, env);
    const tabs = screen.getAllByRole("tab");
    fireEvent.click(tabs[2] as HTMLElement);
    expect(tabs[2]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(tabs[2] as HTMLElement, { key: "ArrowRight" });
    expect(tabs[0]).toHaveAttribute("aria-selected", "true");
    expect(tabs[0]).toHaveFocus();
    fireEvent.keyDown(tabs[0] as HTMLElement, { key: "ArrowLeft" });
    expect(tabs[2]).toHaveAttribute("aria-selected", "true");
    expect(env.store.getState().activeTabs.panes).toBe(2);
  });

  it("resizes a split with the keyboard and the pointer", () => {
    setWindowWidth(1200);
    const env = createEnv();
    const { container } = renderWithProviders(
      <WindowView layout={env.transport.data.layout} />,
      env,
    );
    const [divider] = screen.getAllByRole("separator", { name: "Resize panes" });
    const pane = container.querySelector('[data-part="split.pane"]') as HTMLElement;

    fireEvent.keyDown(divider as HTMLElement, { key: "ArrowRight" });
    expect(env.store.getState().splitSizes.panes?.[0]).toBe("256px");
    expect(geometryOf(pane)).toContain("flex: 0 0 256px");
    fireEvent.keyDown(divider as HTMLElement, { key: "ArrowLeft", shiftKey: true });
    expect(env.store.getState().splitSizes.panes?.[0]).toBe("192px");
    // Clamped at the pane's minimum (160px).
    fireEvent.keyDown(divider as HTMLElement, { key: "ArrowLeft", shiftKey: true });
    expect(env.store.getState().splitSizes.panes?.[0]).toBe("160px");

    fireEvent.pointerDown(divider as HTMLElement, { pointerId: 1, button: 0, clientX: 100 });
    fireEvent.pointerMove(divider as HTMLElement, { pointerId: 1, clientX: 150 });
    expect(env.store.getState().splitSizes.panes?.[0]).toBe("210px");
    fireEvent.pointerUp(divider as HTMLElement, { pointerId: 1 });
    fireEvent.pointerMove(divider as HTMLElement, { pointerId: 1, clientX: 400 });
    expect(env.store.getState().splitSizes.panes?.[0]).toBe("210px");
  });

  it("hides and shows nodes via the UI store (view.toggle)", () => {
    setWindowWidth(1200);
    const env = createEnv();
    const { container } = renderWithProviders(
      <WindowView layout={env.transport.data.layout} />,
      env,
    );
    act(() => env.store.getState().toggleNode("sidebar"));
    expect(container.querySelector('[data-node-id="sidebar"]')).not.toBeInTheDocument();
    const split = container.querySelector('[data-node-id="panes"]') as HTMLElement;
    expect(split.querySelectorAll(':scope > [data-part="split.pane"]')).toHaveLength(2);
    expect(split.querySelectorAll(':scope > [data-part="split.divider"]')).toHaveLength(1);
    act(() => env.store.getState().toggleNode("sidebar"));
    expect(container.querySelector('[data-node-id="sidebar"]')).toBeInTheDocument();
  });

  it("gives the flexible track to the child that renders, not to an empty banner", () => {
    const env = createEnv();
    const layout = normalizeLayout({
      windows: {
        main: [
          {
            root: {
              type: "stack",
              id: "reader",
              sizes: ["auto", "auto", "1fr"],
              children: [
                { type: "widget", kind: "text", options: { text: "Bar" } },
                // Renders nothing while no junk message is selected.
                { type: "widget", kind: "junk-verdict" },
                { type: "widget", kind: "text", options: { text: "Body" } },
              ],
            },
          },
        ],
      },
    });
    const { container } = renderWithProviders(<WindowView layout={layout} />, env, { layout });
    const stack = container.querySelector('[data-node-id="reader"]') as HTMLElement;
    expect(container.querySelector('[data-part="junk-verdict"]')).toBeNull();
    // Two rendered children, two tracks: the body keeps the 1fr row.
    expect(geometryOf(stack)).toContain("grid-template-rows: auto minmax(0, 1fr);");
    // The slot markers are hidden, so they are no grid items.
    const markers = stack.querySelectorAll(':scope > [data-part="stack.slot"]');
    expect(markers).toHaveLength(3);
    for (const marker of markers) expect(marker).toHaveAttribute("hidden");
  });

  it("renders unknown widgets and missing windows with a message", () => {
    const env = createEnv();
    const layout = normalizeLayout({
      windows: {
        main: [
          {
            root: {
              type: "stack",
              children: [
                { type: "widget", kind: "plugin.x.y" },
                { type: "widget", kind: "text", options: { text: "Hi there" } },
              ],
            },
          },
        ],
      },
    });
    const { container } = renderWithProviders(
      <>
        <WindowView layout={layout} />
        <WindowView layout={layout} name="composer" />
      </>,
      env,
      { layout },
    );
    expect(container.querySelector('[data-part="layout.unknown-widget"]')).toHaveTextContent(
      "Unknown widget “plugin.x.y”.",
    );
    expect(
      within(container.querySelector('[data-part="text"]') as HTMLElement).getByText("Hi there"),
    ).toBeInTheDocument();
    expect(container.querySelector('[data-part="layout.missing"]')).toHaveTextContent("composer");
    expect(styleText()).toContain("grid-template-rows: minmax(0, 1fr) auto");
  });
});
