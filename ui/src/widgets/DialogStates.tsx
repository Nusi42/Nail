/**
 * What a feature's dialog shows before its content: nothing to show (the
 * thing it opened for is gone) or still loading. The feature names its
 * parts: the dialog's (`data-state` `empty` / `loading`) and the text's.
 */
import { useT } from "../i18n/I18nProvider";

interface DialogStateProps {
  /** The dialog's part, e.g. `exchange.dialog`. */
  part: string;
  /** The text's part, e.g. `exchange.hint`. */
  textPart: string;
}

/** The text `hintId` and a Close button. */
export function EmptyDialog({
  part,
  textPart,
  hintId,
  close,
}: DialogStateProps & { hintId: string; close: () => void }) {
  const t = useT();
  return (
    <div data-part={part} data-state="empty">
      <p data-part={textPart}>{t(hintId)}</p>
      <button type="button" data-part="form.button" onClick={close}>
        {t("common-close")}
      </button>
    </div>
  );
}

/** "Loading…" while the dialog's data arrives. */
export function LoadingDialog({ part, textPart }: DialogStateProps) {
  const t = useT();
  return (
    <div data-part={part} data-state="loading" aria-busy="true">
      <p data-part={textPart}>{t("common-loading")}</p>
    </div>
  );
}
