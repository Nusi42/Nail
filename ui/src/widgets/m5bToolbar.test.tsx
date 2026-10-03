/**
 * M5b WP162: the ranked reader action bar — labelled primary actions, icon
 * buttons with tooltips and shortcuts, a "More" menu with headings and
 * shortcuts, disabled actions hidden, and overflow into "More" instead of
 * wrapping (or, with `overflow="wrap"`, no separator starting a line).
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ResolvedLayout } from "../api/generated";
import { normalizeLayout } from "../api/normalize";
import { ContextMenuHost } from "../features/menus/ContextMenuHost";
import { NodeView } from "../layout/LayoutRenderer";
import {
  createEnv,
  renderNode,
  renderWithProviders,
  settleQueries,
  stubElementSizes,
  widget,
  type Env,
} from "../test/harness";
import { PROPOSAL_CONTEXT_MENUS, PROPOSAL_READER } from "./proposalToolbars";

const INBOX = "acc-personal/Inbox";
const FIRST = `${INBOX}#1`;

function readerLayout(
  env: Env,
  settings = { hide_disabled: true, overflow: "more" },
): ResolvedLayout {
  return normalizeLayout({
    ...env.transport.data.layout,
    toolbars: { ...env.transport.data.layout.toolbars, reader: PROPOSAL_READER },
    toolbar_settings: { reader: settings },
  });
}

function readerEnv(selected = true): Env {
  const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  if (selected) {
    env.store.getState().setMessageOrder([FIRST]);
    env.store.getState().selectMessage(FIRST);
  }
  return env;
}

/** Layout sizes for jsdom: button widths by kind, the bar's width. */
function stubWidths(bar: { width: number }) {
  const rect = Object.getOwnPropertyDescriptor(Element.prototype, "getBoundingClientRect");
  const client = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const el = this as HTMLElement;
    const part = el.dataset.part;
    let width = 0;
    if (part === "toolbar.button") width = el.dataset.display === "icon" ? 32 : 90;
    else if (part === "toolbar.more") width = 70;
    else if (part === "toolbar.separator") width = 9;
    return {
      width,
      height: 32,
      top: 0,
      left: 0,
      right: width,
      bottom: 32,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    };
  };
  Object.defineProperty(HTMLElement.prototype, "clientWidth", {
    configurable: true,
    get(this: HTMLElement) {
      return this.dataset.part === "toolbar" ? bar.width : 0;
    },
  });
  return () => {
    if (rect) Object.defineProperty(Element.prototype, "getBoundingClientRect", rect);
    if (client) Object.defineProperty(HTMLElement.prototype, "clientWidth", client);
  };
}

