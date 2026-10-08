/** "Close": the button that ends a dialog, a wizard or a view. */
import { useT } from "../i18n/I18nProvider";

export function CloseButton({
  onClose,
  action,
}: {
  onClose: () => void;
  /** `data-action` (tests and the theme tell the buttons apart by it). */
  action?: string;
}) {
  const t = useT();
  return (
    <button type="button" data-part="form.button" data-action={action} onClick={onClose}>
      {t("common-close")}
    </button>
  );
}
