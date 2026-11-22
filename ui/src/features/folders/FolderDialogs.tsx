/**
 * The folder windows (ADR 0196): the name of a new or renamed folder
 * (`folder-name`), where to move one (`folder-move`) and the question
 * before deleting one for good (`folder-delete`). Each sends its request
 * and stays open until the server confirmed the change (`runQueued`); a
 * refusal shows the core's text, and while the server cannot be reached
 * the window says the change waits (and shows one Done).
 *
 * R6 (ADR 0239): the name window is a `DialogForm` with the shared footer
 * (Create / Save, Cancel) and a close guard (a changed name asks before it
 * is lost); the move window is a picker (a pick moves, × cancels); the
 * delete question is a destructive `ConfirmDialog` ("Delete folder", the
 * focus on Cancel, Enter never deletes).
 */
import { useState, type ReactNode } from "react";
import type { Folder, LocalizedMessage, Request } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useBusyAction } from "../../state/busyAction";
import { useAccounts, useClient, useFolders } from "../../state/queries";
import { runQueued } from "../../state/queuedOps";
import { useUiStoreApi } from "../../state/uiStore";
import { ConfirmDialog, DialogFooter, DialogForm, useCloseGuard } from "../../widgets/dialog";
import { EmptyDialog, LoadingDialog, useCloseNow } from "../../widgets/DialogStates";
import { FolderList } from "../../widgets/FolderList";
import { flattenFolders } from "../../widgets/FolderTree";
import { TextField } from "../../widgets/forms";
import { moveTargets, subfoldersOf } from "./model";
import { FOLDER_DIALOG_PART } from "./names";
import { foldersStoreFor, useFolderTask, type FolderTask } from "./store";

/** Sends a folder request; closes on success, says so when it waits. */
function useFolderRequest(close: () => void) {
  const client = useClient();
  const store = useUiStoreApi();
  const closeNow = useCloseNow(close);
  const { busy, problem, setProblem, run } = useBusyAction();
  const [waiting, setWaiting] = useState(false);
  const finish = () => {
    foldersStoreFor(store).getState().setTask(null);
    closeNow();
  };
  /** Sends `request`: `true` when it is done or waits for the server. */
  const send = (request: Request) =>
    run(async () => {
      const outcome = await runQueued(client, request);
      if (outcome === "done") finish();
      else setWaiting(true);
    });
  return { busy, problem, setProblem, waiting, send, finish };
}

function Problem({ message }: { message: LocalizedMessage | null }) {
  const t = useT();
  if (message === null) return null;
  return (
    <p data-part="form.hint" data-tone="danger" role="alert">
      {t(message.id, message.args)}
    </p>
  );
}

/** The change waits for the server: the note and one Done. */
function Waiting({ finish }: { finish: () => void }) {
  const t = useT();
  return (
    <>
      <p data-part="folder-dialog.note" role="status">
        {t("folders-waiting")}
      </p>
      <DialogFooter done={{ onDone: finish }} />
    </>
  );
}

/** The window's folder is gone (deleted elsewhere, or never chosen). */
function Gone({ close }: { close: () => void }) {
  return (
    <EmptyDialog
      part={FOLDER_DIALOG_PART}
      textPart="folder-dialog.note"
      hintId="folders-gone"
      close={close}
    />
  );
}

function Loading() {
  return <LoadingDialog part={FOLDER_DIALOG_PART} textPart="folder-dialog.note" />;
}

/**
 * Renders `render` with the task's folder and its account's folders once
 * loaded; a task of another kind or a folder that is gone says so.
 */
function WithFolder({
  kind,
  close,
  render,
}: {
  kind: "rename" | "move" | "delete";
  close: () => void;
  render: (folder: Folder, folders: Folder[]) => ReactNode;
}) {
  const task = useFolderTask();
  const folders = useFolders(task?.accountId ?? null);
  if (task?.kind !== kind) return <Gone close={close} />;
  if (!folders.data) return <Loading />;
  const folder = folders.data.find((f) => f.id === task.folderId);
  if (!folder) return <Gone close={close} />;
  return render(folder, folders.data);
}

/** Window `folder-name`: a new folder's name, or a folder's new name. */
export function FolderNameDialog({ close }: DialogProps) {
  const task = useFolderTask();
  const folders = useFolders(task?.kind === "create" ? task.accountId : null);
  if (task?.kind === "rename") {
    return (
      <WithFolder
        kind="rename"
        close={close}
        render={(folder) => (
          <NameForm key={folder.id} task={task} folder={folder} parent={null} close={close} />
        )}
      />
    );
  }
  if (task?.kind !== "create") return <Gone close={close} />;
  if (!folders.data) return <Loading />;
  const parent =
    task.parentId === null ? null : (folders.data.find((f) => f.id === task.parentId) ?? null);
  return <NameForm key="new" task={task} folder={null} parent={parent} close={close} />;
}

