/**
 * Accessibility pass (M2c WP54): axe-core over every window of the default
 * layout (every tab of tabbed windows), every feature dialog, every widget
 * of the M2c features, the context menu and the login page, as rendered by
 * jsdom. Colour contrast needs a real layout engine and is checked by the
 * Playwright scan (`e2e/tests/a11y.spec.ts`) instead.
 */
import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "./test/axe";
import type { LayoutNode, ResolvedLayout } from "./api/generated";
import { emptyComposer } from "./state/compose";
import { appRegistry } from "./features/registry";
import { ContextMenuHost } from "./features/menus/ContextMenuHost";
import { RemoteLogin } from "./features/remote/Login";
import { MockAuthServer } from "./features/remote/mockAuth";
import { fakePasskeys } from "./features/remote/testUtil";
import { loadHtmlEditor, loadMarkdownEditor } from "./features/composer/lazyEditors";
import { richStoreFor } from "./features/composer/store";
import { triageStoreFor } from "./features/triage/store";
import { syncStoreFor } from "./features/sync/store";
import { NodeView, WindowView } from "./layout/LayoutRenderer";
import { WindowLayer, tabbables } from "./layout/WindowLayer";
import { compileTheme } from "./theme/compile";
import { createEnv, renderWithProviders, stubElementSizes, widget, type Env } from "./test/harness";

const INBOX = "acc-personal/Inbox";

let restoreSizes: () => void;
beforeAll(() => Promise.all([loadHtmlEditor(), loadMarkdownEditor()]));
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => restoreSizes());

/** Waits until queries settled (nothing busy) and effects ran. */
async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

function mailEnv(): Env {
  const env = createEnv({
    state: {
      activeAccountId: "acc-personal",
      activeFolderId: INBOX,
      composer: { ...emptyComposer(), identityId: "ada@example.org" },
    },
  });
  const work = env.transport.data.accountsConfig.accounts.find((a) => a.id === "acc-work");
  if (work) work.sieve = { enabled: null, host: null, port: null, tls: null, script_name: null };
  const first = env.transport.data.messages.find((m) => m.folder_id === INBOX);
  if (first) env.store.getState().selectMessage(first.id);
  return env;
}

/** Tab containers of a window, with the number of tabs each has. */
function tabsOf(node: LayoutNode, path: string, out: { key: string; count: number }[]) {
  if (node.type === "widget") return out;
  if (node.type === "tabs") out.push({ key: node.id ?? path, count: node.children.length });
  node.children.forEach((child, index) => tabsOf(child, `${path}/${index}`, out));
  return out;
}

describe("axe: windows of the default layout", () => {
  const layout = createEnv().transport.data.layout;
  const names = Object.keys(layout.windows).sort();
  /** Every window, and every further tab of its tab containers. */
  const cases: [string, string, number][] = names.flatMap((name) => {
    const root = layout.windows[name]?.[0]?.root;
    const tabs = root ? tabsOf(root, `${name}/0`, []) : [];
    return [
      [name, "", 0] as [string, string, number],
      ...tabs.flatMap(({ key, count }) =>
        Array.from({ length: count - 1 }, (_, i) => [name, key, i + 1] as [string, string, number]),
      ),
    ];
  });

  it("covers the windows", () => {
    expect(names).toEqual(
      expect.arrayContaining(["main", "mail", "composer", "outbox", "move", "settings", "rules"]),
    );
    // M5b: the settings window is the settings page (no tabs any more); the
    // page's sections are scanned in features/settings/a11y.test.tsx.
    expect(cases.length).toBeGreaterThanOrEqual(names.length);
  });

  it.each(cases)(
    "window %s (tabs %s #%i) has no violations",
    async (name, tabKey, tab) => {
      const env = mailEnv();
      if (tabKey !== "") env.store.getState().setActiveTab(tabKey, tab);
      const { container } = renderWithProviders(<WindowView layout={layout} name={name} />, env);
      await settle(env);
      // Only the shown tab: hidden panels are out of the accessibility tree anyway.
      const scope =
        tabKey === ""
          ? container
          : ([...container.querySelectorAll('[role="tabpanel"]:not([hidden])')].at(-1) ??
            container);
      expect(await violations(scope)).toEqual([]);
    },
    60_000,
  );
});

