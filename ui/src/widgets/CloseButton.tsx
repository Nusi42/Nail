/**
 * "Close": the button that ends a view (the print view's toolbar) or a
 * dialog that has not moved to the shared footer yet. R6 (ADR 0239): a
 * dialog's finished state shows one Done (`DialogFooter`'s `done`), and
 * tool windows have no footer Close (× is enough); new dialogs do not use
 * this button.
 */
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
