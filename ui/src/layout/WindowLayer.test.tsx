/**
 * The window manager (M5c WP171, ADR 0116): moving by the title bar,
 * resizing from the handles, maximize / restore, snapping, keyboard move /
 * resize, focus and z-order, staying on the page, remembered geometry,
 * modal and non-modal windows, accessibility.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";
import type { ActionContext } from "../actions/registry";
import type { WindowState } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { BUILTIN_THEMES } from "../features/appearance";
import { resolveMockTheme } from "../features/appearance/themes";
import { runWindowCommand } from "../features/windows/commands";
import { SAVE_DELAY_MS } from "../features/windows/geometry";
import { WINDOWS_SKIN_CSS, actions as windowActions } from "../features/windows";
import { violations } from "../test/axe";
import { createEnv, renderWithProviders, type Env } from "../test/harness";
import type { WindowRegistry } from "../state/windows";
import { WINDOW_CASCADE_OFFSET, WINDOW_KEYBOARD_STEP, WINDOW_MIN_VISIBLE } from "../state/windows";
import { DialogRegistryContext, type DialogProps } from "./dialogRegistry";
import { MAIN_OWNER, WindowLayer, modalOwner, openingOrder } from "./WindowLayer";
import { WindowSpecsContext } from "./windowSpecs";
import { must } from "../test/must";

/** A dialog with something to focus. */
function Probe({ name, close }: DialogProps) {
  return (
    <div data-part="text">
      <label data-part="form.label">
        {name}
        <input data-part="form.input" />
      </label>
      <button type="button" data-part="form.button" onClick={close}>
        {name}
      </button>
    </div>
  );
}

const DIALOGS = {
  "plugin-manager": Probe,
  "config-errors": Probe,
  "gmail-label-delete": Probe,
};

const SPECS: WindowRegistry = {
  "plugin-manager": { size: { width: 600, height: 400 }, min_size: { width: 400, height: 300 } },
  "config-errors": { size: { width: 500, height: 300 }, resizable: false },
  "gmail-label-delete": { confirmation: true },
};

const PAGE = { width: 1280, height: 800 };

function setPage(width: number, height: number): void {
  Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
  Object.defineProperty(window, "innerHeight", { configurable: true, value: height });
  act(() => {
    window.dispatchEvent(new Event("resize"));
  });
}

// The window manager's skin as the compiled theme will hold it (jsdom applies
// `display`, which decides what Testing Library can see).
let skin: HTMLStyleElement | null = null;
beforeEach(() => {
  setPage(PAGE.width, PAGE.height);
  skin = document.createElement("style");
  skin.textContent = WINDOWS_SKIN_CSS;
  document.head.append(skin);
});
afterEach(() => {
  setPage(1024, 768);
  skin?.remove();
});

/** The declarations of an element's geometry rule. */
function geometry(element: Element): Record<string, string> {
  const id = element.getAttribute("data-geom") ?? "";
  const rule = [
    ...(document.head.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet?.cssRules ??
      []),
  ].find(
    (r): r is CSSStyleRule => r instanceof CSSStyleRule && r.selectorText === `[data-geom="${id}"]`,
  );
  const out: Record<string, string> = {};
  if (!rule) return out;
  for (let i = 0; i < rule.style.length; i++) {
    const property = rule.style.item(i);
    out[property] = rule.style.getPropertyValue(property);
  }
  return out;
}

/** left / top / width / height of a window, in px. */
function rectOf(element: Element) {
  const g = geometry(element);
  return {
    x: parseFloat(g.left ?? "NaN"),
    y: parseFloat(g.top ?? "NaN"),
    width: parseFloat(g.width ?? "NaN"),
    height: parseFloat(g.height ?? "NaN"),
  };
}

/** The window's live region (modes, maximize, snapping). */
const statusOf = (el: HTMLElement) =>
  must(el.querySelector<HTMLElement>('[data-part="window.status"]'));

function frame(id: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(`[data-part="window"][data-window="${id}"]`);
  if (!el) throw new Error(`window ${id} is not shown`);
  return el;
}

const header = (id: string) =>
  must(frame(id).querySelector<HTMLElement>('[data-part="window.header"]'));
const handle = (id: string, edge: string) =>
  must(frame(id).querySelector<HTMLElement>(`[data-part="window.handle"][data-edge="${edge}"]`));

