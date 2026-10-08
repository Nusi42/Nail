/**
 * Pure helpers of the sync feature, ports of `nail_api::accounts::SyncPolicy`
 * and the core's `sync_policy` module (ADR 0043): the effective policy of a
 * folder and editing the rule written for exactly one folder path; pinned by
 * `crates/nail-core/src/sync_policy.rs` (its vectors run in `model.test.ts`).
 */
import type {
  AccountConfig,
  BodyPolicy,
  Folder,
  FolderMatch,
  FolderPolicy,
  FolderSyncPolicy,
} from "../../api/generated";

/** Body policy modes (for labels `sync-bodies-<mode>`). */
export const BODY_MODES: readonly BodyPolicy["mode"][] = ["full", "headers-only", "recent"];

function matches(rule: FolderMatch, folder: Folder): boolean {
  if ("path" in rule) {
    return (
      rule.path === folder.path ||
      (rule.path.toUpperCase() === "INBOX" && folder.path.toUpperCase() === "INBOX")
    );
  }
  return rule.role === folder.role && folder.role !== "other";
}

function isPathRule(rule: FolderPolicy, path: string): boolean {
  return "path" in rule.folder && rule.folder.path === path;
}

/** `SyncPolicy::effective` plus the folder's own rule (the core's `resolve`). */
export function resolvePolicy(account: AccountConfig, folder: Folder): FolderSyncPolicy {
  const imap = account.incoming.protocol === "imap";
  let bodies: BodyPolicy = account.sync.bodies ?? { mode: "full" };
  let enabled = true;
  let idle = folder.role === "inbox";
  for (const rule of account.sync.folders) {
    if (!matches(rule.folder, folder)) continue;
    if (rule.bodies !== null) bodies = rule.bodies;
    if (rule.enabled !== null) enabled = rule.enabled;
    if (rule.idle !== null) idle = rule.idle;
  }
  const own = [...account.sync.folders].reverse().find((r) => isPathRule(r, folder.path));
  return {
    folder_id: folder.id,
    account_id: folder.account_id,
    path: folder.path,
    bodies: imap ? bodies : { mode: "full" },
    enabled,
    idle,
    rule: own ?? null,
    on_demand: imap,
  };
}

/** Replaces the rules for exactly `path` with one (none when all are `null`). */
export function withFolderRule(
  account: AccountConfig,
  path: string,
  bodies: BodyPolicy | null,
  enabled: boolean | null,
  idle: boolean | null,
): AccountConfig {
  const folders = account.sync.folders;
  const position = folders.findIndex((r) => isPathRule(r, path));
  const kept = folders.filter((r) => !isPathRule(r, path));
  if (bodies !== null || enabled !== null || idle !== null) {
    const rule: FolderPolicy = { folder: { path }, bodies, enabled, idle };
    kept.splice(position < 0 ? kept.length : Math.min(position, kept.length), 0, rule);
  }
  return { ...account, sync: { ...account.sync, folders: kept } };
}

/** A folder setting that may follow the account's default (`sync-<setting>-<choice>`). */
export const TRI_CHOICES = ["default", "yes", "no"] as const;
export type Tri = (typeof TRI_CHOICES)[number];
