/**
 * M5d WP182: the toolbar customizer in the UI — right-click a toolbar →
 * Customize toolbar opens its window; buttons move between the bar, "More"
 * and the available actions by drag and drop and by keyboard; plug-in
 * actions are offered; Undo / Redo / Reset go through the `layout` history;
 * a file changed on disk is a conflict with "Undo anyway". Every change is
 * an `UpdateLayout` patch the mock core applies and announces
 * (`ConfigChanged`), like the real core.
 */
import {
  act,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, useState, type ReactNode } from "react";
import type { LayoutPatch, ResolvedLayout, ToolbarItem } from "../../api/generated";
import { normalizeLayout } from "../../api/normalize";
import { simulateExternalEdit } from "../../api/editHistoryMock";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { NodeView } from "../../layout/LayoutRenderer";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { WindowLayer } from "../../layout/WindowLayer";
import { applyEditHistory } from "../../state/editHistory";
import { createEnv, Providers, settleQueries, widget, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { followUpManifest } from "../plugins/mockHost";
import { barItems, moreItems } from "./model";
import { PALETTE_LIMIT } from "./ToolbarCustomizer";
import { toolbarToCustomize } from "./index";
import { staleLayoutRevision } from "../../test/env";

/** The layout the core announced last, as the app keeps it. */
function Live({ env, children }: { env: Env; children: ReactNode }) {
  const [layout, setLayout] = useState<ResolvedLayout>(env.transport.data.layout);
  useEffect(
    () =>
      env.client.subscribe((event) => {
        if (event.type === "ConfigChanged") setLayout(normalizeLayout(event.layout));
        if (event.type === "EditHistoryChanged") applyEditHistory(env.queryClient, event.history);
      }),
    [env],
  );
  return (
    <Providers env={env} layout={layout}>
      {children}
    </Providers>
  );
}

function Windows() {
  return <WindowLayer layout={useLayoutConfig()} />;
}

/** An env with a selected message (the reader's bar hides disabled actions). */
function selectedEnv(): Env {
  const env = createEnv({
    state: { activeAccountId: "acc-personal", activeFolderId: "acc-personal/Inbox" },
  });
  env.store.getState().setMessageOrder(["acc-personal/Inbox#1"]);
  env.store.getState().selectMessage("acc-personal/Inbox#1");
  return env;
}

/** The reader's toolbar and the window layer, with the live layout. */
function renderReader(env: Env = selectedEnv()) {
  const node = widget("toolbar", { id: "reader-bar", options: { toolbar: "reader" } });
  const view = (
    <Live env={env}>
      <NodeView node={node} path="test" />
      <Windows />
    </Live>
  );
  return { env, ...renderWithLive(view) };
}

function renderWithLive(ui: ReactNode) {
  return render(<>{ui}</>);
}

function openCustomizer(env: Env, id = "toolbar-customizer:reader") {
  act(() => {
    env.store.getState().openWindow(id);
  });
}

const patches = (env: Env): LayoutPatch[] =>
  env.transport.data.features.customization.layoutPatches;
const reader = (env: Env): ToolbarItem[] => env.transport.data.layout.toolbars.reader ?? [];
const barIds = (env: Env) =>
  barItems(reader(env)).map((i) => (i.type === "action" ? i.action : i.type));
const moreIds = (env: Env) =>
  moreItems(reader(env)).map((i) => (i.type === "action" ? i.action : i.type));

/** The customizer window, once its lazily loaded content is there. */
async function customizer(): Promise<HTMLElement> {
  const dialog = await screen.findByRole("dialog", { name: "Customize toolbar" });
  await within(dialog).findAllByRole("listbox");
  return dialog;
}

function list(dialog: HTMLElement, name: string): HTMLElement {
  return within(dialog).getByRole("listbox", { name });
}

/**
 * The options of a list. Found by their `role` attribute, not by
 * `getByRole`: computing names and visibility of the ~200 available
 * actions makes role queries take seconds in jsdom.
 */
function options(listbox: HTMLElement): HTMLElement[] {
  return [...listbox.querySelectorAll<HTMLElement>('[role="option"]')];
}

/** The option whose label is `name` (a RegExp tests its whole text). */
function option(dialog: HTMLElement, listName: string, name: string | RegExp): HTMLElement {
  const found = options(list(dialog, listName)).filter((o) => {
    const label = o.querySelector('[data-part="toolbar-customizer.item.label"]')?.textContent ?? "";
    return typeof name === "string" ? label === name : name.test(o.textContent);
  });
  if (found.length !== 1) throw new Error(`${String(found.length)} options ${String(name)}`);
  return must(found[0]);
}

/** Types into the available actions' search field. */
function search(dialog: HTMLElement, text: string) {
  fireEvent.change(within(dialog).getByRole("searchbox", { name: "Find an action" }), {
    target: { value: text },
  });
}

/** A DataTransfer for jsdom's drag events. */
function transfer() {
  const data = new Map<string, string>();
  return {
    dropEffect: "none",
    effectAllowed: "all",
    types: [] as string[],
    setData: (type: string, value: string) => {
      data.set(type, value);
    },
    getData: (type: string) => data.get(type) ?? "",
  };
}

/** A drag event at a pointer position (jsdom's drag events have none). */
function dragEvent(
  kind: "dragOver" | "drop",
  target: HTMLElement,
  dataTransfer: ReturnType<typeof transfer>,
  at: { clientX?: number; clientY?: number },
) {
  const event = createEvent[kind](target, { dataTransfer });
  for (const [key, value] of Object.entries(at)) Object.defineProperty(event, key, { value });
  return fireEvent(target, event);
}

function drag(
  source: HTMLElement,
  target: HTMLElement,
  at: { clientX?: number; clientY?: number } = {},
) {
  const dataTransfer = transfer();
  fireEvent.dragStart(source, { dataTransfer });
  dragEvent("dragOver", target, dataTransfer, at);
  dragEvent("drop", target, dataTransfer, at);
  fireEvent.dragEnd(source, { dataTransfer });
}

/** The reader toolbar's Reply button. */
async function readerReply() {
  const user = userEvent.setup();
  const { env } = renderReader();
  const bar = await screen.findByRole("toolbar", { name: "Message actions" });
  const reply = within(bar).getByRole("button", { name: "Reply" });
  return { user, env, bar, reply };
}

describe("toolbar context menu (M5d)", () => {
  it("opens on a right-click and customizes that toolbar", async () => {
    const { user, env, bar, reply } = await readerReply();
    const notCancelled = fireEvent.contextMenu(reply, { clientX: 40, clientY: 20, button: 2 });
    expect(notCancelled).toBe(false);
    const menu = await screen.findByRole("menu", { name: "Toolbar" });
    await user.click(within(menu).getByRole("menuitem", { name: "Customize toolbar" }));
    const dialog = await customizer();
    expect(env.store.getState().windows).toContain("toolbar-customizer:reader");
    expect(within(dialog).getByRole("combobox", { name: "Toolbar" })).toHaveValue("reader");
    expect(screen.queryByRole("menu", { name: "Toolbar" })).toBeNull();
    expect(bar).toHaveAttribute("data-customizing");
    await settleQueries(env);
  });

  it("opens with Shift+F10 on a button and gives focus back on Escape", async () => {
    const { user, env, reply } = await readerReply();
    act(() => {
      reply.focus();
    });
    await user.keyboard("{Shift>}{F10}{/Shift}");
    const menu = await screen.findByRole("menu", { name: "Toolbar" });
    await waitFor(() => {
      expect(within(menu).getByRole("menuitem", { name: "Customize toolbar" })).toHaveFocus();
    });
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(reply).toHaveFocus();
    expect(env.store.getState().windows).toEqual([]);
  });

  it("leaves the browser's menu when the layout has no toolbar menu", async () => {
    const env = selectedEnv();
    const base = env.transport.data.layout;
    env.transport.data.layout = {
      ...base,
      menus: Object.fromEntries(
        Object.entries(base.menus).filter(([n]) => n !== "context:toolbar"),
      ),
    };
    renderReader(env);
    const bar = await screen.findByRole("toolbar", { name: "Message actions" });
    expect(fireEvent.contextMenu(bar, { clientX: 5, clientY: 5, button: 2 })).toBe(true);
    expect(screen.queryByRole("menu")).toBeNull();
  });

  it("customizes the focused toolbar when run without an argument", () => {
    const bar = document.createElement("div");
    bar.dataset.part = "toolbar";
    bar.dataset.toolbar = "calendar";
    const button = document.createElement("button");
    bar.append(button);
    expect(toolbarToCustomize([], button)).toBe("calendar");
    expect(toolbarToCustomize(["reader"], button)).toBe("reader");
    expect(toolbarToCustomize([], null)).toBeNull();
  });
});

describe("toolbar customizer window (M5d)", () => {
  it("lists the bar, More and the available actions", async () => {
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    const bar = options(list(dialog, "Toolbar"));
    expect(bar.map((o) => o.textContent)).toEqual(
      expect.arrayContaining(["Reply", "Reply all", "Forward", "Separator", "Flexible space"]),
    );
    expect(bar).toHaveLength(barItems(reader(env)).length);
    expect(options(list(dialog, "More menu"))).toHaveLength(moreItems(reader(env)).length);
    expect(option(dialog, "Available actions", "Separator")).toBeInTheDocument();
    // A long list shows its first rows; the search finds the rest.
    expect(options(list(dialog, "Available actions"))).toHaveLength(PALETTE_LIMIT);
    expect(within(dialog).getByText(/^Showing 30 of \d+ actions/)).toBeInTheDocument();
    search(dialog, "reply all");
    expect(within(dialog).queryByText(/^Showing/)).toBeNull();
    // Actions already on the toolbar are marked.
    expect(option(dialog, "Available actions", "Reply all")).toHaveTextContent("On toolbar");
    await settleQueries(env);
  });

  it("moves, sends to More and removes with the keyboard; focus follows the item", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    act(() => {
      option(dialog, "Toolbar", "Reply").focus();
    });
    await user.keyboard("{ArrowRight}");
    expect(option(dialog, "Toolbar", "Reply all")).toHaveFocus();
    await user.keyboard("{Control>}{ArrowRight}{/Control}");
    await waitFor(() => {
      expect(barIds(env).slice(0, 3)).toEqual(["mail.reply", "mail.forward", "mail.reply-all"]);
    });
    expect(patches(env).at(-1)).toEqual({
      op: "move-toolbar-item",
      toolbar: "reader",
      from: { area: "bar", index: 1 },
      to: { area: "bar", index: 2 },
    });
    await waitFor(() => {
      expect(options(list(dialog, "Toolbar"))[2]).toHaveFocus();
    });
    expect(within(dialog).getByRole("status")).toHaveTextContent("Moved Reply all.");

    // M: into More (at its end).
    await user.keyboard("m");
    await waitFor(() => {
      expect(moreIds(env).at(-1)).toBe("mail.reply-all");
    });
    expect(barIds(env)).not.toContain("mail.reply-all");
    expect(within(dialog).getByRole("status")).toHaveTextContent("Moved Reply all to More.");
    await waitFor(() => {
      expect(options(list(dialog, "More menu")).at(-1)).toHaveFocus();
    });

    // Delete: off the toolbar.
    await user.keyboard("{Delete}");
    await waitFor(() => {
      expect(moreIds(env)).not.toContain("mail.reply-all");
    });
    expect(patches(env).at(-1)?.op).toBe("remove-toolbar-item");
    await settleQueries(env);
  });

  it("adds an available action with Enter (Shift+Enter: to More) and filters by search", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    await user.type(
      within(dialog).getByRole("searchbox", { name: "Find an action" }),
      "customize toolbar",
    );
    const palette = list(dialog, "Available actions");
    const shown = options(palette);
    expect(shown).toHaveLength(1);
    act(() => {
      must(shown[0]).focus();
    });
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(barIds(env).at(-1)).toBe("toolbar.customize");
    });
    expect(patches(env).at(-1)).toMatchObject({
      op: "add-toolbar-item",
      item: { type: "action", action: "toolbar.customize" },
      to: { area: "bar", index: barItems(reader(env)).length - 1 },
    });
    await user.keyboard("{Shift>}{Enter}{/Shift}");
    await waitFor(() => {
      expect(moreIds(env).at(-1)).toBe("toolbar.customize");
    });
    await user.clear(within(dialog).getByRole("searchbox", { name: "Find an action" }));
    await user.type(within(dialog).getByRole("searchbox", { name: "Find an action" }), "zzzz");
    expect(within(palette).getByText("No action matches.")).toBeInTheDocument();
    await settleQueries(env);
  });

  it("works with the buttons for the chosen item", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    const moveToMore = within(dialog).getByRole("button", { name: "Move to More" });
    expect(moveToMore).toBeDisabled();
    await user.click(option(dialog, "Toolbar", "Forward"));
    await user.click(moveToMore);
    await waitFor(() => {
      expect(moreIds(env).at(-1)).toBe("mail.forward");
    });
    await user.click(option(dialog, "More menu", "Forward"));
    expect(within(dialog).getByRole("button", { name: "Move to toolbar" })).toBeEnabled();
    await user.click(within(dialog).getByRole("button", { name: "Move earlier" }));
    await waitFor(() => {
      expect(moreIds(env).at(-2)).toBe("mail.forward");
    });
    await user.click(option(dialog, "Available actions", "Separator"));
    expect(within(dialog).getByRole("button", { name: "Move to More" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Remove" })).toBeDisabled();
    await user.click(within(dialog).getByRole("button", { name: "Add to More" }));
    await waitFor(() => {
      expect(moreIds(env).at(-1)).toBe("separator");
    });
    await user.click(option(dialog, "Available actions", "Flexible space"));
    // A spacer means nothing in a menu.
    expect(within(dialog).getByRole("button", { name: "Add to More" })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "Add to toolbar" })).toBeEnabled();
    await settleQueries(env);
  });

  it("drags between the bar, More and the available actions", async () => {
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    // A bar button onto the end of More.
    act(() => {
      drag(option(dialog, "Toolbar", "Forward"), list(dialog, "More menu"));
    });
    await waitFor(() => {
      expect(moreIds(env).at(-1)).toBe("mail.forward");
    });
    expect(patches(env).at(-1)).toEqual({
      op: "move-toolbar-item",
      toolbar: "reader",
      from: { area: "bar", index: 2 },
      to: { area: "more", index: moreItems(reader(env)).length - 1 },
    });
    // An available action dropped before the first bar button.
    search(dialog, "customize toolbar");
    act(() => {
      drag(
        option(dialog, "Available actions", /^Customize toolbar/),
        option(dialog, "Toolbar", "Reply"),
        {
          clientX: -1,
        },
      );
    });
    await waitFor(() => {
      expect(barIds(env)[0]).toBe("toolbar.customize");
    });
    expect(patches(env).at(-1)).toMatchObject({
      op: "add-toolbar-item",
      to: { area: "bar", index: 0 },
    });
    // A bar button dropped after another one (the second half of it).
    act(() => {
      drag(option(dialog, "Toolbar", "Customize toolbar"), option(dialog, "Toolbar", "Reply all"), {
        clientX: 1,
      });
    });
    await waitFor(() => {
      expect(barIds(env).slice(0, 3)).toEqual([
        "mail.reply",
        "mail.reply-all",
        "toolbar.customize",
      ]);
    });
    expect(patches(env).at(-1)).toEqual({
      op: "move-toolbar-item",
      toolbar: "reader",
      from: { area: "bar", index: 0 },
      to: { area: "bar", index: 2 },
    });
    // Back onto the available actions: removed.
    act(() => {
      drag(option(dialog, "Toolbar", "Customize toolbar"), list(dialog, "Available actions"));
    });
    await waitFor(() => {
      expect(barIds(env)).not.toContain("toolbar.customize");
    });
    // A heading cannot go onto the bar: no drop there.
    const heading = options(list(dialog, "More menu"))[0];
    const dataTransfer = transfer();
    fireEvent.dragStart(must(heading), { dataTransfer });
    expect(fireEvent.dragOver(list(dialog, "Toolbar"), { dataTransfer })).toBe(true);
    fireEvent.dragEnd(must(heading), { dataTransfer });
    await settleQueries(env);
  });

  it("offers plug-in actions", async () => {
    const user = userEvent.setup();
    const env = selectedEnv();
    const source = { kind: "file", path: "C:/plugins/follow-up.nailplugin" } as const;
    const preview = await env.client.request(
      { type: "PreviewPluginInstall", source },
      "PluginInstallPreview",
    );
    await env.client.call({
      type: "InstallPlugin",
      source,
      package_sha256: preview.preview.package_sha256,
      grants: must(followUpManifest().permissions).map((permission) => ({
        permission,
        accounts: null,
      })),
      enable: true,
    });
    renderReader(env);
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    search(dialog, "follow");
    const followUp = await waitFor(() => option(dialog, "Available actions", "Follow up"));
    expect(followUp).toHaveAttribute("data-source", "plugin");
    expect(followUp).toHaveTextContent("Plug-in");
    await user.click(followUp);
    await user.click(within(dialog).getByRole("button", { name: "Add to More" }));
    await waitFor(() => {
      expect(moreIds(env).at(-1)).toBe("plugin.follow-up.remind");
    });
    await settleQueries(env);
  });

  it("undoes, redoes and resets through the layout history", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    const before = structuredClone(reader(env));
    const undo = within(dialog).getByRole("button", { name: "Undo" });
    expect(undo).toBeDisabled();
    await user.click(option(dialog, "Toolbar", "Reply"));
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(undo).toBeEnabled();
    });
    expect(undo).toHaveAttribute("title", "Undo: Remove toolbar button");
    await user.click(undo);
    await waitFor(() => {
      expect(reader(env)).toEqual(before);
    });
    expect(within(dialog).getByRole("status")).toHaveTextContent("Undone: Remove toolbar button");
    await user.click(within(dialog).getByRole("button", { name: "Redo" }));
    await waitFor(() => {
      expect(barIds(env)).not.toContain("mail.reply");
    });

    await user.click(within(dialog).getByRole("button", { name: "Reset to default" }));
    await waitFor(() => {
      expect(reader(env)).toEqual(normalizeLayout(defaultLayoutJson).toolbars.reader);
    });
    expect(patches(env).at(-1)).toEqual({ op: "reset-toolbar", toolbar: "reader" });
    expect(within(dialog).getByRole("status")).toHaveTextContent("back to its default buttons");

    await user.click(within(dialog).getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(env.store.getState().windows).toEqual([]);
    await settleQueries(env);
  });

  it("refuses an edit on a changed file and offers Undo anyway", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    await user.click(option(dialog, "Toolbar", "Reply"));
    await user.click(within(dialog).getByRole("button", { name: "Move to More" }));
    await waitFor(() => {
      expect(moreIds(env)).toContain("mail.reply");
    });
    await settleQueries(env);

    // The file changes on disk: the next edit is refused with the base revision it saw.
    const seen = must(
      env.queryClient.getQueryData<{ revision: string }>(["edit-history", "layout"]),
    );
    act(() => {
      simulateExternalEdit(env.transport, "layout");
    });
    const count = patches(env).length;
    expect(
      await env.client.call({
        type: "UpdateLayout",
        patches: [
          { op: "remove-toolbar-item", toolbar: "reader", from: { area: "bar", index: 0 } },
        ],
        base_revision: seen.revision,
      }),
    ).toMatchObject({ code: "conflict" });
    expect(patches(env)).toHaveLength(count);

    await user.click(within(dialog).getByRole("button", { name: "Undo" }));
    const status = within(dialog).getByRole("status");
    await waitFor(() => {
      expect(status).toHaveTextContent("layout.kdl was changed outside Nail");
    });
    await user.click(within(status).getByRole("button", { name: "Undo anyway" }));
    await waitFor(() => {
      expect(moreIds(env)).not.toContain("mail.reply");
    });
    expect(barIds(env)[0]).toBe("mail.reply");
    await settleQueries(env);
  });

  it("shows the core's answer when an edit is refused", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env);
    const dialog = await customizer();
    await settleQueries(env);
    env.transport.data.features.customization.layoutPatches.length = 0;
    // The UI still holds the old revision until the event arrives; a stale one answers conflict.
    staleLayoutRevision(env);
    await user.click(option(dialog, "Toolbar", "Reply"));
    await user.click(within(dialog).getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(within(dialog).getByRole("status")).toHaveTextContent("changed outside Nail");
    });
    expect(within(dialog).getByRole("status")).toHaveAttribute("data-tone", "error");
    expect(patches(env)).toEqual([]);
    await settleQueries(env);
  });

  it("lets the user pick the toolbar when the window names none", async () => {
    const user = userEvent.setup();
    const { env } = renderReader();
    openCustomizer(env, "toolbar-customizer");
    const dialog = await customizer();
    await settleQueries(env);
    const select = within(dialog).getByRole("combobox", { name: "Toolbar" });
    expect(within(select).getByRole("option", { name: "Message actions" })).toBeInTheDocument();
    await user.selectOptions(select, "calendar");
    expect(within(dialog).getByRole("combobox", { name: "Toolbar" })).toHaveValue("calendar");
    expect(dialog.querySelector('[data-toolbar="calendar"]')).not.toBeNull();
  });
});

describe("toolbar settings card (M5d)", () => {
  it("lists the toolbars and opens their customizer", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithLive(
      <Live env={env}>
        <NodeView node={widget("toolbar-editor-settings")} path="test" />
        <Windows />
      </Live>,
    );
    const card = await screen.findByRole("region", { name: "Toolbars" });
    expect(within(card).getAllByRole("listitem")).toHaveLength(
      Object.keys(env.transport.data.layout.toolbars).length,
    );
    await user.click(within(card).getByRole("button", { name: "Customize Message actions" }));
    await customizer();
    expect(env.store.getState().windows).toEqual(["toolbar-customizer:reader"]);
    await settleQueries(env);
  });
});
