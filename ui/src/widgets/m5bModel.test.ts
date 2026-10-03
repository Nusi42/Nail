/**
 * M5b WP162: the pure parts of the mail view — toolbar display / tooltips /
 * the "More" menu / wrapped separators, the folder pane's favorites and
 * collapsing, the list filter chips, and the list header's title rules.
 */
import { QueryClient } from "@tanstack/react-query";
import { TransportClient } from "../api/client";
import type { Folder, ResolvedLayout, ToolbarItem } from "../api/generated";
import { MockTransport } from "../api/mock";
import { createMockData } from "../api/mockData";
import { normalizeLayout } from "../api/normalize";
import { UI_STATE_COLLAPSED_SECTIONS, uiStateKey } from "../state/uiState";
import { accountCount, folderTitle, searchShortcut } from "./ListHeader";
import { accountTags } from "./MessageList";
import {
  FAVORITE_TARGETS,
  favoriteCount,
  toggleCollapsedSection,
  toggledSections,
  unifiedFolder,
} from "./folderPaneState";
import { FAVORITE_FOLDERS } from "./folderPane";
import { LIST_FILTERS, LIST_FILTER_TERMS, filteredScope, isListFilter } from "./listFilter";
import {
  bindingOf,
  displayOf,
  moreMenuEntries,
  toolbarSettingsOf,
  tooltipMessage,
  wrapEdges,
} from "./toolbarModel";

function layoutWith(extra: Record<string, unknown>): ResolvedLayout {
  return normalizeLayout({ ...createMockData(1).layout, ...extra });
}

const action = (id: string, extra: Record<string, unknown> = {}) => ({
  type: "action",
  action: id,
  ...extra,
});

describe("toolbar model", () => {
  it("shows icon and label unless the item says otherwise", () => {
    const [plain, icon] = layoutWith({
      toolbars: { t: [action("mail.reply"), action("mail.archive", { display: "icon" })] },
    }).toolbars.t as Extract<ToolbarItem, { type: "action" }>[];
    expect(displayOf(plain as Extract<ToolbarItem, { type: "action" }>)).toBe("icon-label");
    expect(displayOf(icon as Extract<ToolbarItem, { type: "action" }>)).toBe("icon");
  });

  it("reads per-toolbar settings, with the defaults for toolbars that set nothing", () => {
    const layout = layoutWith({
      toolbars: { reader: [], main: [] },
      toolbar_settings: { reader: { hide_disabled: true, overflow: "more" } },
    });
    expect(toolbarSettingsOf(layout, "reader")).toEqual({ hide_disabled: true, overflow: "more" });
    expect(toolbarSettingsOf(layout, "main")).toEqual({ hide_disabled: false, overflow: "more" });
    expect(toolbarSettingsOf(layout, "nope")).toEqual({ hide_disabled: false, overflow: "more" });
  });

  it("finds the key binding in the toolbar's context first, then global", () => {
    const layout = layoutWith({
      keys: [
        { context: "global", key: "e", action: "mail.archive", args: [] },
        { context: "reader", key: "ctrl+e", action: "mail.archive", args: [] },
        { context: "global", key: "x", action: "mail.archive", args: [] },
        { context: "global", key: "g i", action: "nav.goto", args: ["inbox"] },
      ],
    });
    expect(bindingOf(layout, "mail.archive", [], "reader")).toBe("ctrl+e");
    // The last global binding wins, like the key matcher.
    expect(bindingOf(layout, "mail.archive", [], "list")).toBe("x");
    expect(bindingOf(layout, "nav.goto", ["inbox"], "global")).toBe("g i");
    expect(bindingOf(layout, "nav.goto", ["sent"], "global")).toBeNull();
    expect(bindingOf(layout, "mail.reply", [], "global")).toBeNull();
  });

  it("puts the shortcut into the tooltip only when there is one", () => {
    expect(tooltipMessage("Archive", "E")).toEqual({
      id: "toolbar-button-tooltip",
      args: { label: "Archive", shortcut: "E" },
    });
    expect(tooltipMessage("Archive", null)).toBeNull();
  });

  it("builds the More menu with what did not fit first, then its own entries", () => {
    const layout = layoutWith({
      keys: [{ context: "global", key: "u", action: "mail.toggle-read", args: [] }],
      toolbars: {
        reader: [
          action("mail.reply"),
          action("mail.archive"),
          { type: "separator" },
          {
            type: "more",
            items: [
              { type: "header", label: "@toolbar.group.message" },
              action("mail.toggle-read"),
              { type: "header", label: "@toolbar.group.turn-into" },
            ],
          },
        ],
      },
    });
    const items = layout.toolbars.reader ?? [];
    const entries = moreMenuEntries(layout, items, [1], "global");
    expect(entries.map((e) => e.type)).toEqual(["action", "separator", "header", "action"]);
    expect(entries[0]).toMatchObject({ action: "mail.archive" });
    expect(entries[3]).toMatchObject({ action: "mail.toggle-read", shortcut: "u" });
    // An orphaned heading (nothing below it) is dropped; nothing moved:
    // only the own entries.
    expect(moreMenuEntries(layout, items, [], "global").map((e) => e.type)).toEqual([
      "header",
      "action",
    ]);
    // A toolbar without a `more` item: only the moved actions.
    const plain = [action("mail.reply"), action("mail.forward")] as ToolbarItem[];
    expect(
      moreMenuEntries(
        layout,
        normalizeLayout({ toolbars: { p: plain } }).toolbars.p ?? [],
        [1],
        "global",
      ),
    ).toMatchObject([{ type: "action", action: "mail.forward", args: [], shortcut: null }]);
    expect(moreMenuEntries(layout, [], [], "global")).toEqual([]);
  });

  it("lists overflowed actions in rank order, unranked ones after them (M5b review)", () => {
    const bar = [
      { ...action("mail.forward"), rank: 3 },
      { ...action("plugin.x"), rank: null },
      { ...action("mail.archive"), rank: 4 },
      { ...action("snooze.open"), rank: 8 },
      { ...action("mail.delete"), rank: 5 },
      { type: "more", label: null, icon: null, items: [action("print.message")] },
    ] as ToolbarItem[];
    const layout = layoutWith({});
    // The fitting rule moves the highest ranks first: Snooze, Delete, Archive, …
    const entries = moreMenuEntries(layout, bar, [3, 4, 2, 1, 0], "global");
    expect(entries.map((e) => (e.type === "action" ? e.action : e.type))).toEqual([
      "mail.forward",
      "mail.archive",
      "mail.delete",
      "snooze.open",
      "plugin.x",
      "separator",
      "print.message",
    ]);
  });

  it("removes separators that start a wrapped line and hides those ending one", () => {
    const at = (kind: ToolbarItem["type"], top: number) => ({ kind, top });
    expect(
      wrapEdges([
        at("action", 0),
        at("separator", 0),
        at("action", 0),
        at("separator", 40),
        at("action", 40),
        at("action", 40),
        at("separator", 40),
        at("action", 80),
      ]),
    ).toEqual([null, null, null, "start", null, null, "end", null]);
    expect(wrapEdges([at("separator", 0), at("action", 0)])).toEqual(["start", null]);
    expect(wrapEdges([at("action", 0), at("separator", 0)])).toEqual([null, "end"]);
  });
});

