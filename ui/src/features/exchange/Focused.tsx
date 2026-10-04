/**
 * Focused Inbox (ADR 0072: `MessageSummary.exchange.inference`, queries
 * `focus:focused` / `focus:other`):
 *
 * - widget `focused-tabs` over the message list: All / Focused / Other for
 *   the inbox of the selected Exchange account (a tab = the inbox filtered
 *   by its `focus:` query), and the "Focused Inbox" switch (a view choice
 *   per account). Nothing for other accounts; a note when the server does
 *   not classify (`focused-inbox-unavailable`).
 * - dialog `exchange-classify`: move the chosen messages to Focused /
 *   Other, optionally always for their senders (`ClassifyFocused`).
 * - mount point `FocusMoveButton`: "Move to Other" / "Move to Focused" for
 *   one classified message (reader toolbar).
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  InferenceClassification,
  LocalizedMessage,
  MessageSummary,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { findSummary } from "../../state/mailOps";
import { useFolders } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { Problem, useWriteContext } from "./common";
import { focusOfQuery } from "./model";
import { CLASSIFY_DIALOG } from "./names";
import { classifyFocused, showFocus, useExchangeAccount, useExchangeStatus } from "./queries";
import { exchangeStoreFor, useExchangeUi } from "./store";

const TABS: readonly InferenceClassification[] = ["focused", "other"];

/** The other tab. */
export function otherFocus(focus: InferenceClassification): InferenceClassification {
  return focus === "focused" ? "other" : "focused";
}

/** Widget `focused-tabs`. */
export function FocusedTabs({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const store = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const account = useExchangeAccount(accountId);
  const folders = useFolders(account?.id ?? null);
  const status = useExchangeStatus(account?.id ?? null);
  const off = useExchangeUi((s) => (account ? s.focusedOff.includes(account.id) : false));
  const showAll = optionBool(node, "show-all", true);
  const showSwitch = optionBool(node, "show-switch", true);
  const inbox = folders.data?.find((f) => f.role === "inbox") ?? null;
  if (!account || !inbox) return null;
  const unavailable = status.data?.warnings.includes("focused-inbox-unavailable") ?? false;
  const active = search?.accountId === account.id ? focusOfQuery(search.text) : null;
  const allActive = active === null && search === null && activeFolderId === inbox.id;

  const toggle = () => {
    const turnOn = off;
    exchangeStoreFor(store).getState().setFocused(account.id, turnOn);
    // Switching the tabs off leaves a Focused / Other view.
    if (!turnOn && active !== null) showFocus(store, account.id, inbox.id, null);
  };

  return (
    <nav
      data-part="focused-tabs"
      data-node-id={node.id ?? undefined}
      data-state={off || unavailable ? "off" : "on"}
      aria-label={node.label ? label(node.label) : t("exchange-focused-label")}
    >
      {unavailable ? (
        <p data-part="focused-tabs.note">{t("exchange-warning-focused-inbox-unavailable")}</p>
      ) : (
        !off && (
          <ul data-part="focused-tabs.list">
            {showAll && (
              <li data-part="focused-tabs.item">
                <button
                  type="button"
                  data-part="focused-tabs.tab"
                  data-focus="all"
                  data-state={allActive ? "selected" : undefined}
                  aria-pressed={allActive}
                  onClick={() => {
                    showFocus(store, account.id, inbox.id, null);
                  }}
                >
                  {t("exchange-focus-all")}
                </button>
              </li>
            )}
            {TABS.map((focus) => (
              <li key={focus} data-part="focused-tabs.item">
                <button
                  type="button"
                  data-part="focused-tabs.tab"
                  data-focus={focus}
                  data-state={active === focus ? "selected" : undefined}
                  aria-pressed={active === focus}
                  onClick={() => {
                    showFocus(store, account.id, inbox.id, focus);
                  }}
                >
                  {t(`exchange-focus-${focus}`)}
                </button>
              </li>
            ))}
          </ul>
        )
      )}
      {showSwitch && !unavailable && (
        <button
          type="button"
          role="switch"
          data-part="focused-tabs.switch"
          aria-checked={!off}
          onClick={toggle}
        >
          {t("exchange-focused-switch")}
        </button>
      )}
    </nav>
  );
}

/** Opens the classify dialog for messages. */
export function openClassify(
  store: ReturnType<typeof useUiStoreApi>,
  messageIds: string[],
  to: InferenceClassification,
): void {
  exchangeStoreFor(store).getState().classifyMessages({ messageIds, to });
  store.getState().openWindow(CLASSIFY_DIALOG);
}

/**
 * Mount point `FocusMoveButton`: moves one classified Exchange message to
 * the other tab (through the dialog, which offers "always"); nothing for
 * other mail.
 */
export function FocusMoveButton({ message }: { message: MessageSummary }) {
  const t = useT();
  const store = useUiStoreApi();
  const inference = message.exchange?.inference ?? null;
  if (inference === null) return null;
  const to = otherFocus(inference);
  return (
    <button
      type="button"
      data-part="exchange.focus-move"
      data-focus={to}
      onClick={() => {
        openClassify(store, [message.id], to);
      }}
    >
      {t(`exchange-move-to-${to}`)}
    </button>
  );
}

/** Dialog `exchange-classify`. */
export function ClassifyDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useWriteContext();
  const target = useExchangeUi((s) => s.classify);
  const [always, setAlways] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const finish = () => {
    exchangeStoreFor(ctx.store).getState().classifyMessages(null);
    close();
  };
  if (target === null || target.messageIds.length === 0) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-nothing-chosen")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  const senders = [
    ...new Set(
      target.messageIds
        .map((id) => findSummary(ctx.queryClient, id)?.from?.email)
        .filter((e): e is string => typeof e === "string"),
    ),
  ];
  const move = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await classifyFocused(ctx, target.messageIds, target.to, always);
      finish();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-part="exchange.dialog" data-dialog="classify" aria-busy={busy}>
      <p data-part="exchange.question">
        {t(`exchange-classify-question-${target.to}`, { count: target.messageIds.length })}
      </p>
      <label data-part="form.check" data-field="always">
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={always}
          onChange={(e) => {
            setAlways(e.target.checked);
          }}
        />
        <span data-part="form.check.label">
          {senders.length === 1
            ? t("exchange-classify-always-sender", { sender: senders[0] ?? "" })
            : t("exchange-classify-always")}
        </span>
      </label>
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button type="button" data-part="form.button" onClick={finish}>
          {t("exchange-cancel")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          disabled={busy}
          onClick={() => void move()}
        >
          {t(`exchange-move-to-${target.to}`)}
        </button>
      </div>
    </div>
  );
}
