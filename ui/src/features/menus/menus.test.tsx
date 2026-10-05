import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { JsonValue, MenuItem, ResolvedLayout } from "../../api/generated";
import { createEnv, renderWithProviders, stubElementSizes, widget } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { ContextMenuHost } from "./ContextMenuHost";
import { ariaShortcut, chordText, keyLabel } from "./keyLabels";
import { contextTarget, menuStoreFor } from "./store";
import { findContextTarget, isTextInput } from "./targets";
import { contextMenuName, keyContextOf, resolveMenu } from "./resolve";
import { normalizeLayout } from "../../api/normalize";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { must } from "../../test/must";

const INBOX = "acc-personal/Inbox";
const FIELD = "field";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
});

const action = (id: string, args: JsonValue[] = []): MenuItem => ({
  type: "action",
  action: id,
  args,
  label: null,
  icon: null,
});

function layoutWith(base: ResolvedLayout): ResolvedLayout {
  return {
    ...base,
    keys: [
      ...base.keys,
      { context: "global", key: "shift+f10", action: "menu.context", args: [] },
      { context: "global", key: "contextmenu", action: "menu.context", args: [] },
    ],
    menus: {
      ...base.menus,
      "context:folder-tree.folder": [
        action("sync.now"),
        { type: "separator" },
        {
          type: "submenu",
          label: "@context-menu.action.open",
          items: [action("mail.compose"), action("view.toggle", ["reader"])],
        },
      ],
    },
  };
}

function renderMail() {
  const env = createEnv({
    state: { activeAccountId: "acc-personal", activeFolderId: INBOX },
  });
  const layout = layoutWith(env.transport.data.layout);
  const view = renderWithProviders(
    <>
      <NodeView node={widget("folder-tree")} path="t/0" />
      <NodeView node={widget("message-list")} path="t/1" />
      <input aria-label={FIELD} />
      <ContextMenuHost />
    </>,
    env,
    { layout },
  );
  return { env, layout, ...view };
}

async function firstRow(container: HTMLElement): Promise<HTMLElement> {
  await waitFor(() => {
    expect(container.querySelector('[data-part="message-list.row"]')).not.toBeNull();
  });
  return must(container.querySelector<HTMLElement>('[data-part="message-list.row"]'));
}

describe("key labels", () => {
  const t = (id: string, args?: Record<string, string | number>) =>
    `${id.replace("context-menu-key-", "")}${args ? `(${String(args.keys)})` : ""}`;

  it("formats chords, sequences and aria-keyshortcuts", () => {
    expect(chordText("ctrl+shift+m", t)).toBe("ctrl+shift+M");
    expect(chordText("ctrl++", t)).toBe("ctrl++");
    expect(keyLabel("g r", t)).toBe("sequence(G R)");
    expect(keyLabel("arrowdown", t)).toBe("arrowdown");
    expect(keyLabel("f5", t)).toBe("F5");
    expect(ariaShortcut("ctrl+shift+m")).toBe("Control+Shift+M");
    expect(ariaShortcut("delete")).toBe("Delete");
    expect(ariaShortcut("g r")).toBeUndefined();
  });
});

