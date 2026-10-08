/**
 * The confirmation center (layout widget `confirmation-center`, also the
 * dialog `confirmation-center`): writes by AI agents (MCP clients) and
 * plug-ins that wait for the user, oldest first, and the history of
 * decided ones, newest first, paged.
 *
 * Widget options: `show-history` (default `#true`).
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { ConfirmationCard } from "./ConfirmationCard";
import { useConfirmationHistory, usePendingConfirmations } from "./queries";
import { useAgentsState } from "./store";

type View = "pending" | "history";

function Pending() {
  const t = useT();
  const pending = usePendingConfirmations();
  const focus = useAgentsState((s) => s.focus);
  const dismiss = useAgentsState((s) => s.dismiss);
  const list = pending.data ?? [];
  return (
    <div data-part="confirmation-center.pending">
      {pending.isError && (
        <p data-part="confirmation-center.error" role="alert">
          {t(errorMessageOf(pending.error).id)}
        </p>
      )}
      {pending.isSuccess && list.length === 0 && (
        <p data-part="confirmation-center.empty">{t("agents-center-empty")}</p>
      )}
      {list.length > 0 && (
        <ul data-part="confirmation-center.list" aria-label={t("agents-center-pending-label")}>
          {list.map((confirmation) => (
            <li key={confirmation.id} data-part="confirmation-center.item">
              <ConfirmationCard
                confirmation={confirmation}
                focused={confirmation.id === focus}
                onDecided={(decided) => {
                  dismiss(decided.id);
                }}
              />
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function History() {
  const t = useT();
  const history = useConfirmationHistory();
  const list = history.data?.pages.flat() ?? [];
  return (
    <div data-part="confirmation-center.history">
      {history.isError && (
        <p data-part="confirmation-center.error" role="alert">
          {t(errorMessageOf(history.error).id)}
        </p>
      )}
      {history.isSuccess && list.length === 0 && (
        <p data-part="confirmation-center.empty">{t("agents-history-empty")}</p>
      )}
      {list.length > 0 && (
        <ul data-part="confirmation-center.list" aria-label={t("agents-center-history-label")}>
          {list.map((confirmation) => (
            <li key={confirmation.id} data-part="confirmation-center.item">
              <ConfirmationCard confirmation={confirmation} />
            </li>
          ))}
        </ul>
      )}
      {history.hasNextPage && (
        <button
          type="button"
          data-part="form.button"
          data-action="more"
          disabled={history.isFetchingNextPage}
          onClick={() => void history.fetchNextPage()}
        >
          {t("agents-history-more")}
        </button>
      )}
    </div>
  );
}

/** The center's content (pending / history switch and the list). */
export function CenterContent({ showHistory = true }: { showHistory?: boolean }) {
  const t = useT();
  const [view, setView] = useState<View>("pending");
  const pending = usePendingConfirmations();
  const count = pending.data?.length ?? 0;
  const current = showHistory ? view : "pending";
  return (
    <>
      <p data-part="confirmation-center.intro">{t("agents-center-intro")}</p>
      {showHistory && (
        <div data-part="confirmation-center.views" role="group" aria-label={t("common-show")}>
          <button
            type="button"
            data-part="confirmation-center.view"
            data-view="pending"
            data-state={current === "pending" ? "selected" : undefined}
            aria-pressed={current === "pending"}
            onClick={() => {
              setView("pending");
            }}
          >
            {t("agents-center-show-pending", { count })}
          </button>
          <button
            type="button"
            data-part="confirmation-center.view"
            data-view="history"
            data-state={current === "history" ? "selected" : undefined}
            aria-pressed={current === "history"}
            onClick={() => {
              setView("history");
            }}
          >
            {t("agents-center-show-history")}
          </button>
        </div>
      )}
      {current === "pending" ? <Pending /> : <History />}
    </>
  );
}

/** Widget `confirmation-center`. */
export function ConfirmationCenter({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  return (
    <section
      data-part="confirmation-center"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("agents-center-title")}
    >
      <CenterContent showHistory={optionBool(node, "show-history", true)} />
    </section>
  );
}

/** Dialog `confirmation-center` (title `window-confirmation-center`). */
export function ConfirmationCenterDialog() {
  return (
    <div data-part="confirmation-center">
      <CenterContent />
    </div>
  );
}