describe("folder pane state", () => {
  const folder = (id: string, role: Folder["role"], extra: Partial<Folder> = {}): Folder => ({
    id,
    account_id: "all",
    parent_id: null,
    name: id,
    path: id,
    role,
    unread: 0,
    total: 0,
    ...extra,
  });

  it("toggles section ids", () => {
    expect(toggledSections([], "favorites")).toEqual(["favorites"]);
    expect(toggledSections(["favorites", "account:a"], "favorites")).toEqual(["account:a"]);
  });

  it("gives every favorite a target, and counts inboxes (unread) and drafts (all)", () => {
    for (const favorite of FAVORITE_FOLDERS) expect(FAVORITE_TARGETS[favorite]).toBeDefined();
    expect(FAVORITE_TARGETS["all-inboxes"]).toEqual({ kind: "folder", role: "inbox" });
    expect(FAVORITE_TARGETS.flagged).toEqual({ kind: "search", text: "is:flagged" });
    expect(FAVORITE_TARGETS.snoozed).toEqual({ kind: "action", action: "snooze.list" });
    const folders = [
      folder("all/inbox", "inbox", { unread: 7, total: 30 }),
      folder("all/drafts", "drafts", { unread: 0, total: 2 }),
      folder("all/account-a/inbox", "inbox", { parent_id: "all/account-a", unread: 99 }),
    ];
    expect(unifiedFolder(folders, "inbox")?.id).toBe("all/inbox");
    expect(unifiedFolder(folders, "sent")).toBeNull();
    expect(favoriteCount("all-inboxes", folders)).toBe(7);
    expect(favoriteCount("drafts", folders)).toBe(2);
    expect(favoriteCount("flagged", folders)).toBeNull();
    expect(favoriteCount("all-inboxes", [])).toBeNull();
  });

  it("collapses a section in the cache at once and asks the core to remember it", async () => {
    const transport = new MockTransport({ data: createMockData(1) });
    const client = new TransportClient(transport);
    const queryClient = new QueryClient();
    queryClient.setQueryData(uiStateKey, {});
    await toggleCollapsedSection(client, queryClient, "account:acc-work");
    expect(queryClient.getQueryData(uiStateKey)).toEqual({
      [UI_STATE_COLLAPSED_SECTIONS]: ["account:acc-work"],
    });
    expect(transport.requests).toContainEqual({
      type: "SetUiState",
      key: UI_STATE_COLLAPSED_SECTIONS,
      value: ["account:acc-work"],
    });
    // Expanding the last collapsed section removes the key.
    await toggleCollapsedSection(client, queryClient, "account:acc-work");
    expect(queryClient.getQueryData(uiStateKey)).toEqual({});
    expect(transport.requests).toContainEqual({
      type: "SetUiState",
      key: UI_STATE_COLLAPSED_SECTIONS,
      value: null,
    });
  });
});

