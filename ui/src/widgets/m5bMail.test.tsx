/**
 * M5b WP162: the mail view — the grouped folder pane (New message,
 * Favorites, collapsible account sections with colours, smart folders only
 * when there are some), the list header (search with its key hint, title
 * with counts, filter chips on the query language), dates in the UI locale,
 * the reader (body, attachment status) and axe in all five themes.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { ResolvedLayout } from "../api/generated";
import { defaultTheme } from "../api/mockData";
import { normalizeLayout } from "../api/normalize";
import { listFolders } from "../api/mockMail";
import { BUILTIN_THEMES } from "../features/appearance";
import { resolveMockTheme } from "../features/appearance/themes";
import { FULL_DATE_TIME, formatDateTime, formatRelative, formatShortDate } from "../i18n/format";
import { NodeView } from "../layout/LayoutRenderer";
import { UI_STATE_COLLAPSED_SECTIONS } from "../state/uiState";
import { violations } from "../test/axe";
import {
  createEnv,
  renderNode,
  renderWithProviders,
  settleQueries,
  stubElementSizes,
  widget,
  type Env,
} from "../test/harness";
import { PROPOSAL_READER } from "./proposalToolbars";
import { must } from "../test/must";
import { objectContaining } from "../test/matchers";

const INBOX = "acc-personal/Inbox";
const FIRST = `${INBOX}#1`;

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
  vi.restoreAllMocks();
});

/** The grouped pane as the default layout configures it (WP165). */
const PANE = widget("folder-tree", {
  options: {
    sections: "favorites accounts smart-folders",
    favorites: "all-inboxes flagged snoozed drafts",
    collapsible: true,
    "account-colors": true,
  },
});

function paneEnv(): Env {
  return createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
}

function section(id: string): HTMLElement {
  const el = document.querySelector<HTMLElement>(
    `[data-part="folder-tree.section"][data-section="${id}"]`,
  );
  if (!el) throw new Error(`no section ${id}`);
  return el;
}