describe("context targets", () => {
  it("maps DOM parts to menu parts and leaves text fields alone", () => {
    document.body.innerHTML = `
      <div data-part="message-list" aria-activedescendant="r1" tabindex="0">
        <div id="r1" data-part="message-list.row" data-message="m1" data-thread="t1" data-account="a"><span id="in">x</span></div>
      </div>
      <ul><li id="f" data-part="folder-tree.item" data-folder="f1"></li></ul>
      <input id="text" /><input id="box" type="checkbox" data-part="message-list.row" />`;
    const list = document.querySelector('[data-part="message-list"]');
    expect(findContextTarget(list)).toMatchObject({
      part: "message-list.row",
      messageId: "m1",
      threadId: "t1",
      accountId: "a",
    });
    expect(findContextTarget(document.getElementById("in"))?.messageId).toBe("m1");
    expect(findContextTarget(document.getElementById("f"))).toMatchObject({
      part: "folder-tree.folder",
      folderId: "f1",
    });
    expect(findContextTarget(document.getElementById("text"))).toBeNull();
    expect(isTextInput(document.getElementById("box"))).toBe(false);
    expect(findContextTarget(document.body)).toBeNull();
    document.body.innerHTML = "";
  });

  it("gives a Design B child row and a reader card their menus (P2)", () => {
    document.body.innerHTML = `
      <div data-part="message-list.child" id="c" data-message="m2" data-thread="t1" data-folder="f1" data-account="a"></div>
      <article data-part="thread-view.card" data-message="m3"><span id="body">x</span></article>`;
    expect(findContextTarget(document.getElementById("c"))).toMatchObject({
      part: "message-list.row",
      messageId: "m2",
      threadId: "t1",
      folderId: "f1",
    });
    expect(findContextTarget(document.getElementById("body"))).toMatchObject({
      part: "thread-view.message",
      messageId: "m3",
    });
    document.body.innerHTML = "";
  });

  it("gives the navigation rail its View menu (M5d)", () => {
    document.body.innerHTML = `
      <nav data-part="nav-rail"><button id="mail" data-part="nav-rail.item">Mail</button></nav>`;
    expect(findContextTarget(document.getElementById("mail"))?.part).toBe("nav-rail");
    expect(keyContextOf("nav-rail")).toBe("global");
    const layout = normalizeLayout(defaultLayoutJson);
    const menu = must(resolveMenu(layout, contextMenuName("nav-rail"), "global"));
    expect(menu.items.map((e) => (e.type === "action" ? e.action : e.type))).toEqual([
      "header",
      "layout.customize",
      "theme.edit",
      // M5d review (ADR 0138): the way back from an unreadable theme.
      "theme.reset",
      "separator",
      "palette.open",
    ]);
    document.body.innerHTML = "";
  });
});

