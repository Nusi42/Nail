/** "Refresh": fetches a query again (audit logs, plug-in logs). */
import { useT } from "../i18n/I18nProvider";

export function RefreshButton({
  action = "refresh",
  refetch,
}: {
  /** `data-action` (tests and the theme tell the buttons apart by it). */
  action?: string;
  refetch: () => Promise<unknown>;
}) {
  const t = useT();
  return (
    <button
      type="button"
      data-part="form.button"
      data-action={action}
      onClick={() => void refetch()}
    >
      {t("common-refresh")}
    </button>
  );
}
