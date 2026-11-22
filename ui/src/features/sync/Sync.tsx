/**
 * Per-folder sync policies and bodies on demand:
 *
 * - `FolderPolicyEditor`: what one folder downloads (full / headers only /
 *   the last N days), whether it is synchronized and watched (IDLE); the
 *   choice is written to `accounts.kdl` as a `sync` rule for exactly that
 *   folder ("as the account" removes it). Shown by the settings widget
 *   `sync-folder-policies` and the dialog `sync-folder` (folder menu).
 * - `sync-message-body`: in the reader, a bar for messages whose body is
 *   not stored, with "Download full message".
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { BodyPolicy, FolderId, FolderSyncPolicy, LocalizedMessage } from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { findSummary } from "../../state/mailOps";
import { useAccountsConfig, useClient, useFolders, useMessage } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { flattenFolders } from "../../widgets/FolderTree";
import { NumberField, SelectField } from "../../widgets/forms";
import type { DialogProps } from "../../layout/dialogRegistry";
import { closeWindowNow, useCloseGuard, useGuardKey } from "../../layout/closeGuard";
import { DialogFooter, DialogForm } from "../../widgets/dialog";
import { useLeaveDrafts, useSettingsForm } from "../settings/draft";
import { PickerField } from "../../widgets/pickers";
import { BODY_MODES, TRI_CHOICES, type Tri } from "./model";
import { useSync } from "./store";

export const syncKeys = {
  policy: (folderId: FolderId | null) => ["sync", "policy", folderId] as const,
  all: ["sync"] as const,
};

/** Name of the folder sync dialog. */
export const SYNC_FOLDER_DIALOG = "sync-folder";

function tri(value: boolean | null): Tri {
  return value === null ? "default" : value ? "yes" : "no";
}

function fromTri(value: Tri): boolean | null {
  return value === "default" ? null : value === "yes";
}

function bodiesText(bodies: BodyPolicy, t: (id: string, a?: Record<string, number>) => string) {
  return bodies.mode === "recent"
    ? t("sync-bodies-recent", { days: bodies.days })
    : t(`sync-bodies-${bodies.mode}`);
}

function usePolicy(folderId: FolderId | null) {
  const client = useClient();
  return useQuery({
    queryKey: syncKeys.policy(folderId),
    queryFn: async () =>
      (
        await client.request(
          { type: "GetFolderSyncPolicy", folder_id: folderId ?? "" },
          "FolderSyncPolicy",
        )
      ).policy,
    enabled: folderId !== null,
  });
}

/** What the policy form edits ("default": as the account). */
export interface PolicyDraft {
  mode: "default" | BodyPolicy["mode"];
  /** The days of "recent" (kept while another mode is chosen). */
  days: number | null;
  enabled: Tri;
  idle: Tri;
}

/** The form's values for the folder's own rule. */
export function policyDraftOf(policy: FolderSyncPolicy): PolicyDraft {
  const rule = policy.rule;
  return {
    mode: rule?.bodies?.mode ?? "default",
    days: rule?.bodies?.mode === "recent" ? rule.bodies.days : 30,
    enabled: tri(rule?.enabled ?? null),
    idle: tri(rule?.idle ?? null),
  };
}

/** Problems that keep the draft from being saved. */
export function policyDraftIssues(draft: PolicyDraft): LocalizedMessage[] {
  return draft.mode === "recent" && (draft.days === null || draft.days < 1)
    ? [{ id: "sync-error-days" }]
    : [];
}

/** Writes the folder's rule; the policy query shows it when this resolves. */
function useWritePolicy(policy: FolderSyncPolicy) {
  const client = useClient();
  const queryClient = useQueryClient();
  return async (draft: PolicyDraft) => {
    let bodies: BodyPolicy | null = null;
    if (draft.mode === "recent") bodies = { mode: "recent", days: draft.days ?? 1 };
    else if (draft.mode !== "default") bodies = { mode: draft.mode };
    const answer = await client.request(
      {
        type: "SetFolderSyncPolicy",
        folder_id: policy.folder_id,
        bodies,
        enabled: fromTri(draft.enabled),
        idle: fromTri(draft.idle),
      },
      "FolderSyncPolicy",
    );
    queryClient.setQueryData(syncKeys.policy(policy.folder_id), answer.policy);
  };
}