describe("grouped folder pane (M5b)", () => {
  it("starts with New message, then Favorites and one section per account", async () => {
    const env = paneEnv();
    renderNode(PANE, env);
    const tree = await screen.findByRole("tree", { name: "Folders" });
    await screen.findByText("Personal · ada@example.org");
    const compose = screen.getByRole("button", { name: "New message" });
    expect(compose).toHaveAttribute("data-part", "folder-tree.compose");
    // Before the tree in the pane.
    expect(compose.compareDocumentPosition(tree) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    const sections = [...tree.querySelectorAll('[data-part="folder-tree.section"]')];
    expect(sections.map((s) => s.getAttribute("data-section"))).toEqual([
      "favorites",
      "account:acc-personal",
      "account:acc-work",
    ]);
    // No smart folders yet: no empty section.
    expect(tree.querySelector('[data-section="smart-folders"]')).toBeNull();
    expect(within(section("favorites")).getByText("Favorites")).toBeInTheDocument();
    const work = section("account:acc-work");
    expect(within(work).getByText("Work · ada@work.example")).toBeInTheDocument();
    expect(work).toHaveAttribute("aria-expanded", "true");
    expect(work).toHaveAccessibleName("Work · ada@work.example");
    // Account colours follow the account order (avatar palette slots).
    const dot = (id: string) => section(id).querySelector('[data-part="folder-tree.section.dot"]');
    expect(dot("account:acc-personal")).toHaveAttribute("data-color", "0");
    expect(dot("account:acc-work")).toHaveAttribute("data-color", "1");
    // Account folders keep their part (context menus, themes) one level down.
    const workInbox = await within(work).findByRole("treeitem", { name: /Inbox/ });
    expect(workInbox).toHaveAttribute("data-part", "folder-tree.item");
    expect(workInbox).toHaveAttribute("aria-level", "2");
    expect(within(work).getByRole("treeitem", { name: /Receipts/ })).toHaveAttribute(
      "aria-level",
      "3",
    );
    // The selected folder is marked in its own account only.
    const personalInbox = await within(section("account:acc-personal")).findByRole("treeitem", {
      name: /Inbox/,
    });
    expect(personalInbox).toHaveAttribute("aria-selected", "true");
    expect(workInbox).toHaveAttribute("aria-selected", "false");
  });

  it("opens the composer from New message", async () => {
    const env = paneEnv();
    renderNode(PANE, env);
    fireEvent.click(await screen.findByRole("button", { name: "New message" }));
    await waitFor(() => {
      expect(env.store.getState().windows.some((w) => w.startsWith("composer:"))).toBe(true);
    });
  });

  it("chooses folders across accounts and the unified favorites", async () => {
    const env = paneEnv();
    renderNode(PANE, env);
    const work = await screen.findByText("Work · ada@work.example");
    const workSection = must(work.closest<HTMLElement>('[data-part="folder-tree.section"]'));
    fireEvent.click(await within(workSection).findByRole("treeitem", { name: /Archive/ }));
    expect(env.store.getState().activeAccountId).toBe("acc-work");
    expect(env.store.getState().activeFolderId).toBe("acc-work/Archive");

    // All inboxes: the unified inbox of "All accounts" with its unread count.
    const unified = listFolders(env.transport.data, "all")?.find((f) => f.role === "inbox");
    const allInboxes = within(section("favorites")).getByRole("treeitem", { name: /All inboxes/ });
    expect(allInboxes).toHaveAttribute("data-part", "folder-tree.favorite");
    await waitFor(() =>
      expect(
        allInboxes.querySelector('[data-part="folder-tree.item.unread-count"]'),
      ).toHaveTextContent(String(unified?.unread)),
    );
    fireEvent.click(allInboxes);
    expect(env.store.getState().activeAccountId).toBe("all");
    expect(env.store.getState().activeFolderId).toBe(unified?.id);
    expect(allInboxes).toHaveAttribute("aria-selected", "true");

    // Flagged: a search over every account.
    fireEvent.click(within(section("favorites")).getByRole("treeitem", { name: /Flagged/ }));
    expect(env.store.getState().search).toEqual({
      text: "is:flagged",
      accountId: null,
      folderId: null,
      error: null,
    });
    expect(env.store.getState().searchInput).toBe("is:flagged");
    // Snoozed: its own window.
    fireEvent.click(within(section("favorites")).getByRole("treeitem", { name: /Snoozed/ }));
    await waitFor(() => {
      expect(env.store.getState().windows).toContain("snoozed");
    });
  });

  it("collapses a section, remembers it in the UI state and shows its unread mail", async () => {
    const env = paneEnv();
    renderNode(PANE, env);
    const personal = await screen.findByText("Personal · ada@example.org");
    const header = must(personal.closest<HTMLElement>('[data-part="folder-tree.account"]'));
    expect(header).toHaveAttribute("data-account", "acc-personal");
    fireEvent.click(header);
    const sectionEl = section("account:acc-personal");
    await waitFor(() => expect(sectionEl).toHaveAttribute("aria-expanded", "false"));
    expect(within(sectionEl).queryByRole("treeitem")).toBeNull();
    expect(
      sectionEl.querySelector('[data-part="folder-tree.section.unread-count"]'),
    ).toHaveTextContent("10");
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "SetUiState",
        key: UI_STATE_COLLAPSED_SECTIONS,
        value: ["account:acc-personal"],
      });
    });
    // The collapsed section keeps the tab stop of the selected folder.
    expect(sectionEl).toHaveAttribute("tabindex", "0");
    fireEvent.click(header);
    await waitFor(() => expect(sectionEl).toHaveAttribute("aria-expanded", "true"));
    await settleQueries(env);
  });

  it("starts with the sections the UI state remembers as collapsed", async () => {
    const env = paneEnv();
    await env.client.request(
      { type: "SetUiState", key: UI_STATE_COLLAPSED_SECTIONS, value: ["favorites"] },
      "UiState",
    );
    renderNode(PANE, env);
    await waitFor(() => expect(section("favorites")).toHaveAttribute("aria-expanded", "false"));
    expect(section("account:acc-work")).toHaveAttribute("aria-expanded", "true");
  });

  it("shows smart folders only when there is one, and runs them", async () => {
    const env = paneEnv();
    env.transport.data.features.customization.savedSearches.push({
      id: "s1",
      name: "From Grace",
      query: "from:grace",
      account_id: null,
      folder_id: null,
      sort: "date",
      position: 0,
    });
    renderNode(PANE, env);
    const smart = await screen.findByRole("treeitem", { name: /From Grace/ });
    expect(smart).toHaveAttribute("data-part", "folder-tree.smart-folder");
    expect(section("smart-folders")).toHaveAccessibleName("Smart folders");
    fireEvent.click(smart);
    expect(env.store.getState().search?.text).toBe("from:grace");
    expect(smart).toHaveAttribute("aria-selected", "true");
  });

  it("moves with the arrow keys, expands and collapses with Right / Left, chooses with Enter", async () => {
    const env = paneEnv();
    renderNode(PANE, env);
    await screen.findByText("Work · ada@work.example");
    const favorites = section("favorites");
    act(() => {
      favorites.focus();
    });
    fireEvent.keyDown(favorites, { key: "ArrowDown" });
    const allInboxes = within(favorites).getByRole("treeitem", { name: /All inboxes/ });
    expect(allInboxes).toHaveFocus();
    fireEvent.keyDown(allInboxes, { key: "ArrowLeft" });
    expect(favorites).toHaveFocus();
    fireEvent.keyDown(favorites, { key: "ArrowLeft" });
    await waitFor(() => expect(favorites).toHaveAttribute("aria-expanded", "false"));
    fireEvent.keyDown(favorites, { key: "ArrowRight" });
    await waitFor(() => expect(favorites).toHaveAttribute("aria-expanded", "true"));
    fireEvent.keyDown(favorites, { key: "ArrowRight" });
    expect(within(favorites).getByRole("treeitem", { name: /All inboxes/ })).toHaveFocus();
    fireEvent.keyDown(must(document.activeElement), { key: "End" });
    const last = document.activeElement as HTMLElement;
    expect(last).toHaveAttribute("data-part", "folder-tree.item");
    fireEvent.keyDown(last, { key: "Enter" });
    expect(env.store.getState().activeAccountId).toBe("acc-work");
    expect(env.store.getState().activeFolderId).toBe(last.getAttribute("data-folder"));
    await settleQueries(env);
  });

  it("keeps the pre-M5b tree of the active account without the M5b options", async () => {
    const env = paneEnv();
    renderNode(widget("folder-tree"), env);
    await screen.findByRole("treeitem", { name: /Inbox/ });
    expect(document.querySelector('[data-part="folder-tree.section"]')).toBeNull();
    expect(screen.queryByRole("button", { name: "New message" })).toBeNull();
  });

  it("toggles a section with folders.toggle-section", async () => {
    const env = paneEnv();
    const layout: ResolvedLayout = normalizeLayout({
      ...env.transport.data.layout,
      toolbars: {
        t: [
          { type: "action", action: "folders.toggle-section", args: ["favorites"], label: "Fold" },
        ],
      },
    });
    renderWithProviders(
      <>
        <NodeView node={PANE} path="t/0" />
        <NodeView node={widget("toolbar", { options: { toolbar: "t" } })} path="t/1" />
      </>,
      env,
      { layout },
    );
    await screen.findByText("Favorites");
    fireEvent.click(screen.getByRole("button", { name: "Fold" }));
    await waitFor(() => expect(section("favorites")).toHaveAttribute("aria-expanded", "false"));
  });
});