function NameForm({
  task,
  folder,
  parent,
  close,
}: {
  task: Extract<FolderTask, { kind: "create" | "rename" }>;
  folder: Folder | null;
  parent: Folder | null;
  close: () => void;
}) {
  const t = useT();
  const accounts = useAccounts();
  const account = accounts.data?.find((a) => a.id === task.accountId);
  const initial = folder?.name ?? "";
  const [name, setName] = useState(initial);
  const { busy, problem, setProblem, waiting, send, finish } = useFolderRequest(close);
  /** Sends the name: `true` when it is done or waits for the server. */
  const save = async (): Promise<boolean> => {
    if (name.trim() === "") {
      setProblem({ id: "folder-issue-missing-name" });
      return false;
    }
    return send(
      task.kind === "create"
        ? { type: "CreateFolder", account_id: task.accountId, parent_id: task.parentId, name }
        : { type: "RenameFolder", folder_id: task.folderId, name },
    );
  };
  useCloseGuard({
    name: () => name.trim(),
    isDirty: () => !waiting && name.trim() !== initial.trim(),
    canSave: () => name.trim() !== "",
    save,
  });
  const intro =
    task.kind === "rename"
      ? t("folders-rename-intro", { name: folder?.name ?? "" })
      : parent
        ? t("folders-create-intro", { parent: parent.name })
        : t("folders-create-top-intro", { account: account?.name ?? task.accountId });
  return (
    <DialogForm part={FOLDER_DIALOG_PART} dialog="name" busy={busy} onSubmit={() => void save()}>
      <p data-part="folder-dialog.intro">{intro}</p>
      <TextField
        labelId="common-name"
        name="name"
        value={name}
        focusOnMount
        invalid={problem !== null}
        disabled={waiting}
        onChange={setName}
      />
      <Problem message={problem} />
      {waiting ? (
        <Waiting finish={finish} />
      ) : (
        <DialogFooter
          primary={{
            label: task.kind === "create" ? t("common-create") : t("common-save"),
            submit: true,
            action: task.kind,
          }}
          cancel={{ onClick: close }}
          busy={busy}
        />
      )}
    </DialogForm>
  );
}

/** What a window about one existing folder gets. */
interface FolderFormProps {
  folder: Folder;
  folders: Folder[];
  close: () => void;
}

/** The window of `kind` showing `Form` for the task's folder. */
function folderDialog(kind: "move" | "delete", Form: (props: FolderFormProps) => ReactNode) {
  return function FolderDialog({ close }: DialogProps) {
    return (
      <WithFolder
        kind={kind}
        close={close}
        render={(folder, folders) => <Form folder={folder} folders={folders} close={close} />}
      />
    );
  };
}

/** A picker: a pick moves the folder, × cancels (no footer until it waits). */
function MoveForm({ folder, folders, close }: FolderFormProps) {
  const t = useT();
  const { busy, problem, waiting, send, finish } = useFolderRequest(close);
  const targets = new Set(moveTargets(folders, folder).map((f) => f.id));
  const rows = flattenFolders(folders).filter((r) => targets.has(r.folder.id));
  const move = (parent: Folder | null) => {
    void send({ type: "MoveFolder", folder_id: folder.id, parent_id: parent?.id ?? null });
  };
  return (
    <div data-part={FOLDER_DIALOG_PART} data-dialog="move" aria-busy={busy}>
      <p data-part="folder-dialog.intro">{t("folders-move-intro", { name: folder.name })}</p>
      <Problem message={problem} />
      {waiting ? (
        <Waiting finish={finish} />
      ) : (
        <>
          <button
            type="button"
            data-part="form.button"
            data-action="top-level"
            disabled={busy || folder.parent_id === null}
            onClick={() => {
              move(null);
            }}
          >
            {t("folders-move-top")}
          </button>
          <FolderList rows={rows} disabled={busy} onPick={move} />
        </>
      )}
    </div>
  );
}

function DeleteForm({ folder, folders, close }: FolderFormProps) {
  const t = useT();
  const { busy, problem, waiting, send, finish } = useFolderRequest(close);
  const subfolders = subfoldersOf(folders, folder);
  const messages = [folder, ...subfolders].reduce((sum, f) => sum + f.total, 0);
  if (waiting) {
    return (
      <div data-part={FOLDER_DIALOG_PART} data-dialog="delete">
        <Waiting finish={finish} />
      </div>
    );
  }
  return (
    <ConfirmDialog
      kind="folder-delete"
      risk="destructive"
      question={t("folders-delete-question", { name: folder.name })}
      detail={
        <p data-part="dialog.note">
          {messages === 0 && subfolders.length === 0
            ? t("folders-delete-empty-intro")
            : t("folders-delete-intro", { messages, subfolders: subfolders.length })}
        </p>
      }
      action={t("folders-delete-confirm")}
      busy={busy}
      error={problem === null ? null : t(problem.id, problem.args)}
      onConfirm={() => {
        void send({ type: "DeleteFolder", folder_id: folder.id, permanent: true });
      }}
      cancel={{ onClick: close }}
    />
  );
}

/** Window `folder-move`: the new place of a folder. */
export const FolderMoveDialog = folderDialog("move", MoveForm);

/** Confirmation `folder-delete`: delete a folder for good. */
export const FolderDeleteDialog = folderDialog("delete", DeleteForm);