function PolicyFields({
  policy,
  draft,
  onChange,
  problems,
  saved,
}: {
  policy: FolderSyncPolicy;
  draft: PolicyDraft;
  onChange: (draft: PolicyDraft) => void;
  problems: readonly LocalizedMessage[];
  /** Show that the rule was written. */
  saved: boolean;
}) {
  const t = useT();
  const triChoices = (id: string) =>
    TRI_CHOICES.map((v) => ({ value: v, label: `sync-${id}-${v}` }));
  return (
    <div data-part="sync-policy.form">
      <p data-part="sync-policy.current" role="status">
        {t("sync-current", {
          bodies: bodiesText(policy.bodies, t),
          synced: policy.enabled ? "yes" : "no",
          idle: policy.idle ? "yes" : "no",
        })}
      </p>
      {policy.on_demand ? (
        <>
          <SelectField
            labelId="sync-bodies-label"
            name="bodies"
            value={draft.mode}
            choices={[
              { value: "default", label: "sync-bodies-default" },
              ...BODY_MODES.map((m) => ({ value: m, label: `sync-choice-${m}` })),
            ]}
            onChange={(value) => {
              onChange({ ...draft, mode: value as PolicyDraft["mode"] });
            }}
          />
          {draft.mode === "recent" && (
            <NumberField
              labelId="sync-days"
              name="days"
              min={1}
              value={draft.days}
              onChange={(days) => {
                onChange({ ...draft, days });
              }}
            />
          )}
          <p data-part="form.hint">{t("sync-bodies-hint")}</p>
        </>
      ) : (
        <p data-part="form.hint">{t("sync-pop3-full")}</p>
      )}
      <SelectField
        labelId="sync-enabled-label"
        name="enabled"
        value={draft.enabled}
        choices={triChoices("enabled")}
        onChange={(value) => {
          onChange({ ...draft, enabled: value as Tri });
        }}
      />
      {policy.on_demand && (
        <SelectField
          labelId="sync-idle-label"
          name="idle"
          value={draft.idle}
          choices={triChoices("idle")}
          onChange={(value) => {
            onChange({ ...draft, idle: value as Tri });
          }}
        />
      )}
      {problems.length > 0 && (
        <ul data-part="sync-policy.issues" role="alert">
          {problems.map((p) => (
            <li key={p.id} data-part="sync-policy.issue">
              {t(p.id, p.args)}
            </li>
          ))}
        </ul>
      )}
      {saved && (
        <p data-part="sync-policy.notice" role="status">
          {t("common-saved-to-accounts")}
        </p>
      )}
    </div>
  );
}

/** In Settings (R6, ADR 0239): the edits wait for the page's Save. */
function SettingsPolicyForm({ policy }: { policy: FolderSyncPolicy }) {
  const write = useWritePolicy(policy);
  const [failure, setFailure] = useState<LocalizedMessage | null>(null);
  const [saved, setSaved] = useState(false);
  const form = useSettingsForm(policyDraftOf(policy), {
    canSave: (draft) => policyDraftIssues(draft).length === 0,
    save: write,
    onSaved: () => {
      setSaved(true);
    },
    onError: (error) => {
      setFailure(errorMessageOf(error));
    },
  });
  const issues = policyDraftIssues(form.value);
  return (
    <PolicyFields
      policy={policy}
      draft={form.value}
      onChange={(draft) => {
        form.set(draft);
        setSaved(false);
        setFailure(null);
      }}
      problems={failure === null ? issues : [...issues, failure]}
      saved={saved && !form.dirty}
    />
  );
}

/** The `sync-folder` dialog's form: Save writes and closes, Cancel asks when changed. */
function DialogPolicyForm({ policy, close }: { policy: FolderSyncPolicy; close: () => void }) {
  const t = useT();
  const ui = useUiStoreApi();
  const guard = useGuardKey();
  const write = useWritePolicy(policy);
  const [initial] = useState(() => policyDraftOf(policy));
  const [draft, setDraft] = useState(initial);
  const [failure, setFailure] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const issues = policyDraftIssues(draft);
  const dirty = JSON.stringify(draft) !== JSON.stringify(initial);
  const save = async (): Promise<boolean> => {
    if (issues.length > 0) return false;
    setBusy(true);
    setFailure(null);
    try {
      await write(draft);
      return true;
    } catch (error) {
      setFailure(errorMessageOf(error));
      return false;
    } finally {
      setBusy(false);
    }
  };
  useCloseGuard({
    name: () => policy.path,
    isDirty: () => dirty,
    save,
    canSave: () => issues.length === 0,
  });
  return (
    <DialogForm
      part="sync-policy.dialog"
      busy={busy}
      onSubmit={() => {
        void save().then((saved) => {
          if (saved) closeWindowNow(ui, guard);
        });
      }}
    >
      <PolicyFields
        policy={policy}
        draft={draft}
        onChange={(next) => {
          setDraft(next);
          setFailure(null);
        }}
        problems={failure === null ? issues : [...issues, failure]}
        saved={false}
      />
      <DialogFooter
        primary={{
          label: t("common-save"),
          submit: true,
          action: "save-sync-policy",
          disabled: issues.length > 0,
        }}
        cancel={{ onClick: close }}
        busy={busy}
      />
    </DialogForm>
  );
}

