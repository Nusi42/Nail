import { useEffect, useState, type JSX } from "react";
import { errorMessageId, errorMessageIdOf } from "../api/client";
import { useLocale, useT } from "../i18n/I18nProvider";
import { optionList, type WidgetProps } from "../layout/widgetRegistry";
import { cancelRichSend } from "../features/composer/flows";
import { ShowDetailsButton, type ErrorReport } from "../features/connlog";
import { useComposeContext } from "../state/hooks";
import { useAccounts, useAccountStatuses, useClient, useFolders } from "../state/queries";
import { sendPercent, useUi } from "../state/uiStore";
import { DATE_TIME, formatDateTime } from "../i18n/format";

/** How long "Message sent" stays in the status bar. */
export const SENT_NOTICE_MS = 5000;
/** A send further away than this is shown as scheduled, not as a countdown. */
export const COUNTDOWN_LIMIT_SECS = 120;

function SyncItem() {
  const t = useT();
  const sync = useUi((s) => s.sync);
  const connection = useUi((s) => s.connection);
  const configError = useUi((s) => s.configError);
  const syncError = useUi((s) => s.syncError);
  let text: string;
  let state: string;
  let report: ErrorReport | null = null;
  if (connection === "reconnecting" || connection === "closed") {
    text = t("status-offline");
    state = "offline";
  } else if (configError) {
    text = t("status-config-error", {
      file: configError.file,
      line: configError.line,
      column: configError.column,
    });
    state = "error";
  } else if (syncError) {
    const reason = syncError.localized
      ? t(syncError.localized.id, syncError.localized.args)
      : t(errorMessageId(syncError.code));
    text = t("status-sync-failed", { reason });
    state = "error";
    if (syncError.details) {
      report = {
        accountId: syncError.accountId,
        code: syncError.code,
        localized: syncError.localized ?? null,
        details: syncError.details,
      };
    }
  } else if (sync && sync.done < sync.total) {
    text = t("status-syncing", { done: sync.done, total: sync.total });
    state = "syncing";
  } else {
    text = t("status-ready");
    state = "idle";
  }
  return (
    <span data-part="status-bar.item" data-item="sync-progress" data-state={state}>
      {report ? (
        <>
          <span data-part="status-bar.text">{text}</span>
          <ShowDetailsButton part="status-bar.button" report={report} />
        </>
      ) : (
        text
      )}
    </span>
  );
}

function FolderCountsItem() {
  const t = useT();
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const folders = useFolders(accountId);
  const folder = folders.data?.find((f) => f.id === folderId);
  if (!folder) return null;
  return (
    <span data-part="status-bar.item" data-item="folder-counts">
      {t("status-folder-counts", { total: folder.total, unread: folder.unread })}
    </span>
  );
}

/** Accounts that are not online (offline, sign-in required, connecting). */
function AccountStatusItem() {
  const t = useT();
  const statuses = useAccountStatuses();
  const accounts = useAccounts();
  const problems = (statuses.data ?? []).filter(
    (s) => s.state !== "online" && s.state !== "disabled",
  );
  const first = problems[0];
  if (!first) return null;
  const name = accounts.data?.find((a) => a.id === first.account_id)?.name ?? first.account_id;
  const state = t(`account-state-${first.state}`);
  const text =
    problems.length === 1
      ? t("status-account", { account: name, state })
      : t("status-accounts", { account: name, state, count: problems.length - 1 });
  return (
    <span
      data-part="status-bar.item"
      data-item="account-status"
      data-state={first.state}
      data-account={first.account_id}
    >
      {first.details ? <span data-part="status-bar.text">{text}</span> : text}
      {first.details && (
        <ShowDetailsButton
          part="status-bar.button"
          report={{
            accountId: first.account_id,
            code: first.error ?? first.details.code,
            localized: first.localized ?? null,
            details: first.details,
          }}
        />
      )}
    </span>
  );
}

/** Queued writes still being replayed, or the last one that failed. */
function OpsItem() {
  const t = useT();
  const pending = useUi((s) => s.pendingOps.length);
  const error = useUi((s) => s.opError);
  const failedOp = useUi((s) => s.failedOp);
  const dismiss = useUi((s) => s.dismissOpError);
  const queued = useUi((s) => s.opQueued);
  const client = useClient();
  const [retryError, setRetryError] = useState<string | null>(null);
  const retry = (op: string) => {
    setRetryError(null);
    client
      .request({ type: "RetryOp", op_id: op }, "Queued")
      .then(({ op_ids }) => {
        dismiss();
        queued(op_ids);
      })
      .catch((e: unknown) => {
        setRetryError(errorMessageIdOf(e));
      });
  };
  if (error) {
    return (
      <span data-part="status-bar.item" data-item="ops" data-state="error">
        <span data-part="status-bar.text">
          {t("status-op-failed", { reason: t(retryError ?? errorMessageId(error)) })}
        </span>
        {failedOp !== null && (
          <button
            type="button"
            data-part="status-bar.button"
            onClick={() => {
              retry(failedOp);
            }}
          >
            {t("status-retry")}
          </button>
        )}
        <button type="button" data-part="status-bar.button" onClick={dismiss}>
          {t("common-dismiss")}
        </button>
      </span>
    );
  }
  if (pending === 0) return null;
  return (
    <span data-part="status-bar.item" data-item="ops" data-state="pending">
      {t("status-ops-pending", { count: pending })}
    </span>
  );
}

