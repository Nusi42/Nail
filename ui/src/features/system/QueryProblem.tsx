/**
 * What a desktop settings card shows when its query failed. Outside the
 * desktop app (`nail serve`, the remote UI) the core answers `unsupported`:
 * the card then says that the setting lives in the desktop app instead of
 * showing only its title or an error (P1, ADR 0144). Any other failure is
 * an error.
 */
import { errorMessageOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import { isUnsupported } from "../../state/queries";

export function QueryProblem({ error, part }: { error: unknown; part: string }) {
  const t = useT();
  if (isUnsupported(error)) {
    return (
      <p data-part="form.hint" data-state="desktop-only" role="status">
        {t("system-desktop-only")}
      </p>
    );
  }
  const message = errorMessageOf(error);
  return (
    <p data-part={part} role="alert">
      {t(message.id, message.args)}
    </p>
  );
}

/** Query retry rule: `unsupported` is final, anything else is tried once more. */
export function retryUnlessUnsupported(failures: number, error: unknown): boolean {
  return !isUnsupported(error) && failures < 1;
}
