/**
 * The mock core's folder management (ADR 0196): `CreateFolder`,
 * `RenameFolder`, `MoveFolder`, `DeleteFolder` change the mock folders at
 * once (ids stay on rename and move, as in the core) and "replay" with
 * `OpStatus` like other writes. The core's checks that matter for the UI
 * are kept: names, system folders, taken names, Gmail accounts, Trash.
 * `../../api/mock.ts` routes exactly these requests here.
 */
import type { Folder, Request, Response } from "../../api/generated";
import { mockFolderId } from "../../api/mockData";
import { replayOp, type MockCore } from "../../api/mockMail";
import { canManage, deleteMode, isInside, isSystemFolder, subfoldersOf } from "./model";

/** The requests this feature answers. */
export const FOLDER_REQUESTS = [
  "CreateFolder",
  "RenameFolder",
  "MoveFolder",
  "DeleteFolder",
] as const satisfies readonly Request["type"][];

export type FolderRequest = Extract<Request, { type: (typeof FOLDER_REQUESTS)[number] }>;

export function isFolderRequest(request: Request): request is FolderRequest {
  return (FOLDER_REQUESTS as readonly string[]).includes(request.type);
}

/** A refusal with the core's reason (`folder-issue-*`). */
function refused(id: string): Response {
  return { type: "Error", code: "invalid_request", message: id, localized: { id } };
}

/** Puts `folder` (and its subfolders) at `parent` / `name`, keeping ids. */
function place(folders: Folder[], folder: Folder, parent: Folder | null, name: string): void {
  const oldPath = folder.path;
  const newPath = parent ? `${parent.path}/${name}` : name;
  for (const sub of subfoldersOf(folders, folder)) {
    sub.path = newPath + sub.path.slice(oldPath.length);
  }
  folder.name = name;
  folder.path = newPath;
  folder.parent_id = parent?.id ?? null;
}

/** Queues the change: one op, `MailChanged` and its replay. */
function queued(core: MockCore, accountId: string): Response {
  const opId = core.nextId("folder-");
  core.emit({ type: "MailChanged", folder_ids: [] });
  replayOp(core, opId, accountId);
  return { type: "Queued", op_ids: [opId] };
}

export function answerFolder(core: MockCore, request: FolderRequest): Response {
  const d = core.data;
  const accountId =
    request.type === "CreateFolder"
      ? request.account_id
      : d.folders.find((f) => f.id === request.folder_id)?.account_id;
  const account = d.accounts.find((a) => a.id === accountId);
  if (!account || accountId === undefined) {
    return { type: "Error", code: "not_found", message: "no such account or folder" };
  }
  if (account.kind === "gmail") return refused("folder-issue-gmail-labels");
  if (!canManage(account.kind)) return refused("folder-issue-unsupported");
  const folders = d.folders.filter((f) => f.account_id === accountId);
  const taken = (parentId: string | null, name: string, except: string | null) =>
    folders.some(
      (f) =>
        f.parent_id === parentId && f.name.toLowerCase() === name.toLowerCase() && f.id !== except,
    );
  const byId = (id: string | null) =>
    id === null ? null : (folders.find((f) => f.id === id) ?? null);
  if (request.type === "CreateFolder") {
    const name = request.name.trim();
    if (name === "") return refused("folder-issue-missing-name");
    if (name.includes("/")) return refused("folder-issue-invalid-name");
    if (taken(request.parent_id, name, null)) return refused("folder-issue-name-taken");
    const parent = byId(request.parent_id);
    const path = parent ? `${parent.path}/${name}` : name;
    d.folders.push({
      id: mockFolderId(accountId, path),
      account_id: accountId,
      parent_id: parent?.id ?? null,
      name,
      path,
      role: "other",
      unread: 0,
      total: 0,
    });
    return queued(core, accountId);
  }
  const folder = byId(request.folder_id);
  if (!folder) return { type: "Error", code: "not_found", message: request.folder_id };
  if (isSystemFolder(folder)) return refused("folder-issue-system-folder");
  switch (request.type) {
    case "RenameFolder": {
      const name = request.name.trim();
      if (name === "") return refused("folder-issue-missing-name");
      if (taken(folder.parent_id, name, folder.id)) return refused("folder-issue-name-taken");
      place(folders, folder, byId(folder.parent_id), name);
      return queued(core, accountId);
    }
    case "MoveFolder": {
      const parent = byId(request.parent_id);
      if (parent && isInside(folders, parent, folder.id))
        return refused("folder-issue-into-itself");
      if (taken(request.parent_id, folder.name, folder.id))
        return refused("folder-issue-name-taken");
      place(folders, folder, parent, folder.name);
      return queued(core, accountId);
    }
    case "DeleteFolder": {
      if (!request.permanent) {
        if (deleteMode(folders, folder) === "permanent") return refused("folder-issue-no-trash");
        const trash = folders.find((f) => f.role === "trash") ?? null;
        place(folders, folder, trash, folder.name);
        return queued(core, accountId);
      }
      const gone = new Set([folder.id, ...subfoldersOf(folders, folder).map((f) => f.id)]);
      d.folders = d.folders.filter((f) => !gone.has(f.id));
      d.messages = d.messages.filter((m) => !gone.has(m.folder_id));
      return queued(core, accountId);
    }
  }
}
