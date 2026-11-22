/**
 * ADR 0196: folder management in the UI — what the actions allow, the
 * windows (create, rename, move, delete for good) against the mock core,
 * and waiting for a queued write's outcome.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Folder, OpStatus } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { WindowLayer } from "../../layout/WindowLayer";
import { queryKeys } from "../../state/queries";
import { outcomeOf } from "../../state/queuedOps";
import { withCloseGuard } from "../../test/closeGuard";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { appRegistry } from "../registry";
import { canManage, deleteMode, isInside, moveTargets, subfoldersOf } from "./model";
import {
  FOLDER_DELETE_ACTION,
  FOLDER_DELETE_DIALOG,
  FOLDER_MOVE_ACTION,
  FOLDER_NEW_ACTION,
  FOLDER_RENAME_ACTION,
} from "./names";

const WORK = "acc-work";
const RECEIPTS = "acc-work/Inbox/Receipts";

function folder(id: string, parent: string | null, role: Folder["role"] = "other"): Folder {
  return {
    id,
    account_id: WORK,
    parent_id: parent,
    name: id,
    path: id,
    role,
    unread: 0,
    total: 2,
  };
}

/** The mock's folders and accounts in the query cache, as the app loads them. */
function loaded(env: Env): void {
  env.queryClient.setQueryData(queryKeys.accounts, env.transport.data.accounts);
  env.queryClient.setQueryData(
    queryKeys.folders(WORK),
    env.transport.data.folders.filter((f) => f.account_id === WORK),
  );
}

function ctxOf(env: Env): ActionContext {
  return {
    store: env.store,
    queryClient: env.queryClient,
    client: env.client,
  } as unknown as ActionContext;
}

function mockFolder(env: Env, path: string): Folder | undefined {
  return env.transport.data.folders.find((f) => f.account_id === WORK && f.path === path);
}

/** The app's windows on the mock core, with Receipts (Work) active. */
function withWindows() {
  const env = createEnv({ state: { activeAccountId: WORK, activeFolderId: RECEIPTS } });
  loaded(env);
  const user = userEvent.setup({ delay: null });
  withCloseGuard(env);
  renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
  return { env, user };
}

/** Runs New folder… and returns its name window. */
function newFolderWindow(env: Env): HTMLElement {
  act(() => {
    void must(appRegistry.actions[FOLDER_NEW_ACTION]).run(ctxOf(env), []);
  });
  return windowOf("folder-name");
}

function windowOf(name: string): HTMLElement {
  return must(document.querySelector<HTMLElement>(`[data-part="window"][data-window="${name}"]`));
}

describe("folder model", () => {
  const trash = folder("trash", null, "trash");
  const a = folder("a", null);
  const b = folder("b", "a");
  const old = folder("old", "trash");
  const all = [trash, a, b, old];

  it("finds subfolders, the delete mode and the move targets", () => {
    expect(isInside(all, b, "a")).toBe(true);
    expect(isInside(all, a, "b")).toBe(false);
    expect(subfoldersOf(all, a)).toEqual([b]);
    expect(deleteMode(all, a)).toBe("trash");
    expect(deleteMode(all, old)).toBe("permanent");
    expect(deleteMode([a, b], a)).toBe("permanent");
    expect(moveTargets(all, a).map((f) => f.id)).toEqual(["trash", "old"]);
    expect(canManage("imap")).toBe(true);
    expect(canManage("gmail")).toBe(false);
    expect(canManage(null)).toBe(false);
  });

  it("decides a queued write's outcome from its op statuses", () => {
    const status = (op_id: string, state: OpStatus["state"], error: OpStatus["error"] = null) => ({
      op_id,
      account_id: WORK,
      state,
      error,
      message: null,
    });
    const statuses = new Map<string, OpStatus>([["op-1", status("op-1", "in-flight")]]);
    expect(outcomeOf(["op-1"], statuses)).toBeNull();
    statuses.set("op-1", status("op-1", "pending", "network"));
    expect(outcomeOf(["op-1"], statuses)).toBe("waiting");
    statuses.set("op-1", status("op-1", "done"));
    expect(outcomeOf(["op-1"], statuses)).toBe("done");
    expect(outcomeOf([], statuses)).toBe("done");
    statuses.set("op-2", status("op-2", "failed", "conflict"));
    expect(outcomeOf(["op-1", "op-2"], statuses)).toMatchObject({ op_id: "op-2" });
  });
});