/** A ResizeObserver jsdom lacks; `resize()` notifies every observer. */
function fakeResizeObserver() {
  const callbacks = new Set<() => void>();
  class Fake {
    private readonly cb: () => void;
    constructor(cb: () => void) {
      this.cb = cb;
    }
    observe() {
      callbacks.add(this.cb);
    }
    disconnect() {
      callbacks.delete(this.cb);
    }
    unobserve() {}
  }
  vi.stubGlobal("ResizeObserver", Fake);
  return {
    resize: () =>
      act(() => {
        for (const cb of callbacks) cb();
      }),
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

function bar(): HTMLElement {
  return screen.getByRole("toolbar", { name: "Message actions" });
}

/** Names of the bar's buttons, in order. */
function names(): string[] {
  return within(bar())
    .getAllByRole("button")
    .map((b) => b.getAttribute("aria-label") ?? b.textContent ?? "");
}

describe("reader action bar (M5b)", () => {
  it("labels the primary actions, shows the common ones as icons with tooltips and shortcuts", () => {
    const env = readerEnv();
    renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
      layout: readerLayout(env),
    });
    expect(bar()).toHaveAttribute("data-overflow", "more");
    const reply = screen.getByRole("button", { name: "Reply" });
    expect(reply).toHaveAttribute("data-display", "icon-label");
    expect(reply).toHaveAttribute("data-rank", "1");
    expect(reply).toHaveAttribute("title", "Reply (R)");
    expect(reply).toHaveAttribute("aria-keyshortcuts", "R");
    expect(within(reply).getByText("Reply")).toHaveAttribute("data-part", "toolbar.button.label");
    const archive = screen.getByRole("button", { name: "Archive" });
    expect(archive).toHaveAttribute("data-display", "icon");
    expect(archive).toHaveAttribute("title", "Archive (E)");
    expect(archive.querySelector('[data-part="toolbar.button.label"]')).toBeNull();
    expect(archive.querySelector('[data-part="toolbar.button.icon"]')).toHaveAttribute(
      "data-icon",
      "archive",
    );
    expect(screen.getByRole("button", { name: "Delete" })).toHaveAttribute("title", "Delete (Del)");
    expect(names()).toEqual([
      "Reply",
      "Reply all",
      "Forward",
      "Archive",
      "Delete",
      "Move to…",
      "Flag",
      "Snooze…",
      "More",
    ]);
  });

  it("puts the rest into More, with headings, icons and shortcuts, and runs what is chosen", async () => {
    const env = readerEnv();
    renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
      layout: readerLayout(env),
    });
    const more = screen.getByRole("button", { name: "More" });
    expect(more).toHaveAttribute("aria-haspopup", "menu");
    expect(more).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(more);
    const menu = await screen.findByRole("menu", { name: "More" });
    expect(more).toHaveAttribute("aria-expanded", "true");
    const headings = menu.querySelectorAll('[data-part="context-menu.header"]');
    expect([...headings].map((h) => h.textContent)).toEqual(["Message", "Turn into"]);
    const items = within(menu).getAllByRole("menuitem");
    expect(
      items.map((i) => i.querySelector('[data-part="context-menu.item.label"]')?.textContent),
    ).toEqual([
      "Mark as read/unread",
      "Mark as junk",
      // Enabled for any selected message (checked when it runs).
      "Import Autocrypt setup message",
      "Print the message…",
      "Create event from email",
      "Create task from email",
    ]);
    // Disabled actions are hidden: Labels… only works for Gmail messages,
    // a rule needs the message's summary (the list is not loaded here).
    expect(within(menu).queryByRole("menuitem", { name: /Labels/ })).toBeNull();
    expect(within(menu).queryByRole("menuitem", { name: /rule/ })).toBeNull();
    const toggle = items[0] as HTMLElement;
    expect(toggle.querySelector('[data-part="context-menu.item.shortcut"]')).toHaveTextContent("U");
    expect(toggle.querySelector('[data-part="context-menu.item.icon"]')).not.toBeNull();
    fireEvent.click(toggle);
    await waitFor(() =>
      expect(env.transport.requests.some((r) => r.type === "UpdateFlags")).toBe(true),
    );
    expect(screen.queryByRole("menu")).toBeNull();
    expect(more).toHaveFocus();
  });

  it("opens More from the keyboard with the first entry focused, Escape returns", async () => {
    const env = readerEnv();
    renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
      layout: readerLayout(env),
    });
    const more = screen.getByRole("button", { name: "More" });
    act(() => more.focus());
    fireEvent.keyDown(more, { key: "ArrowDown" });
    const menu = await screen.findByRole("menu");
    await waitFor(() => expect(within(menu).getAllByRole("menuitem")[0]).toHaveFocus());
    fireEvent.keyDown(document.activeElement as Element, { key: "Escape" });
    expect(screen.queryByRole("menu")).toBeNull();
    expect(more).toHaveFocus();
    // The arrow keys move between the bar's buttons, More included.
    fireEvent.keyDown(more, { key: "Home" });
    expect(screen.getByRole("button", { name: "Reply" })).toHaveFocus();
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    expect(more).toHaveFocus();
  });

  it("hides actions that cannot run instead of greying them out", () => {
    const env = readerEnv(false);
    renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
      layout: readerLayout(env),
    });
    // Nothing selected: no message action can run, More has nothing either.
    expect(within(bar()).queryAllByRole("button")).toEqual([]);
    act(() => {
      env.store.getState().setMessageOrder([FIRST]);
      env.store.getState().selectMessage(FIRST);
    });
    expect(names()[0]).toBe("Reply");
    // Without hide-disabled they stay, disabled.
    const other = readerEnv(false);
    renderNode(widget("toolbar", { id: "other", options: { toolbar: "reader" } }), other, {
      layout: readerLayout(other, { hide_disabled: false, overflow: "more" }),
    });
    const bars = screen.getAllByRole("toolbar");
    const reply = within(bars[1] as HTMLElement).getByRole("button", { name: "Reply" });
    expect(reply).toBeDisabled();
  });

  it("never wraps: what does not fit moves into More in rank order, and comes back", async () => {
    const width = { width: 400 };
    const restore = stubWidths(width);
    const observer = fakeResizeObserver();
    try {
      const env = readerEnv();
      renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
        layout: readerLayout(env),
      });
      // 3 × 90 + 9 + 5 × 32 + 70 = 509 > 400: Snooze, Flag, Move and Delete
      // (highest rank first) move until 381 fits.
      expect(names()).toEqual(["Reply", "Reply all", "Forward", "Archive", "More"]);
      expect(bar().querySelectorAll('[data-part="toolbar.separator"]')).toHaveLength(1);
      fireEvent.click(screen.getByRole("button", { name: "More" }));
      const menu = await screen.findByRole("menu");
      const labels = within(menu)
        .getAllByRole("menuitem")
        .map((i) => i.querySelector('[data-part="context-menu.item.label"]')?.textContent);
      // Appended after the own entries, in bar order.
      expect(labels.slice(-4)).toEqual(["Delete", "Move to…", "Flag", "Snooze…"]);
      fireEvent.keyDown(within(menu).getAllByRole("menuitem")[0] as Element, { key: "Escape" });

      // Narrower: the labelled actions go too, separators never dangle.
      width.width = 180;
      observer.resize();
      expect(names()).toEqual(["Reply", "More"]);
      expect(bar().querySelectorAll('[data-part="toolbar.separator"]')).toHaveLength(0);

      // Wider again: everything is back in the bar.
      width.width = 1000;
      observer.resize();
      expect(names()).toEqual([
        "Reply",
        "Reply all",
        "Forward",
        "Archive",
        "Delete",
        "Move to…",
        "Flag",
        "Snooze…",
        "More",
      ]);
    } finally {
      restore();
    }
  });

  it("adds a More button of its own to a toolbar without one when it overflows", async () => {
    const width = { width: 200 };
    const restore = stubWidths(width);
    try {
      const env = createEnv({ state: { activeAccountId: "acc-work" } });
      const layout = normalizeLayout({
        ...env.transport.data.layout,
        toolbars: {
          main: [
            { type: "action", action: "sync.now" },
            { type: "separator" },
            { type: "action", action: "outbox.open" },
            { type: "action", action: "view.toggle", args: ["sidebar"], label: "Sidebar" },
          ],
        },
      });
      renderNode(widget("toolbar", { options: { toolbar: "main" } }), env, { layout });
      // 90 + 9 + 90 + 90 = 279 > 200; unranked items move from the end.
      expect(names()).toEqual(["Check for new mail", "More"]);
      fireEvent.click(screen.getByRole("button", { name: "More" }));
      const menu = await screen.findByRole("menu");
      fireEvent.click(within(menu).getByRole("menuitem", { name: /Sidebar/ }));
      expect(env.store.getState().hiddenNodes).toEqual(["sidebar"]);
    } finally {
      restore();
    }
  });

  it("with overflow=wrap, a separator never starts or ends a line", () => {
    const top = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetTop");
    // Items 0–2 on the first line, the separator (3) and the rest wrapped.
    Object.defineProperty(HTMLElement.prototype, "offsetTop", {
      configurable: true,
      get(this: HTMLElement) {
        return Number(this.dataset.item ?? 0) >= 3 ? 40 : 0;
      },
    });
    try {
      const env = readerEnv();
      renderNode(widget("toolbar", { options: { toolbar: "reader" } }), env, {
        layout: readerLayout(env, { hide_disabled: true, overflow: "wrap" }),
      });
      expect(bar()).toHaveAttribute("data-overflow", "wrap");
      const separator = bar().querySelector('[data-part="toolbar.separator"]');
      expect(separator).toHaveAttribute("data-wrap-edge", "start");
      // Nothing moves into More: it only holds its own entries.
      expect(names()).toContain("Snooze…");
    } finally {
      if (top) Object.defineProperty(HTMLElement.prototype, "offsetTop", top);
    }
  });
});

