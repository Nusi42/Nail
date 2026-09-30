import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Request } from "../../api/generated";
import { createDispatcher } from "../../actions/registry";
import { NodeView } from "../../layout/LayoutRenderer";
import {
  createEnv,
  renderWithProviders,
  stubElementSizes,
  widget,
  type Env,
} from "../../test/harness";
import { appRegistry } from "../registry";
import { rowStates } from "../../widgets/MessageList";
import { failingBodies } from "./mock";
import { SYNC_FOLDER_DIALOG, SyncFolderDialog } from "./Sync";
import { syncStoreFor } from "./store";

const INBOX = "acc-personal/Inbox";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
  failingBodies.clear();
});

function sent<T extends Request["type"]>(env: Env, type: T): Extract<Request, { type: T }>[] {
  return env.transport.requests.filter((r): r is Extract<Request, { type: T }> => r.type === type);
}

function dispatcher(env: Env) {
  return createDispatcher(
    {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      layout: env.transport.data.layout,
      t: (id) => id,
      locale: "en",
    },
    { ...appRegistry.actions },
  );
}

describe("sync-folder-policies", () => {
  it("writes a folder's body policy and removes it again", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<NodeView node={widget("sync-folder-policies")} path="t" />, env);
    const section = await screen.findByRole("region", { name: "Synchronization per folder" });
    await within(section).findByRole("option", { name: /Archive/ });
    await user.selectOptions(within(section).getByLabelText("Folder"), "acc-personal/Archive");
    const status = await within(section).findByText(/Now: Full messages; synchronized\./);
    expect(status).toBeVisible();
    await user.selectOptions(within(section).getByLabelText("Download"), "recent");
    const days = within(section).getByLabelText("Days to keep in full");
    await user.clear(days);
    await user.click(within(section).getByRole("button", { name: "Save" }));
    expect(await within(section).findByRole("alert")).toHaveTextContent("at least 1 day");
    await user.type(days, "14");
    await user.selectOptions(within(section).getByLabelText(/Watch for new mail/), "yes");
    await user.click(within(section).getByRole("button", { name: "Save" }));
    expect(await within(section).findByText("Saved to accounts.kdl.")).toBeVisible();
    expect(sent(env, "SetFolderSyncPolicy")[0]).toEqual({
      type: "SetFolderSyncPolicy",
      folder_id: "acc-personal/Archive",
      bodies: { mode: "recent", days: 14 },
      enabled: null,
      idle: true,
    });
    expect(
      await within(section).findByText(/Full messages of the last 14 days; synchronized, watched/),
    ).toBeVisible();
    // "As the account" everywhere removes the folder's rule.
    await user.selectOptions(within(section).getByLabelText("Download"), "default");
    await user.selectOptions(within(section).getByLabelText(/Watch for new mail/), "default");
    await user.click(within(section).getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(sent(env, "SetFolderSyncPolicy")[1]).toMatchObject({
        bodies: null,
        enabled: null,
        idle: null,
      }),
    );
    const account = env.transport.data.accountsConfig.accounts.find((a) => a.id === "acc-personal");
    expect(account?.sync.folders).toEqual([]);
  });

  it("opens the folder dialog from the folder's menu target or the active folder", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const actions = dispatcher(env);
    expect(actions.isEnabled("sync.folder-policy")).toBe(true);
    await actions.dispatch("sync.folder-policy");
    expect(env.store.getState().windows).toEqual([SYNC_FOLDER_DIALOG]);
    expect(syncStoreFor(env.store).getState().folderId).toBe(INBOX);
    renderWithProviders(<SyncFolderDialog />, env);
    await user.selectOptions(await screen.findByLabelText("Download"), "headers-only");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(sent(env, "SetFolderSyncPolicy")[0]).toMatchObject({
        folder_id: INBOX,
        bodies: { mode: "headers-only" },
      }),
    );
  });
});

describe("bodies on demand", () => {
  it("marks headers-only rows", () => {
    const message = createEnv().transport.data.messages[0];
    if (!message) throw new Error("no message");
    expect(rowStates({ ...message, headers_only: true }, false)).toContain("headers-only");
    expect(rowStates({ ...message, headers_only: false }, false) ?? "").not.toContain(
      "headers-only",
    );
  });

  it("downloads the full message from the reader bar", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    message.headers_only = true;
    env.store.getState().selectMessage(message.id);
    const { container } = renderWithProviders(
      <>
        <NodeView node={widget("message-list")} path="t/0" />
        <NodeView node={widget("sync-message-body")} path="t/1" />
      </>,
      env,
    );
    await waitFor(() =>
      expect(
        container.querySelector(`[data-message="${message.id}"][data-state~="headers-only"]`),
      ).not.toBeNull(),
    );
    const bar = await screen.findByRole("status", { name: "Message download" });
    expect(bar).toHaveTextContent("Only the headers");
    expect(dispatcher(env).isEnabled("sync.download-message")).toBe(true);
    await user.click(within(bar).getByRole("button", { name: "Download full message" }));
    await waitFor(() =>
      expect(screen.queryByRole("status", { name: "Message download" })).toBeNull(),
    );
    expect(sent(env, "FetchMessageBodies")).toEqual([
      { type: "FetchMessageBodies", message_ids: [message.id] },
    ]);
  });

  it("says when the download failed", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
    if (!message) throw new Error("no inbox message");
    message.headers_only = true;
    failingBodies.add(message.id);
    env.store.getState().selectMessage(message.id);
    renderWithProviders(
      <>
        <NodeView node={widget("message-list")} path="t/0" />
        <NodeView node={widget("sync-message-body")} path="t/1" />
      </>,
      env,
    );
    const bar = await screen.findByRole("status", { name: "Message download" });
    await user.click(within(bar).getByRole("button", { name: "Download full message" }));
    expect(await within(bar).findByText(/could not be downloaded/)).toBeVisible();
    expect(bar).toHaveAttribute("data-state", "failed");
  });
});