function useSecondsUntil(iso: string | null): number {
  const target = iso === null ? Number.NaN : Date.parse(iso);
  const compute = () => Math.max(0, Math.ceil((target - Date.now()) / 1000));
  const [seconds, setSeconds] = useState(compute);
  useEffect(() => {
    if (Number.isNaN(target)) return;
    const tick = () => {
      setSeconds(Math.max(0, Math.ceil((target - Date.now()) / 1000)));
    };
    tick();
    const timer = setInterval(tick, 1000);
    return () => {
      clearInterval(timer);
    };
  }, [target]);
  return seconds;
}

/** The last sent message: undo-send countdown, sending, sent, failed. */
function SendStatusItem() {
  const t = useT();
  const locale = useLocale();
  const ctx = useComposeContext();
  const entry = useUi((s) => s.lastSend);
  const details = useUi((s) => s.lastSendDetails);
  const setLastSend = useUi((s) => s.setLastSend);
  const progress = useUi((s) => (s.lastSend ? s.sendProgress[s.lastSend.id] : undefined));
  const seconds = useSecondsUntil(entry?.state === "scheduled" ? entry.send_at : null);
  const [failure, setFailure] = useState<string | null>(null);

  const sentId = entry?.state === "sent" ? entry.id : null;
  useEffect(() => {
    if (sentId === null) return;
    const timer = setTimeout(() => {
      const current = ctx.store.getState().lastSend;
      if (current?.id === sentId) setLastSend(null);
    }, SENT_NOTICE_MS);
    return () => {
      clearTimeout(timer);
    };
  }, [sentId, setLastSend, ctx.store]);

  if (!entry || entry.state === "cancelled") return null;
  let text: string;
  let undo = false;
  switch (entry.state) {
    case "scheduled":
      undo = true;
      if (seconds > COUNTDOWN_LIMIT_SECS) {
        text = t("status-send-scheduled", {
          time: formatDateTime(entry.send_at, locale, DATE_TIME),
        });
      } else {
        text = t("status-send-countdown", { seconds });
      }
      break;
    case "sending":
      text = progress
        ? t("status-sending-progress", { percent: sendPercent(progress) })
        : t("status-sending");
      break;
    case "sent":
      text = t("status-sent");
      break;
    case "failed":
      text = t("status-send-failed", {
        reason: t(errorMessageId(entry.error ?? "internal")),
      });
      break;
  }
  return (
    <span data-part="status-bar.item" data-item="send-status" data-state={entry.state}>
      <span data-part="status-bar.text">{failure ? t(failure) : text}</span>
      {undo && (
        <button
          type="button"
          data-part="status-bar.button"
          data-action="outbox.undo"
          onClick={() => {
            setFailure(null);
            cancelRichSend(ctx, entry.id).catch((error: unknown) => {
              setFailure(errorMessageIdOf(error));
            });
          }}
        >
          {t("common-undo")}
        </button>
      )}
      {entry.state === "failed" && details && (
        <ShowDetailsButton
          part="status-bar.button"
          report={{
            accountId: entry.account_id,
            code: entry.error ?? details.code,
            localized: null,
            details,
          }}
        />
      )}
    </span>
  );
}

const ITEMS: Record<string, () => JSX.Element | null> = {
  "sync-progress": SyncItem,
  "folder-counts": FolderCountsItem,
  "account-status": AccountStatusItem,
  ops: OpsItem,
  "send-status": SendStatusItem,
};

/** Items the status bar knows (`items` option). */
export const STATUS_ITEMS = Object.keys(ITEMS);

/** Status line; items come from the `items` option. */
export function StatusBar({ node }: WidgetProps) {
  const t = useT();
  const items = optionList(node, "items", ["sync-progress"]).filter((item) =>
    Object.hasOwn(ITEMS, item),
  );
  return (
    <div
      data-part="status-bar"
      data-node-id={node.id ?? undefined}
      role="status"
      aria-live="polite"
      aria-label={t("common-status")}
    >
      {items.map((item) => {
        const Item = ITEMS[item];
        return Item ? <Item key={item} /> : null;
      })}
    </div>
  );
}
