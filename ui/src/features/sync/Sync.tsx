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
import { useUi } from "../../state/uiStore";
import { flattenFolders } from "../../widgets/FolderTree";
import { NumberField, SelectField } from "../../widgets/forms";
import { BODY_MODES } from "./model";
import { useSync } from "./store";

export const syncKeys = {
  policy: (folderId: FolderId | null) => ["sync", "policy", folderId] as const,
  all: ["sync"] as const,
};

/** Name of the folder sync dialog. */
export const SYNC_FOLDER_DIALOG = "sync-folder";

type Tri = "default" | "yes" | "no";

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

function PolicyForm({
  policy,
  saved,
  setSaved,
}: {
  policy: FolderSyncPolicy;
  /** Kept by the editor: a save re-creates the form. */
  saved: boolean;
  setSaved(saved: boolean): void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const rule = policy.rule;
  const [mode, setMode] = useState<"default" | BodyPolicy["mode"]>(rule?.bodies?.mode ?? "default");
  const [days, setDays] = useState<number | null>(
    rule?.bodies?.mode === "recent" ? rule.bodies.days : 30,
  );
  const [enabled, setEnabled] = useState<Tri>(tri(rule?.enabled ?? null));
  const [idle, setIdle] = useState<Tri>(tri(rule?.idle ?? null));
  const [problems, setProblems] = useState<LocalizedMessage[]>([]);
  const triChoices = (id: string) =>
    (["default", "yes", "no"] as const).map((v) => ({ value: v, label: `sync-${id}-${v}` }));

  const save = async () => {
    let bodies: BodyPolicy | null = null;
    if (mode === "recent") {
      if (days === null || days < 1) {
        setProblems([{ id: "sync-error-days" }]);
        return;
      }
      bodies = { mode: "recent", days };
    } else if (mode !== "default") {
      bodies = { mode };
    }
    setProblems([]);
    try {
      const answer = await client.request(
        {
          type: "SetFolderSyncPolicy",
          folder_id: policy.folder_id,
          bodies,
          enabled: fromTri(enabled),
          idle: fromTri(idle),
        },
        "FolderSyncPolicy",
      );
      queryClient.setQueryData(syncKeys.policy(policy.folder_id), answer.policy);
      setSaved(true);
    } catch (error) {
      setProblems([errorMessageOf(error)]);
    }
  };

  const changed = () => setSaved(false);
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
            value={mode}
            choices={[
              { value: "default", label: "sync-bodies-default" },
              ...BODY_MODES.map((m) => ({ value: m, label: `sync-choice-${m}` })),
            ]}
            onChange={(value) => {
              setMode(value as typeof mode);
              changed();
            }}
          />
          {mode === "recent" && (
            <NumberField
              labelId="sync-days"
              name="days"
              min={1}
              value={days}
              onChange={(value) => {
                setDays(value);
                changed();
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
        value={enabled}
        choices={triChoices("enabled")}
        onChange={(value) => {
          setEnabled(value as Tri);
          changed();
        }}
      />
      {policy.on_demand && (
        <SelectField
          labelId="sync-idle-label"
          name="idle"
          value={idle}
          choices={triChoices("idle")}
          onChange={(value) => {
            setIdle(value as Tri);
            changed();
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
          {t("sync-saved")}
        </p>
      )}
      <div data-part="sync-policy.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          onClick={() => void save()}
        >
          {t("sync-save")}
        </button>
      </div>
    </div>
  );
}

/** The sync settings of one folder. */
export function FolderPolicyEditor({ folderId }: { folderId: FolderId }) {
  const t = useT();
  const policy = usePolicy(folderId);
  const [saved, setSaved] = useState(false);
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
      <PolicyForm
        key={JSON.stringify(policy.data.rule)}
        policy={policy.data}
        saved={saved}
        setSaved={setSaved}
      />
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
        <p data-part="sync-folder-policies.empty">{t("sync-no-accounts")}</p>
      ) : (
        <>
          <SelectField
            labelId="sync-account"
            name="account"
            value={accountId ?? ""}
            choices={accounts.map((a) => ({ value: a.id, label: a.name, literal: true }))}
            onChange={setAccount}
          />
          <SelectField
            labelId="sync-folder"
            name="folder"
            value={folderId ?? ""}
            choices={rows.map(({ folder, depth }) => ({
              value: folder.id,
              label: `${" ".repeat(depth)}${folder.name}`,
              literal: true,
            }))}
            onChange={setFolder}
          />
          {folderId !== null && <FolderPolicyEditor key={folderId} folderId={folderId} />}
        </>
      )}
    </section>
  );
}

/** The dialog `sync-folder`: the folder the menu was opened on. */
export function SyncFolderDialog() {
  const t = useT();
  const folderId = useSync((s) => s.folderId);
  if (folderId === null) return <p data-part="sync-policy.empty">{t("sync-no-folder")}</p>;
  return <FolderPolicyEditor key={folderId} folderId={folderId} />;
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
  const empty =
    message.data !== undefined && message.data.text === "" && !message.data.sanitized_html;
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