describe("context menu", () => {
  it("opens on right-click, selects the row and runs an action", async () => {
    const user = userEvent.setup();
    const { env, container } = renderMail();
    const row = await firstRow(container);
    const id = must(row.getAttribute("data-message"));
    fireEvent.contextMenu(row, { clientX: 40, clientY: 50, button: 2 });
    const menu = await screen.findByRole("menu", { name: "Message actions" });
    expect(env.store.getState().selectedMessageId).toBe(id);
    const items = within(menu).getAllByRole("menuitem");
    expect(items[0]).toHaveTextContent("Reply");
    expect(items[0]).toHaveFocus();
    expect(within(menu).getAllByRole("separator").length).toBeGreaterThan(0);
    // Positioned through the theme's stylesheet, not inline.
    expect(menu.closest('[data-part="context-menu"]')).not.toHaveAttribute("style");

    await user.keyboard("{End}");
    expect(items[items.length - 1]).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(items[0]).toHaveFocus();
    // Typeahead: the next item starting with "t" (Toggle read…).
    await user.keyboard("t");
    const flag = within(menu).getByRole("menuitem", { name: /flag/i });
    while (document.activeElement !== flag) await user.keyboard("{ArrowDown}");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(screen.queryByRole("menu")).toBeNull();
    });
    expect(env.transport.requests).toContainEqual(
      expect.objectContaining({
        type: "ApplyMailAction",
        targets: [{ kind: "message", message_id: id }],
      }),
    );
    // Focus went back to the list (the focusable around the row).
    expect(container.querySelector('[data-part="message-list"]')).toHaveFocus();
  });

  it("opens from the keyboard (Shift+F10) and closes with Escape", async () => {
    const user = userEvent.setup();
    const { env, container } = renderMail();
    await firstRow(container);
    const list = must(container.querySelector<HTMLElement>('[data-part="message-list"]'));
    act(() => {
      env.store.getState().selectMessage(env.store.getState().messageOrder[0] ?? null);
    });
    list.focus();
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu");
    expect(within(menu).getAllByRole("menuitem")[0]).toHaveFocus();
    // Keys meant for the menu never reach the app's bindings ("j" = next message).
    const before = env.store.getState().selectedMessageId;
    await user.keyboard("j");
    expect(env.store.getState().selectedMessageId).toBe(before);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(list).toHaveFocus();
  });

  it("names the folder of a folder menu and navigates submenus", async () => {
    const user = userEvent.setup();
    const { env } = renderMail();
    const folder = await screen.findByRole("treeitem", { name: /Archive/ });
    const folderId = folder.getAttribute("data-folder");
    fireEvent.contextMenu(folder, { clientX: 5, clientY: 5, button: 2 });
    const menu = await screen.findByRole("menu", { name: "Folder actions" });
    expect(contextTarget(env.store)?.folderId).toBe(folderId);
    // Right-clicking a folder does not switch folders.
    expect(env.store.getState().activeFolderId).toBe(INBOX);
    const sub = within(menu).getByRole("menuitem", { name: "Open context menu" });
    expect(sub).toHaveAttribute("aria-haspopup", "menu");
    sub.focus();
    await user.keyboard("{ArrowRight}");
    const submenu = await screen.findAllByRole("menu");
    expect(submenu).toHaveLength(2);
    expect(sub).toHaveAttribute("aria-expanded", "true");
    expect(within(must(submenu[1])).getAllByRole("menuitem")[0]).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(screen.getAllByRole("menu")).toHaveLength(1);
    expect(sub).toHaveFocus();
    await user.keyboard("{Tab}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(contextTarget(env.store)).toBeNull();
  });

  it("keeps the browser menu in text fields and closes on outside clicks", async () => {
    const { env, container } = renderMail();
    const row = await firstRow(container);
    const field = screen.getByRole("textbox", { name: "field" });
    const event = new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2 });
    field.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(screen.queryByRole("menu")).toBeNull();

    fireEvent.contextMenu(row, { clientX: 1, clientY: 1, button: 2 });
    await screen.findByRole("menu");
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
    expect(menuStoreFor(env.store).getState().open).toBeNull();
  });

  it("hides the actions that do not apply (M5b review) and opens nothing when none does", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const base = env.transport.data.layout;
    const menuOf = (items: ResolvedLayout["menus"][string]): ResolvedLayout => ({
      ...base,
      menus: { "context:folder-tree.folder": items },
    });
    const { unmount } = renderWithProviders(
      <>
        <NodeView node={widget("folder-tree")} path="t/0" />
        <ContextMenuHost />
      </>,
      env,
      {
        layout: menuOf([action("window.close"), { type: "separator" }, action("mail.compose")]),
      },
    );
    const folder = await screen.findByRole("treeitem", { name: /Archive/ });
    fireEvent.contextMenu(folder, { clientX: 5, clientY: 5, button: 2 });
    const menu = await screen.findByRole("menu");
    expect(within(menu).queryByRole("menuitem", { name: /close/i })).toBeNull();
    expect(within(menu).getAllByRole("menuitem")).toHaveLength(1);
    // The separator that would start the menu is gone too.
    expect(menu.querySelector('[data-part="context-menu.separator"]')).toBeNull();
    unmount();

    renderWithProviders(
      <>
        <NodeView node={widget("folder-tree")} path="t/0" />
        <ContextMenuHost />
      </>,
      env,
      { layout: menuOf([action("window.close")]) },
    );
    const again = await screen.findByRole("treeitem", { name: /Archive/ });
    fireEvent.contextMenu(again, { clientX: 5, clientY: 5, button: 2 });
    await waitFor(() => {
      expect(menuStoreFor(env.store).getState().open).toBeNull();
    });
    expect(screen.queryByRole("menu")).toBeNull();
  });
});
