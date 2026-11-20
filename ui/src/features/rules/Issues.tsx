/**
 * The rules window's compact problem indicator (R1, ADR 0219): errors and
 * missing pieces never push the window's content around. The indicator
 * sits in a row that is always there (the editor's view switch, the run
 * panel's button row) and shows only a count; its explanation is the
 * indicator's hover text (`title`) and a popover in the top layer that
 * Enter / Space or a click opens (native `popover`, so Escape and a click
 * outside close it). The popover is the button's description, so a screen
 * reader reads the problems with the button.
 *
 * Parts: `rules.issues` (`data-kind` = `error` / `warning`), its button
 * `rules.issues.badge` (+ `.icon`), the popover `rules.issues.details` with
 * `rules.issues.items` / `rules.issues.item`.
 */
import { useId } from "react";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";

/** `error`: something failed; `warning`: the rule is not ready to save yet. */
export type IssueKind = "error" | "warning";

export function Issues({ kind, lines }: { kind: IssueKind; lines: readonly string[] }) {
  const t = useT();
  const id = useId();
  if (lines.length === 0) return null;
  return (
    <span data-part="rules.issues" data-kind={kind} role={kind === "error" ? "alert" : undefined}>
      <button
        type="button"
        data-part="rules.issues.badge"
        data-kind={kind}
        title={lines.join("\n")}
        aria-describedby={id}
        popoverTarget={id}
      >
        <Icon name="circle-alert" part="rules.issues.icon" />
        {t(kind === "error" ? "rules-issues-errors" : "rules-issues-warnings", {
          count: lines.length,
        })}
      </button>
      <div id={id} popover="auto" data-part="rules.issues.details" data-kind={kind}>
        <ul data-part="rules.issues.items">
          {lines.map((line, i) => (
            <li key={i} data-part="rules.issues.item">
              {line}
            </li>
          ))}
        </ul>
      </div>
    </span>
  );
}
