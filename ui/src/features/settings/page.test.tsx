/**
 * The settings page widget (M5b WP163): grouped navigation, the shown
 * section remembered in the core's UI state, cards per widget, the search
 * with highlighted results and deep links, and the `settings.section` /
 * `settings.search` actions.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActionContext } from "../../actions/registry";
import type { ResolvedLayout } from "../../api/generated";
import {
  createEnv,
  onPlatform,
  renderNode,
  settleQueries,
  widget,
  type Env,
  stubElementSizesEachTest,
} from "../../test/harness";
import { UI_STATE_SETTINGS_SECTION } from "../../state/uiState";
import { actions } from ".";
import { settingsStoreFor } from "./store";
import { FOUND_ATTRIBUTE } from "./widgets";
import { must } from "../../test/must";

stubElementSizesEachTest();

const page = () => widget("settings-page");

async function uiState(env: Env): Promise<unknown> {
  const answer = await env.client.call({ type: "GetUiState" });
  return answer.type === "UiState" ? answer.state[UI_STATE_SETTINGS_SECTION] : undefined;
}

function actionContext(
  env: Env,
  layout: ResolvedLayout = env.transport.data.layout,
): ActionContext {
  return {
    client: env.client,
    store: env.store,
    queryClient: env.queryClient,
    layout,
    t: (id) => id,
    locale: "en",
  };
}

/** The widgets of the cards on the page, in order. */
const cardWidgets = (container: HTMLElement) =>
  [...container.querySelectorAll('[data-part="settings-page.card"]')].map((c) =>
    c.getAttribute("data-widget"),
  );
const nav = () => screen.getByRole("navigation", { name: "Settings sections" });
const title = () =>
  screen.getByRole("heading", {
    level: 2,
    name: (_n, el) => el.getAttribute("data-part") === "settings-page.title",
  });

/** The settings page once settled, and its search field. */
async function settingsSearch() {
  const env = createEnv();
  const user = userEvent.setup();
  renderNode(page(), env);
  await settleQueries(env);
  const search = screen.getByRole("searchbox", { name: "Search settings" });
  return { env, user, search };
}