/** The sync settings of one folder: in Settings, or in the `sync-folder` dialog (`close`). */
export function FolderPolicyEditor({
  folderId,
  close,
}: {
  folderId: FolderId;
  close?: () => void;
}) {
  const t = useT();
  const policy = usePolicy(folderId);
  if (policy.isError) {
    const message = errorMessageOf(policy.error);
    return (
      <p data-part="sync-policy.error" role="alert">
        {t(message.id, message.args)}
      </p>
    );
  }
  if (!policy.data) return <p data-part="sync-policy.loading">{t("sync-loading")}</p>;
  return (
    <section
      data-part="sync-policy"
      aria-label={t("sync-policy-label", { path: policy.data.path })}
    >
      {close === undefined ? (
        <SettingsPolicyForm policy={policy.data} />
      ) : (
        <DialogPolicyForm policy={policy.data} close={close} />
      )}
    </section>
  );
}

/** Settings → Synchronization (widget `sync-folder-policies`). */
export function SyncPoliciesWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const config = useAccountsConfig();
  const accounts = config.data?.accounts ?? [];
  const [chosenAccount, setAccount] = useState<string | null>(null);
  const leave = useLeaveDrafts();
  const accountId = chosenAccount ?? accounts[0]?.id ?? null;
  const folders = useFolders(accountId);
  const rows = flattenFolders(folders.data ?? []);
  const [chosenFolder, setFolder] = useState<string | null>(null);
  const folderId = rows.some((r) => r.folder.id === chosenFolder)
    ? chosenFolder
    : (rows[0]?.folder.id ?? null);
  return (
    <section
      data-part="sync-folder-policies"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("sync-policies-label")}
    >
      <p data-part="sync-folder-policies.intro">{t("sync-intro")}</p>
      {accounts.length === 0 ? (
        <p data-part="sync-folder-policies.empty">{t("common-no-accounts-yet")}</p>
      ) : (
        <>
          {/* Pickers: only a picked account or folder changes what the editor shows. */}
          <PickerField
            kind="account"
            labelId="sync-account"
            name="account"
            value={accountId ?? ""}
            onChange={(id, option) => {
              // R6: another account asks first when this folder has unsaved changes.
              if (option) {
                leave(() => {
                  setAccount(id);
                });
              }
            }}
          />
          <PickerField
            kind="folder"
            labelId="sync-folder"
            name="folder"
            accountId={accountId}
            folderValue="id"
            value={folderId ?? ""}
            onChange={(id, option) => {
              if (option) {
                leave(() => {
                  setFolder(id);
                });
              }
            }}
          />
          {folderId !== null && <FolderPolicyEditor key={folderId} folderId={folderId} />}
        </>
      )}
    </section>
  );
}

/** The dialog `sync-folder`: the folder the menu was opened on. */
export function SyncFolderDialog({ close }: DialogProps) {
  const t = useT();
  const folderId = useSync((s) => s.folderId);
  if (folderId === null) return <p data-part="sync-policy.empty">{t("sync-no-folder")}</p>;
  return <FolderPolicyEditor key={folderId} folderId={folderId} close={close} />;
}

/**
 * Reader bar for a message whose body is not stored (headers-only or
 * older than the folder's "recent" window): "Download full message".
 */
export function MessageBodyBar({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const queryClient = useQueryClient();
  const { dispatch } = useActions();
  const selectedId = useUi((s) => s.selectedMessageId);
  const message = useMessage(selectedId);
  // Re-render when list data (and with it `headers_only`) changes.
  useUi((s) => s.messageOrder);
  const busy = useSync((s) => s.fetching.includes(selectedId ?? ""));
  const failed = useSync((s) => s.failed.includes(selectedId ?? ""));
  const summary = selectedId === null ? undefined : findSummary(queryClient, selectedId);
  if (selectedId === null || !summary?.headers_only) return null;
  const empty = message.data?.text === "" && !message.data.sanitized_html;
  return (
    <div
      data-part="sync-message-body"
      data-state={failed ? "failed" : busy ? "fetching" : undefined}
      data-node-id={node.id ?? undefined}
      role="status"
      aria-label={node.label ? label(node.label) : t("sync-message-body-label")}
    >
      <span data-part="sync-message-body.text">
        {t(
          failed
            ? "sync-message-body-failed"
            : empty
              ? "sync-message-body-missing"
              : "sync-message-body-partial",
        )}
      </span>
      <button
        type="button"
        data-part="form.button"
        data-action="sync.download-message"
        disabled={busy}
        onClick={() => void dispatch("sync.download-message")}
      >
        {t(busy ? "sync-message-body-fetching" : "sync-action-download-message")}
      </button>
    </div>
  );
}