describe("folder pane after the M5b review", () => {
  it("shows an account's state as a tag beside its heading, outside the cut title", async () => {
    const env = paneEnv();
    const work = env.transport.data.statuses.find((x) => x.account_id === "acc-work");
    if (work) work.state = "disabled";
    renderNode(PANE, env);
    await screen.findByText("Work · ada@work.example");
    const heading = section("account:acc-work").querySelector('[data-part="folder-tree.account"]');
    const title = heading?.querySelector('[data-part="folder-tree.section.title"]');
    const status = await within(heading as HTMLElement).findByText("Disabled");
    expect(status).toHaveAttribute("data-part", "folder-tree.section.status");
    // A sibling of the title (only the title is cut with an ellipsis) …
    expect(status.parentElement).toBe(heading);
    expect(title?.contains(status)).toBe(false);
    expect(title).toHaveAttribute("title", "Work · ada@work.example");
    // … and still part of the section's name.
    expect(section("account:acc-work")).toHaveAccessibleName("Work · ada@work.example Disabled");
    expect(
      section("account:acc-personal").querySelector('[data-part="folder-tree.section.status"]'),
    ).toBeNull();
  });

  it("lists the Outbox as a favorite only while mail waits there", async () => {
    const env = paneEnv();
    const pane = widget("folder-tree", {
      options: { ...PANE.options, favorites: "all-inboxes outbox" },
    });
    const { unmount } = renderNode(pane, env);
    await screen.findByText("Favorites");
    await settleQueries(env);
    expect(document.querySelector('[data-favorite="outbox"]')).toBeNull();
    unmount();
    env.queryClient.clear();
    env.transport.data.outbox.push({
      id: "o1",
      draft_id: "d1",
      account_id: "acc-personal",
      identity_id: "i1",
      subject: "Later",
      recipients: [],
      send_at: "2026-10-03T12:00:00Z",
      state: "scheduled",
      attempts: 0,
      error: null,
      message: null,
    });
    renderNode(pane, env);
    const outbox = await screen.findByRole("treeitem", { name: /Outbox/ });
    expect(within(outbox).getByLabelText("One message waiting to be sent")).toHaveTextContent("1");
  });
});

