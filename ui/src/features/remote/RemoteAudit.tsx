/**
 * Settings → Remote access → Security log (layout widget `remote-audit`):
 * every login, failed login, pairing, lockout and administrative change of
 * the remote UI, newest first, filterable by user and paged with "load
 * older".
 */
import { useState } from "react";
import { errorMessageId, errorMessageIdOf } from "../../api/client";
import type { RemoteAuditEntry, RemoteUserId } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { SelectField } from "../../widgets/forms";
import { formatTime } from "./model";
import { useRemoteAudit, useRemoteUsers } from "./queries";

function Row({ entry }: { entry: RemoteAuditEntry }) {
  const t = useT();
  const locale = useLocale();
  const who = entry.user_name ?? t("remote-audit-unknown-user");
  return (
    <tr data-part="remote-audit.row" data-kind={entry.kind} data-outcome={entry.outcome}>
      <td data-part="remote-audit.time">{formatTime(entry.at, locale)}</td>
      <td data-part="remote-audit.event">
        <span data-part="remote-audit.kind">{t(`remote-audit-kind-${entry.kind}`)}</span>
        {entry.method !== null && (
          <span data-part="remote-audit.method">{t(`remote-method-${entry.method}`)}</span>
        )}
        {entry.name !== null && <span data-part="remote-audit.name">{entry.name}</span>}
      </td>
      <td data-part="remote-audit.user">{who}</td>
      <td data-part="remote-audit.origin">
        {entry.ip !== null && <span data-part="remote-audit.ip">{entry.ip}</span>}
        {entry.user_agent !== null && (
          <span data-part="remote-audit.agent">{entry.user_agent}</span>
        )}
      </td>
      <td data-part="remote-audit.outcome" data-outcome={entry.outcome}>
        {t(`remote-audit-outcome-${entry.outcome}`)}
        {entry.error !== null && (
          <span data-part="remote-audit.error">{t(errorMessageId(entry.error))}</span>
        )}
      </td>
    </tr>
  );
}

/** The `remote-audit` widget. */
export function RemoteAuditWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const users = useRemoteUsers().data ?? [];
  const [userId, setUserId] = useState<RemoteUserId | null>(null);
  const audit = useRemoteAudit(userId);
  const entries = audit.data?.pages.flat() ?? [];
  const failures = entries.filter((e) => e.outcome !== "success").length;

  return (
    <section
      data-part="remote-audit"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("remote-audit-title")}
      aria-busy={audit.isFetching}
    >
      <h3 data-part="remote-audit.title">{t("remote-audit-title")}</h3>
      <p data-part="remote-audit.intro">{t("remote-audit-intro")}</p>
      <div data-part="remote-audit.toolbar">
        <SelectField
          labelId="remote-audit-filter-user"
          name="audit-user"
          width="medium"
          value={userId ?? ""}
          onChange={(v) => {
            setUserId(v === "" ? null : v);
          }}
          choices={[
            { value: "", label: "remote-audit-all-users" },
            ...users.map((u) => ({ value: u.id, label: u.name, literal: true })),
          ]}
        />
        <button
          type="button"
          data-part="form.button"
          data-action="refresh-audit"
          onClick={() => void audit.refetch()}
        >
          {t("remote-audit-refresh")}
        </button>
      </div>
      {audit.isError && (
        <p data-part="remote-audit.error" role="alert">
          {t(errorMessageIdOf(audit.error))}
        </p>
      )}
      {audit.isSuccess && entries.length === 0 && (
        <p data-part="remote-audit.empty">{t("remote-audit-empty")}</p>
      )}
      {entries.length > 0 && (
        <>
          <p data-part="remote-audit.summary">
            {t("remote-audit-summary", { count: entries.length, failures })}
          </p>
          <table data-part="remote-audit.table" aria-label={t("remote-audit-title")}>
            <thead data-part="remote-audit.head">
              <tr>
                <th scope="col">{t("remote-audit-col-time")}</th>
                <th scope="col">{t("remote-audit-col-event")}</th>
                <th scope="col">{t("remote-audit-col-user")}</th>
                <th scope="col">{t("remote-audit-col-origin")}</th>
                <th scope="col">{t("remote-audit-col-outcome")}</th>
              </tr>
            </thead>
            <tbody data-part="remote-audit.body">
              {entries.map((e) => (
                <Row key={e.id} entry={e} />
              ))}
            </tbody>
          </table>
        </>
      )}
      {audit.hasNextPage && (
        <button
          type="button"
          data-part="form.button"
          data-action="load-older"
          disabled={audit.isFetchingNextPage}
          onClick={() => void audit.fetchNextPage()}
        >
          {t("remote-audit-load-older")}
        </button>
      )}
    </section>
  );
}