describe("the matching right-click menus (M5b)", () => {
  it("hold every action of the bar, in the bar's groups", () => {
    const barActions = PROPOSAL_READER.flatMap((item) =>
      item.type === "action"
        ? [item.action]
        : item.type === "more"
          ? (item.items ?? []).flatMap((m) => (m.type === "action" ? [m.action] : []))
          : [],
    );
    const menuActions = (name: keyof typeof PROPOSAL_CONTEXT_MENUS) =>
      PROPOSAL_CONTEXT_MENUS[name].flatMap((m) => ("action" in m ? [m.action] : []));
    for (const action of barActions) expect(menuActions("context:message-view")).toContain(action);
    // The row's menu has the same groups (no Autocrypt import for a row).
    const view = menuActions("context:message-view").filter(
      (a) => a !== "security.autocrypt.import" && a !== "print.save-pdf",
    );
    const row = menuActions("context:message-list.row").filter((a) => a !== "print.thread");
    expect(row).toEqual(view);
  });

  it("show headings and shortcuts on a right-click on a row", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const layout = normalizeLayout({
      ...env.transport.data.layout,
      menus: { ...env.transport.data.layout.menus, ...PROPOSAL_CONTEXT_MENUS },
    });
    const restore = stubElementSizes();
    try {
      renderWithProviders(
        <>
          <NodeView node={widget("message-list")} path="t/0" />
          <ContextMenuHost />
        </>,
        env,
        { layout },
      );
      const row = (await screen.findAllByRole("option"))[0] as HTMLElement;
      fireEvent.contextMenu(row, { clientX: 5, clientY: 5, button: 2 });
      const menu = await screen.findByRole("menu");
      const headings = [...menu.querySelectorAll('[data-part="context-menu.header"]')];
      expect(headings.map((h) => h.textContent)).toEqual(["Message", "Turn into"]);
      const archive = within(menu).getByRole("menuitem", { name: /Archive/ });
      expect(archive.querySelector('[data-part="context-menu.item.shortcut"]')).toHaveTextContent(
        "E",
      );
      expect(archive.querySelector('[data-part="context-menu.item.icon"]')).toHaveAttribute(
        "data-icon",
        "archive",
      );
      fireEvent.keyDown(archive, { key: "Escape" });
      await settleQueries(env);
    } finally {
      restore();
    }
  });
});