describe("folder actions and windows", () => {
  it("allow changes to own folders only, never to system folders or Gmail", () => {
    const env = createEnv({ state: { activeAccountId: WORK, activeFolderId: RECEIPTS } });
    loaded(env);
    const ctx = ctxOf(env);
    const enabled = (id: string) => must(appRegistry.actions[id]).enabled?.(ctx, []);
    expect(enabled(FOLDER_NEW_ACTION)).toBe(true);
    expect(enabled(FOLDER_RENAME_ACTION)).toBe(true);
    expect(enabled(FOLDER_MOVE_ACTION)).toBe(true);
    act(() => {
      env.store.getState().selectFolder(`${WORK}/Inbox`);
    });
    expect(enabled(FOLDER_RENAME_ACTION)).toBe(false);
    expect(enabled(FOLDER_DELETE_ACTION)).toBe(false);
    const gmail = { ...must(env.transport.data.accounts[1]), kind: "gmail" as const };
    env.queryClient.setQueryData(queryKeys.accounts, [env.transport.data.accounts[0], gmail]);
    expect(enabled(FOLDER_NEW_ACTION)).toBe(false);
    expect(appRegistry.actionInfo[FOLDER_NEW_ACTION]?.icon).toBe("folder-plus");
    expect(appRegistry.windows[FOLDER_DELETE_DIALOG]).toEqual({ confirmation: true });
  });

  it("creates and renames a folder, and shows the core's reason for a refusal", async () => {
    const { env, user } = withWindows();
    const create = newFolderWindow(env);
    expect(within(create).getByText("New folder in Work")).toBeInTheDocument();
    await user.type(within(create).getByRole("textbox", { name: "Name" }), "Archive");
    await user.click(within(create).getByRole("button", { name: "Create" }));
    expect(
      await within(create).findByText("A folder with this name exists there already."),
    ).toBeInTheDocument();
    const input = within(create).getByRole("textbox", { name: "Name" });
    await user.clear(input);
    await user.type(input, "Projects");
    await user.click(within(create).getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(document.querySelector('[data-window="folder-name"]')).toBeNull();
    });
    expect(mockFolder(env, "Projects")?.parent_id).toBeNull();

    loaded(env);
    act(() => {
      void must(appRegistry.actions[FOLDER_RENAME_ACTION]).run(ctxOf(env), []);
    });
    const rename = windowOf("folder-name");
    expect(within(rename).getByText("Rename “Receipts”")).toBeInTheDocument();
    const name = within(rename).getByRole("textbox", { name: "Name" });
    await user.clear(name);
    await user.type(name, "Bills");
    await user.click(within(rename).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(mockFolder(env, "Inbox/Bills")?.id).toBe(RECEIPTS);
    });
  });

  it("asks before a typed folder name is lost (R6 close guard)", async () => {
    const { env, user } = withWindows();
    const create = newFolderWindow(env);
    const footer = must(create.querySelector('[data-part="dialog.footer"]'));
    expect([...footer.querySelectorAll("button")].map((b) => b.getAttribute("data-role"))).toEqual([
      "primary",
      "cancel",
    ]);
    // Nothing typed: Cancel closes at once.
    await user.click(within(create).getByRole("button", { name: "Cancel" }));
    expect(document.querySelector('[data-window="folder-name"]')).toBeNull();

    const again = newFolderWindow(env);
    await user.type(within(again).getByRole("textbox", { name: "Name" }), "Plans");
    await user.click(within(again).getByRole("button", { name: "Cancel" }));
    const prompt = await waitFor(() => windowOf("unsaved-changes:folder-name"));
    expect(within(prompt).getByText("Save changes to “Plans”?")).toBeInTheDocument();
    await user.click(within(prompt).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(mockFolder(env, "Plans")?.parent_id).toBeNull();
    });
    await waitFor(() => {
      expect(document.querySelector('[data-window="folder-name"]')).toBeNull();
    });
    expect(document.querySelector('[data-window^="unsaved-changes:"]')).toBeNull();
  });

  it("moves a folder and deletes it: into Trash, then for good after asking", async () => {
    const { env, user } = withWindows();
    act(() => {
      void must(appRegistry.actions[FOLDER_MOVE_ACTION]).run(ctxOf(env), []);
    });
    const move = windowOf("folder-move");
    await user.click(within(move).getByRole("button", { name: "Archive" }));
    await waitFor(() => {
      expect(mockFolder(env, "Archive/Receipts")?.id).toBe(RECEIPTS);
    });

    loaded(env);
    await act(async () => {
      await must(appRegistry.actions[FOLDER_DELETE_ACTION]).run(ctxOf(env), []);
    });
    expect(mockFolder(env, "Trash/Receipts")?.id).toBe(RECEIPTS);

    loaded(env);
    act(() => {
      void must(appRegistry.actions[FOLDER_DELETE_ACTION]).run(ctxOf(env), []);
    });
    const confirm = windowOf(FOLDER_DELETE_DIALOG);
    expect(within(confirm).getByText("Delete the folder “Receipts” for good?")).toBeInTheDocument();
    expect(
      within(confirm).getByText(
        "It holds 3 messages. It is deleted on the server too. This cannot be undone.",
      ),
    ).toBeInTheDocument();
    // R6: the red verb is never the default — Cancel has the focus, Enter deletes nothing.
    const cancel = within(confirm).getByRole("button", { name: "Cancel" });
    await waitFor(() => {
      expect(cancel).toHaveFocus();
    });
    const verb = within(confirm).getByRole("button", { name: "Delete folder" });
    expect(verb).toHaveAttribute("data-variant", "danger");
    expect(verb).toHaveAttribute("type", "button");
    await user.click(verb);
    await waitFor(() => {
      expect(env.transport.data.folders.some((f) => f.id === RECEIPTS)).toBe(false);
    });
    await waitFor(() => {
      expect(screen.queryByText(/for good, on the server too\?/)).toBeNull();
    });
  });
});