function Wrap({ children }: { children: ReactNode }) {
  return (
    <DialogRegistryContext.Provider value={DIALOGS}>
      <WindowSpecsContext.Provider value={SPECS}>{children}</WindowSpecsContext.Provider>
    </DialogRegistryContext.Provider>
  );
}

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
    expect(env.queryClient.isMutating()).toBe(0);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function setup(open: string[], options: { theme?: ReturnType<typeof defaultTheme> } = {}) {
  const env = createEnv();
  for (const id of open) env.store.getState().openWindow(id);
  const view = renderWithProviders(
    <Wrap>
      <WindowLayer layout={env.transport.data.layout} />
    </Wrap>,
    env,
    options.theme ? { theme: options.theme } : {},
  );
  await settle(env);
  return { env, ...view };
}

async function remembered(env: Env): Promise<Record<string, WindowState>> {
  const answer = await env.client.call({ type: "GetWindowStates" });
  return answer.type === "WindowStates" ? answer.states : {};
}

/** What an action handler gets. */
function actionContext(env: Env): ActionContext {
  return {
    store: env.store,
    client: env.client,
    queryClient: env.queryClient,
    layout: env.transport.data.layout,
    t: (id) => id,
    locale: "en",
  };
}

const drag = (el: HTMLElement, from: [number, number], to: [number, number], up = true) => {
  fireEvent.pointerDown(el, { pointerId: 1, button: 0, clientX: from[0], clientY: from[1] });
  fireEvent.pointerMove(el, { pointerId: 1, clientX: to[0], clientY: to[1] });
  if (up) fireEvent.pointerUp(el, { pointerId: 1, clientX: to[0], clientY: to[1] });
};