describe("list filters", () => {
  it("map to the query language like nail_api::views::ListFilter", () => {
    expect(LIST_FILTERS).toEqual(["all", "unread", "flagged", "attachments"]);
    expect(LIST_FILTER_TERMS).toEqual({
      all: null,
      unread: "is:unread",
      flagged: "is:flagged",
      attachments: "has:attachment",
    });
    expect(isListFilter("unread")).toBe(true);
    expect(isListFilter("starred")).toBe(false);
    expect(isListFilter(3)).toBe(false);
  });

  it("search the shown folder, or narrow the user's search", () => {
    expect(filteredScope("all", null, "a", "a/Inbox")).toBeNull();
    expect(filteredScope("unread", null, "a", "a/Inbox")).toEqual({
      text: "is:unread",
      accountId: "a",
      folderId: "a/Inbox",
    });
    expect(filteredScope("unread", null, "a", null)).toBeNull();
    expect(
      filteredScope(
        "attachments",
        { text: "from:ada OR from:grace", accountId: null, folderId: null },
        "a",
        "a/Inbox",
      ),
    ).toEqual({ text: "(from:ada OR from:grace) has:attachment", accountId: null, folderId: null });
  });
});

describe("list header rules", () => {
  const inbox: Folder = {
    id: "all/inbox",
    account_id: "all",
    parent_id: null,
    name: "Inbox",
    path: "Inbox",
    role: "inbox",
    unread: 3,
    total: 9,
  };

  it("titles unified folders like their favorite, special folders by role, others by name", () => {
    expect(folderTitle(inbox, "all")).toEqual({ id: "favorite-folder-all-inboxes" });
    expect(folderTitle({ ...inbox, role: "sent" }, "all")).toEqual({ id: "favorite-folder-sent" });
    expect(folderTitle({ ...inbox, role: "archive", name: "ARCHIVE" }, "all")).toEqual({
      id: "folder-role-archive",
    });
    expect(folderTitle({ ...inbox, name: "INBOX" }, "acc-work")).toEqual({
      id: "folder-role-inbox",
    });
    expect(folderTitle({ ...inbox, role: "other", name: "Newsletters" }, "acc-work")).toEqual({
      text: "Newsletters",
    });
    expect(folderTitle(undefined, "all")).toBeNull();
  });

  it("counts the real accounts of All accounts only", () => {
    const accounts = createMockData(1).accounts;
    expect(
      accountCount("all", [...accounts, { id: "v", name: "V", email: "", kind: "virtual" }]),
    ).toBe(2);
    expect(accountCount("acc-work", accounts)).toBeNull();
  });

  it("prefers a chord with a modifier as the search hint", () => {
    const keys = (list: [string, string][]) =>
      layoutWith({
        keys: list.map(([key, a]) => ({ context: "global", key, action: a, args: [] })),
      });
    expect(
      searchShortcut(
        keys([
          ["/", "search.focus"],
          ["ctrl+k", "search.focus"],
        ]),
      ),
    ).toBe("ctrl+k");
    expect(searchShortcut(keys([["/", "search.focus"]]))).toBe("/");
    expect(searchShortcut(keys([["ctrl+k", "palette.open"]]))).toBeNull();
  });

  it("colours accounts by their order, like the folder pane", () => {
    const tags = accountTags([
      { id: "all", name: "", email: "" },
      { id: "work", name: "Work", email: "w@x" },
      { id: "home", name: "", email: "h@x" },
    ]);
    expect([...tags.entries()]).toEqual([
      ["work", { name: "Work", slot: 0 }],
      ["home", { name: "h@x", slot: 1 }],
    ]);
  });
});
