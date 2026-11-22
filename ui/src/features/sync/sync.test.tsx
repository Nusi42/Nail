import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
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
import { SYNC_FOLDER_DIALOG } from "./Sync";
import { syncStoreFor } from "./store";
import { requestsOf, actionContext } from "../../test/env";
import { renderSettings, saveBar, saveSettings, saveSettingsDone } from "../settings/testing";
import { withCloseGuard } from "../../test/closeGuard";
import { WindowLayer } from "../../layout/WindowLayer";
import { INBOX } from "../../test/mockIds";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
  failingBodies.clear();
});

function dispatcher(env: Env) {
  return createDispatcher(actionContext(env), { ...appRegistry.actions });
}

/** The personal Inbox with its first message downloaded without its body. */
function headersOnlyInbox() {
  const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  const message = env.transport.data.messages.find((m) => m.folder_id === INBOX);
  if (!message) throw new Error("no inbox message");
  message.headers_only = true;
  return { env, message };
}

describe("sync-folder-policies", () => {
  it("writes a folder's body policy and removes it again", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderSettings(env, "sync-folder-policies");
    const section = await screen.findByRole("region", { name: "Synchronization per folder" });
    // The folder is a folder picker: type to search the tree, Enter picks.
    const folder = await within(section).findByRole("combobox", { name: "Folder" });
    await within(section).findByDisplayValue("Inbox");
    await user.clear(folder);
    await user.type(folder, "Arch");
    await within(section).findByRole("treeitem", { name: /Archive/ });
    await user.keyboard("{Enter}");
    expect(folder).toHaveValue("Archive");
    const status = await within(section).findByText(/Now: Full messages; synchronized\./);
    expect(status).toBeVisible();
    await user.selectOptions(within(section).getByLabelText("Download"), "recent");
    const days = within(section).getByLabelText("Days to keep in full");
    await user.clear(days);
    // R6: nothing is written yet; an invalid draft cannot be saved.
    expect(await within(section).findByRole("alert")).toHaveTextContent("at least 1 day");
    const bar = saveBar();
    expect(bar && within(bar).getByRole("button", { name: "Save" })).toBeDisabled();
    expect(requestsOf(env, "SetFolderSyncPolicy")).toEqual([]);
    await user.type(days, "14");
    await user.selectOptions(within(section).getByLabelText(/Watch for new mail/), "yes");
    expect(within(section).queryByRole("button", { name: "Save" })).toBeNull();
    await saveSettingsDone(user);
    expect(await within(section).findByText("Saved to accounts.kdl.")).toBeVisible();
    expect(requestsOf(env, "SetFolderSyncPolicy")[0]).toEqual({
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
    await saveSettings(user);
    await waitFor(() => {
      expect(requestsOf(env, "SetFolderSyncPolicy")[1]).toMatchObject({
        bodies: null,
        enabled: null,
        idle: null,
      });
    });
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
    withCloseGuard(env);
    renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
    const dialog = await screen.findByRole("dialog", { name: "Folder synchronization" });
    // R6: Cancel with a change asks; Keep editing goes back to the dialog.
    await user.selectOptions(await within(dialog).findByLabelText("Download"), "headers-only");
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    const prompt = await screen.findByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(prompt).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([SYNC_FOLDER_DIALOG]);
    });
    expect(requestsOf(env, "SetFolderSyncPolicy")).toEqual([]);
    // Save writes and closes.
    await user.click(within(dialog).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(requestsOf(env, "SetFolderSyncPolicy")[0]).toMatchObject({
        folder_id: INBOX,
        bodies: { mode: "headers-only" },
      });
    });
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
  });

  it("asks before another folder shows while one has unsaved changes", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderSettings(env, "sync-folder-policies");
    const section = await screen.findByRole("region", { name: "Synchronization per folder" });
    await within(section).findByDisplayValue("Inbox");
    await user.selectOptions(await within(section).findByLabelText("Download"), "headers-only");
    const folder = within(section).getByRole("combobox", { name: "Folder" });
    await user.clear(folder);
    await user.type(folder, "Arch");
    await within(section).findByRole("treeitem", { name: /Archive/ });
    await user.keyboard("{Enter}");
    const prompt = await screen.findByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(prompt).getByRole("button", { name: "Don't save" }));
    await waitFor(() => {
      expect(folder).toHaveValue("Archive");
    });
    expect(saveBar()).toBeNull();
    expect(requestsOf(env, "SetFolderSyncPolicy")).toEqual([]);
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
    const { env, message } = headersOnlyInbox();
    env.store.getState().selectMessage(message.id);
    const { container } = renderWithProviders(
      <>
        <NodeView node={widget("message-list")} path="t/0" />
        <NodeView node={widget("sync-message-body")} path="t/1" />
      </>,
      env,
    );
    await waitFor(() => {
      expect(
        container.querySelector(`[data-message="${message.id}"][data-state~="headers-only"]`),
      ).not.toBeNull();
    });
    const bar = await screen.findByRole("status", { name: "Message download" });
    expect(bar).toHaveTextContent("Only the headers");
    expect(dispatcher(env).isEnabled("sync.download-message")).toBe(true);
    await user.click(within(bar).getByRole("button", { name: "Download full message" }));
    await waitFor(() => {
      expect(screen.queryByRole("status", { name: "Message download" })).toBeNull();
    });
    expect(requestsOf(env, "FetchMessageBodies")).toEqual([
      { type: "FetchMessageBodies", message_ids: [message.id] },
    ]);
  });

  it("says when the download failed", async () => {
    const user = userEvent.setup();
    const { env, message } = headersOnlyInbox();
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
