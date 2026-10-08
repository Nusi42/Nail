/**
 * The MCP audit log: every call an agent made, newest first, with its
 * outcome. Pages with "load more"; refreshes on demand.
 */
import { errorMessageIdOf } from "../../api/client";
import type { McpAuditEntry, McpClientId } from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useMcpAudit } from "./queries";
import { SHORT_DATE_TIME_SECONDS, formatDateTime } from "../../i18n/format";
import { RefreshButton } from "../../widgets/RefreshButton";

/** A date-time for the log (the raw string when it does not parse). */
export function formatAuditTime(iso: string, locale: string): string {
  return formatDateTime(iso, locale, SHORT_DATE_TIME_SECONDS);
}

function Row({ entry }: { entry: McpAuditEntry }) {
  const t = useT();
  const locale = useLocale();
  return (
    <tr data-part="mcp-audit.row" data-outcome={entry.outcome} data-kind={entry.kind}>
      <td data-part="mcp-audit.time">{formatAuditTime(entry.at, locale)}</td>
      <td data-part="mcp-audit.client">
        {entry.client_name === "" ? t("mcp-audit-unknown-client") : entry.client_name}
      </td>
      <td data-part="mcp-audit.call">
        <span data-part="mcp-audit.kind">{t(`mcp-audit-kind-${entry.kind}`)}</span>
        <span data-part="mcp-audit.name">{entry.name}</span>
      </td>
      <td data-part="mcp-audit.outcome" data-outcome={entry.outcome}>
        {t(`mcp-audit-outcome-${entry.outcome}`)}
      </td>
      <td data-part="mcp-audit.detail">
        {entry.localized ? t(entry.localized.id, entry.localized.args) : (entry.detail ?? "")}
      </td>
    </tr>
  );
}

/** The log of one client, or of all clients when `clientId` is `null`. */
export function AuditLog({ clientId }: { clientId: McpClientId | null }) {
  const t = useT();
  const audit = useMcpAudit(clientId);
  const entries = audit.data?.pages.flat() ?? [];
  return (
    <section
      data-part="mcp-audit"
      data-client={clientId ?? undefined}
      aria-label={t("mcp-audit-label")}
      aria-busy={audit.isFetching}
    >
      <div data-part="mcp-audit.toolbar">
        <RefreshButton action="refresh-audit" refetch={audit.refetch} />
      </div>
      {audit.isError && (
        <div data-part="mcp-audit.error" role="alert">
          {t(errorMessageIdOf(audit.error))}
        </div>
      )}
      {audit.isSuccess && entries.length === 0 && (
        <div data-part="mcp-audit.empty">{t("mcp-audit-empty")}</div>
      )}
      {entries.length > 0 && (
        <table data-part="mcp-audit.table">
          <thead data-part="mcp-audit.head">
            <tr>
              <th scope="col">{t("common-time")}</th>
              <th scope="col">{t("mcp-audit-client")}</th>
              <th scope="col">{t("mcp-audit-call")}</th>
              <th scope="col">{t("common-result")}</th>
              <th scope="col">{t("mcp-audit-detail")}</th>
            </tr>
          </thead>
          <tbody data-part="mcp-audit.body">
            {entries.map((entry) => (
              <Row key={entry.id} entry={entry} />
            ))}
          </tbody>
        </table>
      )}
      {audit.hasNextPage && (
        <button
          type="button"
          data-part="form.button"
          data-action="more-audit"
          disabled={audit.isFetchingNextPage}
          onClick={() => void audit.fetchNextPage()}
        >
          {t("mcp-audit-more")}
        </button>
      )}
    </section>
  );
}