describe("window manager (M5c WP171)", () => {
  it("keeps the opening order of windows whatever their z-order", () => {
    const first = openingOrder([], ["a", "b"]);
    expect(first).toEqual(["a", "b"]);
    expect(openingOrder(first, ["b", "a"])).toBe(first);
    expect(openingOrder(first, ["b", "c"])).toEqual(["b", "c"]);
    expect(openingOrder(["a", "b", "c"], ["c", "a"])).toEqual(["a", "c"]);
  });

  it("renders non-modal windows as labelled dialogs that do not block the page", async () => {
    const { container } = await setup(["plugin-manager"]);
    const dialog = screen.getByRole("dialog", { name: "Plug-ins" });
    expect(dialog).not.toHaveAttribute("aria-modal");
    expect(dialog).not.toHaveAttribute("data-modal");
    expect(dialog).toHaveAttribute("data-state", "active");
    expect(container.querySelector('[data-part="window-scrim"]')).toBeNull();
    expect(container.querySelector('[data-part="window-layer"]')).not.toHaveAttribute("data-modal");
    // Centred at the registered size.
    expect(rectOf(dialog)).toEqual({ x: 340, y: 200, width: 600, height: 400 });
    // The size is pinned (a theme width cannot squeeze it).
    expect(geometry(dialog)).toMatchObject({ "min-width": "600px", "max-width": "600px" });
    // Focus starts in the window's content.
    expect(within(dialog).getByRole("textbox")).toHaveFocus();
    expect(dialog.querySelectorAll('[data-part="window.handle"]')).toHaveLength(8);
    expect(within(dialog).getByRole("button", { name: "Maximize" })).toBeInTheDocument();
  });

  it("moves a window by its title bar and keeps it on the page", async () => {
    await setup(["plugin-manager"]);
    drag(header("plugin-manager"), [500, 210], [600, 260]);
    expect(rectOf(frame("plugin-manager"))).toMatchObject({ x: 440, y: 250 });
    // Dragged far off the top: the title bar stays below the top edge.
    drag(header("plugin-manager"), [600, 260], [600, -3000]);
    expect(rectOf(frame("plugin-manager"))).toMatchObject({ x: 440, y: 0 });
    // Far off the bottom: WINDOW_MIN_VISIBLE px stay on the page.
    drag(header("plugin-manager"), [600, 10], [600, 5000]);
    expect(rectOf(frame("plugin-manager"))).toMatchObject({
      x: 440,
      y: PAGE.height - WINDOW_MIN_VISIBLE,
    });
    // A press on a title-bar button does not start a move.
    const before = rectOf(frame("plugin-manager"));
    const maximize = within(frame("plugin-manager")).getByRole("button", { name: "Maximize" });
    fireEvent.pointerDown(maximize, { pointerId: 2, button: 0, clientX: 10, clientY: 10 });
    fireEvent.pointerMove(header("plugin-manager"), { pointerId: 2, clientX: 400, clientY: 400 });
    expect(rectOf(frame("plugin-manager"))).toEqual(before);
  });

  it("ignores a click without movement and moves after the pointer is released no more", async () => {
    await setup(["plugin-manager"]);
    const before = rectOf(frame("plugin-manager"));
    drag(header("plugin-manager"), [500, 210], [501, 211]);
    expect(rectOf(frame("plugin-manager"))).toEqual(before);
    fireEvent.pointerMove(header("plugin-manager"), { pointerId: 1, clientX: 900, clientY: 600 });
    expect(rectOf(frame("plugin-manager"))).toEqual(before);
  });

  it("resizes from the handles, not below the minimum size", async () => {
    await setup(["plugin-manager"]);
    drag(handle("plugin-manager", "se"), [940, 600], [1000, 650]);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 340, y: 200, width: 660, height: 450 });
    drag(handle("plugin-manager", "nw"), [340, 200], [2000, 2000]);
    // Minimum 400 × 300; the bottom-right corner stays where it was.
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 600, y: 350, width: 400, height: 300 });
    drag(handle("plugin-manager", "w"), [600, 400], [500, 400]);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 500, y: 350, width: 500, height: 300 });
    // A cancelled resize goes back.
    fireEvent.pointerDown(handle("plugin-manager", "n"), {
      pointerId: 3,
      button: 0,
      clientX: 600,
      clientY: 350,
    });
    fireEvent.pointerMove(handle("plugin-manager", "n"), {
      pointerId: 3,
      clientX: 600,
      clientY: 100,
    });
    expect(rectOf(frame("plugin-manager")).height).toBe(550);
    fireEvent.pointerCancel(handle("plugin-manager", "n"), { pointerId: 3 });
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 500, y: 350, width: 500, height: 300 });
  });

  it("maximizes and restores with the button and a double-click on the title bar", async () => {
    const { env } = await setup(["plugin-manager"]);
    const dialog = frame("plugin-manager");
    await userEvent.setup().click(within(dialog).getByRole("button", { name: "Maximize" }));
    expect(dialog).toHaveAttribute("data-state", "active maximized");
    expect(rectOf(dialog)).toEqual({ x: 0, y: 0, ...PAGE });
    expect(dialog.querySelectorAll('[data-part="window.handle"]')).toHaveLength(0);
    expect(statusOf(dialog)).toHaveTextContent("Window maximized");
    fireEvent.click(within(dialog).getByRole("button", { name: "Restore" }));
    expect(rectOf(dialog)).toEqual({ x: 340, y: 200, width: 600, height: 400 });
    fireEvent.doubleClick(header("plugin-manager"));
    expect(rectOf(dialog)).toEqual({ x: 0, y: 0, ...PAGE });
    fireEvent.doubleClick(header("plugin-manager"));
    expect(rectOf(dialog)).toEqual({ x: 340, y: 200, width: 600, height: 400 });
    expect(statusOf(dialog)).toHaveTextContent("Window restored");
    // The command (action `window.maximize`) does the same for the active window.
    act(() => {
      expect(runWindowCommand(env.store, { type: "maximize" })).toBe(true);
    });
    expect(dialog).toHaveAttribute("data-state", "active maximized");
  });

  it("snaps to a page half when dropped at the edge, with a preview while dragging", async () => {
    const { container } = await setup(["plugin-manager"]);
    const dialog = frame("plugin-manager");
    drag(header("plugin-manager"), [500, 210], [5, 300], false);
    const preview = container.querySelector('[data-part="window-snap-preview"]');
    expect(preview).toHaveAttribute("data-snap", "left");
    expect(preview).toHaveAttribute("aria-hidden", "true");
    expect(dialog).toHaveAttribute("data-dragging", "move");
    fireEvent.pointerUp(header("plugin-manager"), { pointerId: 1, clientX: 5, clientY: 300 });
    expect(container.querySelector('[data-part="window-snap-preview"]')).toBeNull();
    expect(dialog).toHaveAttribute("data-snap", "left");
    expect(dialog).toHaveAttribute("data-state", "active snapped");
    expect(dialog).not.toHaveAttribute("data-dragging");
    expect(rectOf(dialog)).toEqual({ x: 0, y: 0, width: 640, height: 800 });
    expect(statusOf(dialog)).toHaveTextContent("Left half");

    // Dragged away it comes loose at its old size under the pointer.
    drag(header("plugin-manager"), [320, 10], [700, 300]);
    expect(dialog).not.toHaveAttribute("data-snap");
    expect(rectOf(dialog)).toEqual({ x: 400, y: 290, width: 600, height: 400 });

    drag(header("plugin-manager"), [700, 300], [1279, 300]);
    expect(dialog).toHaveAttribute("data-snap", "right");
    expect(rectOf(dialog)).toEqual({ x: 640, y: 0, width: 640, height: 800 });
  });

  it("moves with the arrow keys and resizes with Shift+arrow keys on the focused title bar", async () => {
    await setup(["plugin-manager"]);
    const bar = header("plugin-manager");
    expect(bar).toHaveAttribute("role", "group");
    expect(bar).toHaveAccessibleName("Title bar of Plug-ins");
    expect(bar).toHaveAccessibleDescription(/Arrow keys move the window/);
    bar.focus();
    const step = WINDOW_KEYBOARD_STEP;
    fireEvent.keyDown(bar, { key: "ArrowRight" });
    fireEvent.keyDown(bar, { key: "ArrowDown" });
    expect(rectOf(frame("plugin-manager"))).toEqual({
      x: 340 + step,
      y: 200 + step,
      width: 600,
      height: 400,
    });
    fireEvent.keyDown(bar, { key: "ArrowRight", shiftKey: true });
    fireEvent.keyDown(bar, { key: "ArrowUp", shiftKey: true });
    expect(rectOf(frame("plugin-manager"))).toEqual({
      x: 340 + step,
      y: 200 + step,
      width: 600 + step,
      height: 400 - step,
    });
    // Keys on the title-bar buttons are theirs.
    const close = within(frame("plugin-manager")).getByRole("button", { name: "Close" });
    fireEvent.keyDown(close, { key: "ArrowLeft" });
    expect(rectOf(frame("plugin-manager")).x).toBe(340 + step);
  });

  it("has Alt+F7 / Alt+F8 move and size modes (Enter keeps, Escape goes back)", async () => {
    const { env } = await setup(["plugin-manager"]);
    const input = within(frame("plugin-manager")).getByRole("textbox");
    input.focus();
    fireEvent.keyDown(input, { key: "F7", altKey: true });
    const bar = header("plugin-manager");
    expect(bar).toHaveFocus();
    expect(frame("plugin-manager")).toHaveAttribute("data-mode", "move");
    expect(statusOf(frame("plugin-manager"))).toHaveTextContent(/Moving the window/);
    fireEvent.keyDown(bar, { key: "ArrowLeft" });
    fireEvent.keyDown(bar, { key: "ArrowLeft" });
    expect(rectOf(frame("plugin-manager")).x).toBe(340 - 2 * WINDOW_KEYBOARD_STEP);
    // Escape goes back and does not close the window.
    fireEvent.keyDown(bar, { key: "Escape" });
    expect(rectOf(frame("plugin-manager")).x).toBe(340);
    expect(frame("plugin-manager")).not.toHaveAttribute("data-mode");
    expect(env.store.getState().windows).toEqual(["plugin-manager"]);

    fireEvent.keyDown(bar, { key: "F8", altKey: true });
    expect(frame("plugin-manager")).toHaveAttribute("data-mode", "size");
    fireEvent.keyDown(bar, { key: "ArrowRight" });
    fireEvent.keyDown(bar, { key: "Enter" });
    expect(frame("plugin-manager")).not.toHaveAttribute("data-mode");
    expect(rectOf(frame("plugin-manager")).width).toBe(600 + WINDOW_KEYBOARD_STEP);
  });

  it("keeps fixed-size windows fixed", async () => {
    await setup(["config-errors"]);
    const dialog = frame("config-errors");
    expect(dialog.querySelectorAll('[data-part="window.handle"]')).toHaveLength(0);
    expect(within(dialog).queryByRole("button", { name: "Maximize" })).toBeNull();
    expect(header("config-errors")).toHaveAccessibleDescription("Arrow keys move the window.");
    fireEvent.doubleClick(header("config-errors"));
    expect(dialog).not.toHaveAttribute("data-state", "active maximized");
    drag(header("config-errors"), [500, 260], [2, 300]);
    expect(dialog).not.toHaveAttribute("data-snap");
    fireEvent.keyDown(header("config-errors"), { key: "ArrowRight", shiftKey: true });
    expect(rectOf(dialog).width).toBe(500);
    fireEvent.keyDown(header("config-errors"), { key: "F8", altKey: true });
    expect(dialog).not.toHaveAttribute("data-mode");
  });

  it("raises a window when it is clicked or focused, without reordering the DOM", async () => {
    const { env, container } = await setup(["plugin-manager", "config-errors"]);
    const order = () =>
      [...container.querySelectorAll('[data-part="window"]')].map((w) =>
        w.getAttribute("data-window"),
      );
    expect(order()).toEqual(["plugin-manager", "config-errors"]);
    const z = (id: string) => Number(geometry(frame(id))["z-index"]);
    expect(z("config-errors")).toBeGreaterThan(z("plugin-manager"));
    expect(frame("config-errors")).toHaveAttribute("data-state", "active");
    expect(frame("plugin-manager")).not.toHaveAttribute("data-state");

    fireEvent.pointerDown(frame("plugin-manager"));
    expect(env.store.getState().windows).toEqual(["config-errors", "plugin-manager"]);
    expect(z("plugin-manager")).toBeGreaterThan(z("config-errors"));
    expect(order()).toEqual(["plugin-manager", "config-errors"]);

    act(() => {
      within(frame("config-errors")).getByRole("button", { name: "config-errors" }).focus();
    });
    expect(env.store.getState().windows).toEqual(["plugin-manager", "config-errors"]);
    // Both stay usable: non-modal windows do not block each other.
    expect(frame("plugin-manager")).not.toHaveAttribute("inert");
    expect(screen.getAllByRole("dialog")).toHaveLength(2);
  });

  it("cycles the windows with Ctrl+F6 and goes back to the main window with Ctrl+Shift+F6", async () => {
    const outside = document.createElement("button");
    outside.textContent = "main";
    document.body.append(outside);
    const { env } = await setup(["plugin-manager", "config-errors"]);
    act(() => {
      outside.focus();
    });
    // From the main window to the topmost window.
    fireEvent.keyDown(outside, { key: "F6", ctrlKey: true });
    expect(frame("config-errors").contains(document.activeElement)).toBe(true);
    // The bottom window comes up, so every window comes round.
    fireEvent.keyDown(must(document.activeElement), { key: "F6", ctrlKey: true });
    expect(env.store.getState().windows).toEqual(["config-errors", "plugin-manager"]);
    expect(frame("plugin-manager").contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(must(document.activeElement), { key: "F6", ctrlKey: true });
    expect(frame("config-errors").contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(must(document.activeElement), {
      key: "F6",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(outside).toHaveFocus();
    // The action does the same.
    await act(async () => {
      await windowActions["window.cycle"]?.run(actionContext(env), []);
    });
    expect(frame("config-errors").contains(document.activeElement)).toBe(true);
    outside.remove();
  });

  it("goes back and forth between one window and the main window", async () => {
    const outside = document.createElement("button");
    document.body.append(outside);
    await setup(["plugin-manager"]);
    act(() => {
      outside.focus();
    });
    fireEvent.keyDown(outside, { key: "F6", ctrlKey: true });
    expect(frame("plugin-manager").contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(must(document.activeElement), { key: "F6", ctrlKey: true });
    expect(outside).toHaveFocus();
    outside.remove();
  });

  it("closes the active window with Escape and keeps Tab inside it", async () => {
    const user = userEvent.setup();
    const { env } = await setup(["plugin-manager", "config-errors"]);
    const dialog = frame("config-errors");
    const input = within(dialog).getByRole("textbox");
    expect(input).toHaveFocus();
    await user.tab();
    expect(within(dialog).getByRole("button", { name: "config-errors" })).toHaveFocus();
    await user.tab();
    // Wraps around to the title bar.
    expect(header("config-errors")).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(env.store.getState().windows).toEqual(["plugin-manager"]);
  });

  it("keeps windows on the page when it shrinks and gives them back their place when it grows", async () => {
    await setup(["plugin-manager"]);
    drag(header("plugin-manager"), [500, 210], [1000, 510]);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 840, y: 500, width: 600, height: 400 });
    setPage(800, 500);
    expect(rectOf(frame("plugin-manager"))).toEqual({
      x: 800 - WINDOW_MIN_VISIBLE,
      y: 500 - WINDOW_MIN_VISIBLE,
      width: 600,
      height: 400,
    });
    setPage(500, 350);
    expect(rectOf(frame("plugin-manager"))).toMatchObject({ width: 500, height: 350 });
    setPage(PAGE.width, PAGE.height);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 840, y: 500, width: 600, height: 400 });
  });

  it("keeps a window that lay wholly on the page wholly on it when the page shrinks (P1)", async () => {
    await setup(["plugin-manager"]);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 340, y: 200, width: 600, height: 400 });
    // Narrower and lower: moved in from the right and bottom edges, the title bar
    // and its buttons stay on the page.
    setPage(800, 500);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 200, y: 100, width: 600, height: 400 });
    setPage(500, 350);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 0, y: 0, width: 500, height: 350 });
    setPage(PAGE.width, PAGE.height);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 340, y: 200, width: 600, height: 400 });
  });

  it("remembers geometry per kind (debounced) and restores it when the window opens again", async () => {
    const { env } = await setup(["plugin-manager"]);
    const bar = header("plugin-manager");
    for (let i = 0; i < 3; i++) fireEvent.keyDown(bar, { key: "ArrowRight" });
    // Not yet: key repeats coalesce.
    expect((await remembered(env))["plugin-manager"]).toBeUndefined();
    await waitFor(
      async () => {
        expect((await remembered(env))["plugin-manager"]?.page).toEqual({
          rect: { x: 340 + 3 * WINDOW_KEYBOARD_STEP, y: 200, width: 600, height: 400 },
          maximized: false,
          snap: null,
        });
      },
      { timeout: SAVE_DELAY_MS * 10 },
    );
    // Maximized, then closed before the delay: still remembered.
    fireEvent.doubleClick(header("plugin-manager"));
    act(() => {
      env.store.getState().closeWindow("plugin-manager");
    });
    await settle(env);
    expect((await remembered(env))["plugin-manager"]?.page?.maximized).toBe(true);

    act(() => {
      env.store.getState().openWindow("plugin-manager");
    });
    await settle(env);
    expect(frame("plugin-manager")).toHaveAttribute("data-state", "active maximized");
    fireEvent.click(within(frame("plugin-manager")).getByRole("button", { name: "Restore" }));
    expect(rectOf(frame("plugin-manager"))).toEqual({
      x: 340 + 3 * WINDOW_KEYBOARD_STEP,
      y: 200,
      width: 600,
      height: 400,
    });
    await settle(env);
  });

  it("restores remembered geometry that loads after the window opened, and cascades a second instance", async () => {
    const env = createEnv();
    await env.client.request(
      {
        type: "SetWindowState",
        window: "plugin-manager",
        state: {
          page: { rect: { x: 100, y: 120, width: 500, height: 350 }, maximized: false, snap: null },
          pop_out: null,
          popped_out: false,
        },
      },
      "WindowStates",
    );
    env.store.getState().openWindow("plugin-manager");
    env.store.getState().openWindow("plugin-manager:w2");
    renderWithProviders(
      <Wrap>
        <WindowLayer layout={env.transport.data.layout} />
      </Wrap>,
      env,
    );
    await settle(env);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 100, y: 120, width: 500, height: 350 });
    expect(rectOf(frame("plugin-manager:w2"))).toEqual({
      x: 100 + WINDOW_CASCADE_OFFSET,
      y: 120 + WINDOW_CASCADE_OFFSET,
      width: 500,
      height: 350,
    });
    expect(screen.getAllByRole("dialog", { name: "Plug-ins" })).toHaveLength(2);
  });

  it("puts open windows back in their default places when positions are reset", async () => {
    const { env } = await setup(["plugin-manager"]);
    drag(header("plugin-manager"), [500, 210], [700, 310]);
    await waitFor(
      async () => {
        expect((await remembered(env))["plugin-manager"]).toBeDefined();
      },
      { timeout: SAVE_DELAY_MS * 10 },
    );
    const reset = windowActions["window.reset-positions"];
    await act(async () => {
      await reset?.run(actionContext(env), []);
    });
    expect(await remembered(env)).toEqual({});
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 340, y: 200, width: 600, height: 400 });
    await settle(env);
  });

  it("a confirmation from the main window covers only the main window", async () => {
    const outside = document.createElement("button");
    document.body.append(outside);
    const { env, container } = await setup(["plugin-manager", "gmail-label-delete"]);
    const confirm = screen.getByRole("dialog", { name: "Delete label" });
    expect(confirm).toHaveAttribute("aria-modal", "true");
    expect(confirm).toHaveAttribute("data-modal", "");
    expect(confirm).toHaveAttribute("data-owner", "main");
    expect(confirm.querySelectorAll('[data-part="window.handle"]')).toHaveLength(0);
    const scrim = must(container.querySelector<HTMLElement>('[data-part="window-scrim"]'));
    expect(scrim).toHaveAttribute("aria-hidden", "true");
    expect(container.querySelector('[data-part="window-layer"]')).toHaveAttribute("data-modal", "");
    // Below every window: only the main window is covered.
    expect(Number(geometry(scrim)["z-index"])).toBeLessThan(
      Number(geometry(frame("plugin-manager"))["z-index"]),
    );
    expect(frame("plugin-manager")).not.toHaveAttribute("inert");
    // Focus that strays into the main window comes back; Ctrl+Shift+F6 does
    // not leave; Ctrl+F6 reaches the other (usable) window.
    act(() => {
      outside.focus();
    });
    expect(confirm.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(must(document.activeElement), {
      key: "F6",
      ctrlKey: true,
      shiftKey: true,
    });
    expect(confirm.contains(document.activeElement)).toBe(true);
    fireEvent.keyDown(must(document.activeElement), { key: "F6", ctrlKey: true });
    expect(frame("plugin-manager").contains(document.activeElement)).toBe(true);
    // Raised, it still stays below the confirmation.
    expect(env.store.getState().windows.at(-1)).toBe("plugin-manager");
    expect(Number(geometry(confirm)["z-index"])).toBeGreaterThan(
      Number(geometry(frame("plugin-manager"))["z-index"]),
    );
    outside.remove();
  });

  it("a confirmation of a window blocks only that window", async () => {
    const outside = document.createElement("button");
    document.body.append(outside);
    const { env, container } = await setup(["plugin-manager", "config-errors"]);
    // The user works in the plug-in manager when its confirmation opens.
    act(() => {
      within(frame("plugin-manager")).getByRole("textbox").focus();
    });
    act(() => {
      env.store.getState().openWindow("gmail-label-delete");
    });
    const confirm = await screen.findByRole("dialog", { name: "Delete label" });
    await waitFor(() => expect(confirm).toHaveAttribute("data-owner", "plugin-manager"));
    expect(confirm).not.toHaveAttribute("aria-modal");
    expect(frame("plugin-manager")).toHaveAttribute("inert");
    expect(frame("plugin-manager")).toHaveAttribute("data-blocked", "");
    expect(frame("config-errors")).not.toHaveAttribute("inert");
    expect(container.querySelector('[data-part="window-scrim"]')).toBeNull();
    // The main window stays usable.
    act(() => {
      outside.focus();
    });
    expect(document.activeElement).toBe(outside);
    // Closing the confirmation gives the window back.
    act(() => {
      env.store.getState().closeWindow("gmail-label-delete");
    });
    await waitFor(() => expect(frame("plugin-manager")).not.toHaveAttribute("inert"));
    outside.remove();
  });

  it("opens a window's confirmation over that window and stacks it just above it", async () => {
    const { env } = await setup(["plugin-manager", "config-errors"]);
    // The plug-in manager moved to the left (600 x 400 at 140 / 200).
    drag(header("plugin-manager"), [500, 210], [300, 210]);
    expect(rectOf(frame("plugin-manager"))).toEqual({ x: 140, y: 200, width: 600, height: 400 });
    act(() => {
      within(frame("plugin-manager")).getByRole("textbox").focus();
    });
    act(() => {
      env.store.getState().openWindow("gmail-label-delete");
    });
    const confirm = await screen.findByRole("dialog", { name: "Delete label" });
    await waitFor(() => expect(confirm).toHaveAttribute("data-owner", "plugin-manager"));
    // Centred over its owner (440 x 220), not over the page.
    await waitFor(() => {
      expect(rectOf(confirm)).toEqual({ x: 220, y: 290, width: 440, height: 220 });
    });
    const z = (el: Element) => Number(geometry(el)["z-index"]);
    expect(z(confirm)).toBeGreaterThan(z(frame("plugin-manager")));
    // Another window raised over the owner covers the confirmation too.
    act(() => {
      env.store.getState().focusWindow("config-errors");
    });
    expect(z(frame("config-errors"))).toBeGreaterThan(z(confirm));
    expect(z(confirm)).toBeGreaterThan(z(frame("plugin-manager")));
    // A confirmation of the main window stays above every window.
    act(() => {
      env.store.getState().closeWindow("gmail-label-delete");
    });
    act(() => {
      document.body.focus();
    });
    const outside = document.createElement("button");
    document.body.append(outside);
    act(() => {
      outside.focus();
    });
    act(() => {
      env.store.getState().openWindow("gmail-label-delete");
    });
    const pageModal = await screen.findByRole("dialog", { name: "Delete label" });
    await waitFor(() => expect(pageModal).toHaveAttribute("data-owner", MAIN_OWNER));
    expect(z(pageModal)).toBeGreaterThan(z(frame("config-errors")));
    expect(z(pageModal)).toBeGreaterThan(z(frame("plugin-manager")));
    outside.remove();
  });

  it("opens a new window in a cascade slot no open window of its kind uses", async () => {
    const { env } = await setup(["plugin-manager", "plugin-manager:w2"]);
    const first = { x: 340, y: 200, width: 600, height: 400 };
    const second = {
      ...first,
      x: first.x + WINDOW_CASCADE_OFFSET,
      y: first.y + WINDOW_CASCADE_OFFSET,
    };
    expect(rectOf(frame("plugin-manager"))).toEqual(first);
    expect(rectOf(frame("plugin-manager:w2"))).toEqual(second);
    // The first closes, a third opens: in the free slot, not over the second.
    act(() => {
      env.store.getState().closeWindow("plugin-manager");
    });
    act(() => {
      env.store.getState().openWindow("plugin-manager:w3");
    });
    await settle(env);
    expect(rectOf(frame("plugin-manager:w3"))).toEqual(first);
    expect(rectOf(frame("plugin-manager:w2"))).toEqual(second);
  });

  it("finds the flow a confirmation belongs to", () => {
    const modal = (id: string) => id.startsWith("composer-close") || id === "smartcard-pin";
    const open = ["composer:w1", "composer:w2", "composer-close:w1"];
    expect(modalOwner("composer-close:w1", open, modal, "composer:w2")).toBe("composer:w1");
    expect(modalOwner("smartcard-pin", [...open, "smartcard-pin"], modal, "composer:w2")).toBe(
      "composer:w2",
    );
    expect(modalOwner("smartcard-pin", ["smartcard-pin"], modal, null)).toBe(MAIN_OWNER);
    // A focused window that has closed meanwhile: the main window.
    expect(modalOwner("smartcard-pin", ["smartcard-pin"], modal, "composer:w9")).toBe(MAIN_OWNER);
  });

  it("does not show windows that are popped out into their own OS window", async () => {
    const env = createEnv();
    env.queryClient.setQueryData(
      ["popOuts"],
      [{ window: "config-errors", title: null, context: null, geometry: null }],
    );
    env.store.getState().openWindow("plugin-manager");
    env.store.getState().openWindow("config-errors");
    renderWithProviders(
      <Wrap>
        <WindowLayer layout={env.transport.data.layout} />
      </Wrap>,
      env,
    );
    await settle(env);
    expect(screen.getAllByRole("dialog").map((d) => d.getAttribute("data-window"))).toEqual([
      "plugin-manager",
    ]);
  });

  it.each(BUILTIN_THEMES.map(([id]) => id))("has no axe violations in %s", async (id) => {
    const theme = resolveMockTheme(defaultTheme(), id, { density: "comfortable", overrides: {} });
    const { container } = await setup(["plugin-manager", "config-errors", "gmail-label-delete"], {
      theme,
    });
    expect(await violations(container)).toEqual([]);
    act(() => {
      fireEvent.keyDown(header("gmail-label-delete"), { key: "F7", altKey: true });
    });
    drag(header("gmail-label-delete"), [600, 300], [5, 300], false);
    expect(await violations(container)).toEqual([]);
    fireEvent.pointerUp(header("gmail-label-delete"), { pointerId: 1, clientX: 5, clientY: 300 });
  });
});