describe("settings page (M5b)", () => {
  it("shows the groups and sections, and the first section when none is remembered", async () => {
    const env = createEnv();
    const { container } = renderNode(page(), env);
    await settleQueries(env);
    const groups = within(nav()).getAllByRole("group");
    expect(
      groups.map(
        (g) =>
          g.getAttribute("aria-labelledby") && within(g).getAllByRole("button")[0]?.textContent,
      ),
    ).toEqual(["Accounts", "Appearance", "Keys & encryption", "Agents (MCP)"]);
    expect(within(nav()).getByRole("group", { name: "Security" })).toBeInTheDocument();
    // A named search landmark: section widgets bring search landmarks too.
    expect(screen.getByRole("search", { name: "Search settings" })).toBeInTheDocument();
    const current = within(nav()).getByRole("button", { current: "page" });
    expect(current).toHaveTextContent("Accounts");
    expect(title()).toHaveTextContent("Accounts");
    expect(container.querySelector('[data-part="settings-page.breadcrumb"]')).toHaveTextContent(
      "General",
    );
    // Each widget of the section on its own card.
    const cards = container.querySelectorAll('[data-part="settings-page.card"]');
    expect([...cards].map((c) => c.getAttribute("data-widget"))).toEqual([
      "account-settings",
      "virtual-account-editor",
    ]);
    expect(cards[0]?.getAttribute("data-card-for")).toBe("page-account-settings");
  });

  it("switches sections and remembers the choice in the UI state", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    const { container } = renderNode(page(), env);
    await settleQueries(env);
    await user.click(within(nav()).getByRole("button", { name: "Notifications" }));
    expect(title()).toHaveTextContent("Notifications");
    expect(within(nav()).getByRole("button", { current: "page" })).toHaveTextContent(
      "Notifications",
    );
    expect(
      container.querySelector('[data-part="settings-page.card"]')?.getAttribute("data-widget"),
    ).toBe("notification-settings");
    await settleQueries(env);
    expect(await uiState(env)).toBe("notifications");
  });

  it("opens at the remembered section", async () => {
    const env = createEnv();
    await env.client.call({ type: "SetUiState", key: UI_STATE_SETTINGS_SECTION, value: "privacy" });
    renderNode(page(), env);
    await waitFor(() => expect(title()).toHaveTextContent("Privacy & data protection"));
    await settleQueries(env);
  });

  it("searches every label, highlights the matches and opens a result", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    const { container } = renderNode(page(), env);
    await settleQueries(env);
    const search = screen.getByRole("searchbox", { name: "Search settings" });
    await user.type(search, "quiet");
    expect(screen.getByRole("status")).toHaveTextContent("One section matches “quiet”.");
    const results = screen.getByRole("list", { name: "Search results" });
    const marks = [...results.querySelectorAll('[data-part="settings-page.mark"]')];
    expect(marks.map((m) => m.textContent.toLowerCase())).toContain("quiet");
    // A setting's highlighted label is one flex item of its button (the
    // marks must not split it into separately wrapping pieces).
    for (const setting of results.querySelectorAll('[data-part="settings-page.result.setting"]')) {
      expect([...setting.childNodes].map((n) => (n as Element).getAttribute("data-part"))).toEqual([
        "settings-page.result.label",
      ]);
    }
    // The groups give way to the results while searching.
    expect(within(nav()).queryByRole("group")).toBeNull();
    const section = within(results).getByRole("button", { name: "Notifications" });
    await user.click(section);
    expect(title()).toHaveTextContent("Notifications");
    expect(section).toHaveAttribute("aria-current", "page");
    // Clearing brings the groups back.
    await user.click(screen.getByRole("button", { name: "Clear the search" }));
    expect(within(nav()).getAllByRole("group")).toHaveLength(4);
    expect(search).toHaveFocus();
    expect(container.querySelector('[data-part="settings-page.results"]')).toBeNull();
    await settleQueries(env);
  });

  it("says when nothing matches, and Escape clears the search", async () => {
    const { user, search } = await settingsSearch();
    await user.type(search, "zzzz");
    expect(screen.getByRole("status")).toHaveTextContent("No settings match “zzzz”.");
    expect(screen.queryByRole("list", { name: "Search results" })).toBeNull();
    await user.keyboard("{Escape}");
    expect(search).toHaveValue("");
    expect(within(nav()).getAllByRole("group")).toHaveLength(4);
  });

  it("opens the best result with Enter and moves into the results with the down key", async () => {
    const { env, user, search } = await settingsSearch();
    await user.type(search, "plug-ins{Enter}");
    await waitFor(() => expect(title()).toHaveTextContent("Plug-ins"));
    await user.click(search);
    await user.keyboard("{ArrowDown}");
    const results = screen.getByRole("list", { name: "Search results" });
    expect(within(results).getAllByRole("button")[0]).toHaveFocus();
    await settleQueries(env);
  });

  it("reveals a setting from a result: scrolls to it, marks it and focuses its control", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const env = createEnv();
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime.bind(vi) });
      renderNode(page(), env);
      await settleQueries(env);
      await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "quiet hours");
      const results = screen.getByRole("list", { name: "Search results" });
      await user.click(within(results).getByRole("button", { name: "Quiet hours" }));
      expect(title()).toHaveTextContent("Notifications");
      // The widget loads its settings first; the page waits for the row.
      const found = await waitFor(() => {
        const el = document.querySelector<HTMLLabelElement>(`[${FOUND_ATTRIBUTE}]`);
        expect(el).not.toBeNull();
        return el as HTMLElement;
      });
      expect(found).toHaveTextContent("Quiet hours");
      expect(settingsStoreFor(env.store).getState().reveal).toBeNull();
      act(() => {
        vi.advanceTimersByTime(3000);
      });
      expect(found).not.toHaveAttribute(FOUND_ATTRIBUTE);
      await settleQueries(env);
    } finally {
      vi.useRealTimers();
    }
  });

  it("focuses the control of a revealed field label", async () => {
    const env = createEnv();
    renderNode(page(), env);
    await settleQueries(env);
    act(() => {
      settingsStoreFor(env.store).getState().requestReveal({
        section: "accounts",
        node: null,
        widget: "account-settings",
        text: "Account name",
      });
    });
    await waitFor(() => {
      expect(document.querySelector<HTMLLabelElement>(`[${FOUND_ATTRIBUTE}]`)).not.toBeNull();
    });
    const label = must(document.querySelector<HTMLLabelElement>(`[${FOUND_ATTRIBUTE}]`));
    expect(label.tagName).toBe("LABEL");
    expect(document.activeElement).toBe(document.getElementById(label.htmlFor));
    await settleQueries(env);
  });

  it("hides the search when the widget says so", async () => {
    const env = createEnv();
    renderNode(widget("settings-page", { options: { search: false } }), env);
    await settleQueries(env);
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(within(nav()).getAllByRole("group")).toHaveLength(4);
  });

  it("leaves out widgets the user hid", async () => {
    const env = createEnv();
    env.store.getState().toggleNode("page-virtual-accounts");
    const { container } = renderNode(page(), env);
    await settleQueries(env);
    expect(cardWidgets(container)).toEqual(["account-settings"]);
  });

  it.each([
    ["windows", ["update-settings", "desktop-settings", "default-apps"]],
    ["linux", ["update-settings", "desktop-settings"]],
  ] as const)("shows on %s only the desktop cards the system has (ADR 0202)", async (os, cards) => {
    const env = createEnv();
    if (os !== "windows") onPlatform(env, os);
    await env.client.call({
      type: "SetUiState",
      key: UI_STATE_SETTINGS_SECTION,
      value: "desktop",
    });
    const { container } = renderNode(page(), env);
    await settleQueries(env);
    expect(cardWidgets(container)).toEqual(cards);
  });

  it.each([
    ["windows", "One section matches “mapi”."],
    ["linux", "No settings match “mapi”."],
  ] as const)("finds on %s only what the system has (ADR 0202)", async (os, status) => {
    const env = createEnv();
    if (os !== "windows") onPlatform(env, os);
    const user = userEvent.setup();
    renderNode(page(), env);
    await settleQueries(env);
    await user.type(screen.getByRole("searchbox", { name: "Search settings" }), "mapi");
    expect(screen.getByRole("status")).toHaveTextContent(status);
  });

  it("explains a layout without settings sections", async () => {
    const env = createEnv();
    const layout = { ...env.transport.data.layout, settings: [] };
    renderNode(page(), env, { layout });
    await settleQueries(env);
    expect(screen.getByText("This layout's settings page has no sections.")).toBeInTheDocument();
  });
});