describe("list header (M5b)", () => {
  function headerLayout(env: Env): ResolvedLayout {
    return normalizeLayout({
      ...env.transport.data.layout,
      keys: [
        ...env.transport.data.layout.keys,
        { context: "global", key: "ctrl+k", action: "search.focus", args: [] },
      ],
    });
  }

  it("shows the search with its key, the title with counts and the chips", async () => {
    const env = createEnv({ state: { activeAccountId: "all", activeFolderId: "all/inbox" } });
    renderNode(widget("list-header"), env, { layout: headerLayout(env) });
    const region = screen.getByRole("region", { name: "Message list" });
    const input = within(region).getByRole("searchbox", { name: "Search mail" });
    expect(input).toHaveAttribute("aria-keyshortcuts", "Control+K");
    expect(region.querySelector('[data-part="search-bar.shortcut"]')).toHaveTextContent("Ctrl+K");
    expect(await within(region).findByRole("heading", { name: "All inboxes" })).toHaveAttribute(
      "data-part",
      "list-header.title",
    );
    const unread = listFolders(env.transport.data, "all")?.find(
      (f) => f.id === "all/inbox",
    )?.unread;
    expect(region.querySelector('[data-part="list-header.counts"]')).toHaveTextContent(
      `${String(must(unread))} unread · 2 accounts`,
    );
    const chips = within(
      within(region).getByRole("group", { name: "Filter messages" }),
    ).getAllByRole("button");
    expect(chips.map((c) => c.textContent)).toEqual(["All", "Unread", "Flagged", "Attachments"]);
    expect(chips[0]).toHaveAttribute("aria-pressed", "true");
    // Typing hides the key hint; the search title replaces the folder's.
    fireEvent.change(input, { target: { value: "grace" } });
    expect(region.querySelector('[data-part="search-bar.shortcut"]')).toBeNull();
    fireEvent.submit(must(input.closest<HTMLElement>("form")));
    expect(within(region).getByRole("heading", { name: "Search results" })).toBeInTheDocument();
    await settleQueries(env);
  });

  it("names a real account's folder and its unread mail only", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    renderNode(widget("list-header", { options: { chips: "all unread" } }), env);
    expect(await screen.findByRole("heading", { name: "Inbox" })).toBeInTheDocument();
    expect(document.querySelector('[data-part="list-header.counts"]')).toHaveTextContent(
      "10 unread",
    );
    expect(screen.getAllByRole("button", { pressed: false }).map((b) => b.textContent)).toEqual([
      "Unread",
    ]);
  });

  it("filters the list with the chips through the query language", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    renderWithProviders(
      <>
        <NodeView node={widget("list-header")} path="t/0" />
        <NodeView node={widget("message-list")} path="t/1" />
      </>,
      env,
    );
    const all = await screen.findAllByRole("option");
    fireEvent.click(screen.getByRole("button", { name: "Unread" }));
    expect(env.store.getState().listFilter).toBe("unread");
    expect(screen.getByRole("button", { name: "Unread" })).toHaveAttribute("aria-pressed", "true");
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({
          type: "Search",
          query: objectContaining({
            text: "is:unread",
            account_id: "acc-personal",
            folder_id: INBOX,
          }),
        }),
      );
    });
    await waitFor(() => {
      const rows = screen.getAllByRole("option");
      expect(rows.length).toBeGreaterThan(0);
      expect(rows.length).toBeLessThan(all.length);
      for (const row of rows) expect(row.getAttribute("data-state")).toContain("unread");
    });
    expect(screen.getByRole("listbox")).toHaveAttribute("data-filter", "unread");
    // No search summary for a filter (that is for typed searches).
    expect(document.querySelector('[data-part="message-list.search-summary"]')).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "All" }));
    await waitFor(() => {
      expect(screen.getAllByRole("option")).toHaveLength(all.length);
    });
    await settleQueries(env);
  });

  it("says so when no message matches a filter", async () => {
    const env = createEnv({
      state: { activeAccountId: "acc-personal", activeFolderId: "acc-personal/Drafts" },
    });
    env.store.getState().setListFilter("flagged");
    renderNode(widget("message-list"), env);
    expect(await screen.findByText("No messages match this filter.")).toHaveAttribute(
      "data-filter",
      "flagged",
    );
  });

  it("sets the chip with list.filter", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const layout: ResolvedLayout = normalizeLayout({
      ...env.transport.data.layout,
      toolbars: {
        t: [{ type: "action", action: "list.filter", args: ["attachments"], label: "Files" }],
      },
    });
    renderNode(widget("toolbar", { options: { toolbar: "t" } }), env, { layout });
    fireEvent.click(screen.getByRole("button", { name: "Files" }));
    await waitFor(() => {
      expect(env.store.getState().listFilter).toBe("attachments");
    });
  });
});