describe("axe: dialogs", () => {
  const dialogs = Object.keys(appRegistry.dialogs).sort();

  it.each(dialogs)("dialog %s has no violations", async (name) => {
    const env = mailEnv();
    const selected = env.store.getState().selectedMessageId;
    triageStoreFor(env.store)
      .getState()
      .setSnoozing(selected === null ? [] : [selected]);
    syncStoreFor(env.store).getState().setFolder(INBOX);
    const base = env.transport.data.layout;
    // Without a layout window of that name, so the feature dialog renders.
    const layout: ResolvedLayout = {
      ...base,
      windows: Object.fromEntries(Object.entries(base.windows).filter(([n]) => n !== name)),
    };
    act(() => env.store.getState().openWindow(name));
    const { container } = renderWithProviders(<WindowLayer layout={layout} />, env, { layout });
    await settle(env);
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("axe: M2c widgets", () => {
  const kinds = [
    "sieve-status",
    "vacation-editor",
    "junk-settings",
    "junk-verdict",
    "snooze-list",
    "notification-settings",
    "sync-folder-policies",
    "sync-message-body",
    "toolbar-editor",
    "key-binding-editor",
    "column-editor",
  ];

  it.each(kinds)(
    "%s has no violations",
    async (kind) => {
      const env = mailEnv();
      const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
      if (message) {
        message.headers_only = true;
        await env.client.request(
          { type: "MarkJunk", message_ids: [message.id], junk: true },
          "Queued",
        );
        await env.client.request(
          {
            type: "SnoozeMessages",
            message_ids: [message.id],
            until: new Date(Date.now() + 86_400_000).toISOString(),
            wake_folder_id: null,
          },
          "Queued",
        );
        env.store.getState().selectMessage(message.id);
      }
      const { container } = renderWithProviders(
        <>
          <NodeView node={widget("message-list")} path="t/0" />
          <NodeView node={widget(kind)} path="t/1" />
        </>,
        env,
      );
      await settle(env);
      expect(container.querySelector("[data-part]")).not.toBeNull();
      expect(await violations(container)).toEqual([]);
      // The editors are large trees for axe in jsdom: past 20 s under load.
    },
    60_000,
  );
});

describe("axe: context menu, composer modes and login", () => {
  it("the open context menu has no violations", async () => {
    const env = mailEnv();
    const { container } = renderWithProviders(
      <>
        <NodeView node={widget("message-list")} path="t/0" />
        <ContextMenuHost />
      </>,
      env,
    );
    await waitFor(() =>
      expect(container.querySelector('[data-part="message-list.row"]')).not.toBeNull(),
    );
    fireEvent.contextMenu(container.querySelector('[data-part="message-list.row"]') as Element, {
      clientX: 10,
      clientY: 10,
      button: 2,
    });
    await screen.findByRole("menu");
    expect(await violations(container)).toEqual([]);
  });

  it.each(["html", "markdown"] as const)("the %s composer has no violations", async (mode) => {
    const env = mailEnv();
    richStoreFor(env.store).setState({ mode, html: "<p>Hi</p>", markdown: "Hi" });
    const { container } = renderWithProviders(<NodeView node={widget("composer")} path="t" />, env);
    await settle(env);
    expect(await violations(container)).toEqual([]);
  });

  it("the login page (pairing) has no violations", async () => {
    const env = createEnv();
    const browser = new MockAuthServer(env.transport);
    const { container } = renderWithProviders(
      <RemoteLogin auth={browser} passkeys={fakePasskeys()} onLoggedIn={() => {}} />,
      env,
    );
    await screen.findByRole("form", { name: "Pair this browser" });
    expect(await violations(container)).toEqual([]);
  });
});

describe("keyboard and motion", () => {
  it("keeps Tab inside the active window and gives focus back on close", async () => {
    const user = userEvent.setup();
    const env = mailEnv();
    const layout = env.transport.data.layout;
    const opener = document.createElement("button");
    document.body.append(opener);
    opener.focus();
    act(() => env.store.getState().openWindow("outbox"));
    renderWithProviders(<WindowLayer layout={layout} />, env);
    const dialog = await screen.findByRole("dialog");
    await settle(env);
    const items = tabbables(dialog);
    expect(items.length).toBeGreaterThan(0);
    const first = items[0] as HTMLElement;
    const last = items[items.length - 1] as HTMLElement;
    last.focus();
    await user.tab();
    expect(first).toHaveFocus();
    await user.tab({ shift: true });
    expect(last).toHaveFocus();
    act(() => env.store.getState().closeWindow("outbox"));
    expect(opener).toHaveFocus();
    opener.remove();
  });

  it("moves along a toolbar with the arrow keys", async () => {
    const user = userEvent.setup();
    const env = mailEnv();
    renderWithProviders(
      <NodeView node={widget("toolbar", { options: { toolbar: "main" } })} path="t" />,
      env,
    );
    const toolbar = screen.getByRole("toolbar");
    const buttons = [...toolbar.querySelectorAll<HTMLButtonElement>("button:not(:disabled)")];
    expect(buttons.length).toBeGreaterThan(2);
    buttons[0]?.focus();
    await user.keyboard("{ArrowRight}");
    expect(buttons[1]).toHaveFocus();
    await user.keyboard("{End}");
    expect(buttons.at(-1)).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(buttons[0]).toHaveFocus();
    await user.keyboard("{ArrowLeft}");
    expect(buttons.at(-1)).toHaveFocus();
  });

  it("stops animations when the system asks for reduced motion", () => {
    const { css } = compileTheme(createEnv().transport.data.theme);
    expect(css).toContain("@media (prefers-reduced-motion: reduce)");
    expect(css).toMatch(/transition-duration: 0\.01ms !important/);
  });
});