describe("settings actions (M5b)", () => {
  it("opens the page at a section, a former tab or a widget, revealing the widget", async () => {
    const env = createEnv();
    const ctx = actionContext(env);
    const action = actions["settings.section"];
    expect(action?.enabled?.(ctx, ["appearance"])).toBe(true);
    expect(action?.enabled?.(ctx, ["security-keys"])).toBe(true);
    expect(action?.enabled?.(ctx, ["nope"])).toBe(false);
    expect(action?.enabled?.(ctx, [])).toBe(false);
    await action?.run(ctx, ["security-smartcards"]);
    // The default layout's view host shows the settings view (WP165).
    expect(env.store.getState().windows).toEqual([]);
    expect(env.store.getState().activeView).toBe("settings");
    expect(settingsStoreFor(env.store).getState().reveal).toMatchObject({
      section: "keys",
      node: "page-security-smartcards",
      widget: "security-smartcards",
      text: null,
    });
    await vi.waitFor(async () => {
      expect(await uiState(env)).toBe("keys");
    });

    // Rendered, the page shows the section and reveals the widget's card.
    const { container } = renderNode(page(), env);
    expect(title()).toHaveTextContent("Keys & encryption");
    await waitFor(() =>
      expect(container.querySelector('[data-card-for="page-security-smartcards"]')).toHaveAttribute(
        FOUND_ATTRIBUTE,
      ),
    );
    expect(settingsStoreFor(env.store).getState().reveal).toBeNull();
    await settleQueries(env);

    // A section id alone reveals nothing.
    await action?.run(ctx, ["appearance"]);
    expect(settingsStoreFor(env.store).getState().reveal).toBeNull();
    await waitFor(() => expect(title()).toHaveTextContent("Appearance"));
    await settleQueries(env);
  });

  it("opens the settings window when the layout has no settings view", async () => {
    const env = createEnv();
    const layout = { ...env.transport.data.layout, views: {} };
    await actions["settings.section"]?.run(actionContext(env, layout), ["mail"]);
    expect(env.store.getState().windows).toEqual(["settings"]);
    await settleQueries(env);
  });

  it("focuses the search with settings.search", async () => {
    const env = createEnv();
    const ctx = actionContext(env);
    renderNode(page(), env);
    await settleQueries(env);
    const action = actions["settings.search"];
    expect(action?.enabled?.(ctx, [])).toBe(true);
    expect(action?.enabled?.(actionContext(env, { ...ctx.layout, settings: [] }), [])).toBe(false);
    act(() => {
      void action?.run(ctx, []);
    });
    expect(screen.getByRole("searchbox", { name: "Search settings" })).toHaveFocus();
    expect(env.store.getState().activeView).toBe("settings");
    expect(env.store.getState().windows).toEqual([]);
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "dark" } });
    expect(settingsStoreFor(env.store).getState().query).toBe("dark");
  });
});