describe("dates follow the UI locale, not the browser's (regression)", () => {
  it("German browser, English UI: list rows and the reader show English dates", async () => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["de-DE", "de"]);
    vi.spyOn(navigator, "language", "get").mockReturnValue("de-DE");
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    env.store.getState().selectMessage(FIRST);
    renderWithProviders(
      <>
        <NodeView
          node={widget("message-list", { rows: ["{date | relative}", "{date | short-date}"] })}
          path="t/0"
        />
        <NodeView node={widget("message-view", { options: { headers: "from date" } })} path="t/1" />
      </>,
      env,
      { locale: "en" },
    );
    const row = must((await screen.findAllByRole("option"))[0]);
    const summary = env.transport.data.messages.find(
      (m) => m.id === row.getAttribute("data-message"),
    );
    const date = new Date(summary?.date ?? "");
    const now = Date.now();
    const [relative, short] = [...row.querySelectorAll('[data-part="message-list.row.date"]')].map(
      (d) => d.textContent,
    );
    expect(relative).toBe(formatRelative(date, now, "en"));
    expect(relative).not.toBe(formatRelative(date, now, "de"));
    expect(short).toBe(formatShortDate(date, now, "en"));
    const header = await screen.findByText(
      (_text, el) =>
        el?.getAttribute("data-part") === "message-view.header.value" &&
        el.parentElement?.getAttribute("data-field") === "date",
    );
    const body = env.transport.data.bodies.get(FIRST);
    const sent = body?.headers.date ?? "";
    expect(header).toHaveTextContent(formatDateTime(sent, "en", FULL_DATE_TIME));
    expect(header.textContent).not.toBe(formatDateTime(sent, "de", FULL_DATE_TIME));
    expect(document.documentElement.lang).toBe("en");
    await settleQueries(env);
  });
});

describe("reader (M5b)", () => {
  it("shows Saving…, then the outcome, in the always-present status region", async () => {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    env.store.getState().selectMessage(FIRST);
    renderNode(widget("message-view"), env);
    const save = await screen.findByRole("button", { name: /^Save / });
    const card = must(save.closest<HTMLElement>('[data-part="attachment-card"]'));
    const status = card.querySelector('[data-part="attachment-card.status"]');
    expect(status).toHaveAttribute("role", "status");
    expect(status).toHaveAttribute("data-state", "empty");
    expect(status).toBeEmptyDOMElement();
    fireEvent.click(save);
    expect(status).toHaveTextContent("Saving…");
    expect(card).toHaveAttribute("data-state", "saving");
    // jsdom cannot download (no object URLs): the failure replaces it.
    await waitFor(() => expect(status).toHaveTextContent("The attachment could not be saved."));
    expect(card).toHaveAttribute("data-state", "failed");
    expect(status?.querySelector('[data-part="attachment-card.status.icon"]')).toHaveAttribute(
      "data-icon",
      "circle-alert",
    );
  });
});

describe("axe in all five themes (M5b mail view)", () => {
  it.each(BUILTIN_THEMES.map(([id]) => id))(
    "%s: folder pane, list header, list, action bar with More open",
    async (id) => {
      const theme = resolveMockTheme(defaultTheme(), id, { density: "comfortable", overrides: {} });
      const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
      env.transport.data.features.customization.savedSearches.push({
        id: "s1",
        name: "From Grace",
        query: "from:grace",
        account_id: null,
        folder_id: null,
        sort: "date",
        position: 0,
      });
      env.store.getState().selectMessage(FIRST);
      const layout = normalizeLayout({
        ...env.transport.data.layout,
        toolbars: { ...env.transport.data.layout.toolbars, reader: PROPOSAL_READER },
        toolbar_settings: { reader: { hide_disabled: true, overflow: "more" } },
      });
      const { container, unmount } = renderWithProviders(
        <main>
          <NodeView node={PANE} path="t/0" />
          <NodeView node={widget("list-header")} path="t/1" />
          <NodeView node={widget("message-list")} path="t/2" />
          <NodeView node={widget("toolbar", { options: { toolbar: "reader" } })} path="t/3" />
        </main>,
        env,
        { theme, layout },
      );
      await screen.findByText("From Grace");
      await screen.findAllByRole("option");
      fireEvent.click(screen.getByRole("button", { name: "More" }));
      await screen.findByRole("menu");
      await settleQueries(env);
      expect(await violations(container)).toEqual([]);
      unmount();
    },
    // axe walks a large tree in jsdom.
    60_000,
  );
});
