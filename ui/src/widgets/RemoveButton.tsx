/** "Remove" for one entry of a list editor; its accessible name says which. */
import { useT } from "../i18n/I18nProvider";

export function RemoveButton({
  label,
  action = "remove",
  onRemove,
}: {
  /** The accessible name (already translated), e.g. "Remove reminder 2". */
  label: string;
  /** `data-action` (tests and the theme tell the buttons apart by it). */
  action?: string;
  onRemove: () => void;
}) {
  const t = useT();
  return (
    <button
      type="button"
      data-part="form.button"
      data-action={action}
      aria-label={label}
      onClick={onRemove}
    >
      {t("common-remove")}
    </button>
  );
}
