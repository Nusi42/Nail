/**
 * The rules window's problems (R1, ADR 0219): errors and missing pieces as
 * the shared compact indicator (`widgets/ProblemIndicator`, ADR 0221), so
 * they never push the window's content around. It sits in a row that is
 * always there (the editor's view switch, the run panel's button row) and
 * shows only a count; the lines are its hover text and its popover.
 *
 * Parts of the list inside the details: `rules.issues.items` /
 * `rules.issues.item`.
 */
import { useT } from "../../i18n/I18nProvider";
import { ProblemIndicator, type ProblemSeverity } from "../../widgets/ProblemIndicator";

export function Issues({ kind, lines }: { kind: ProblemSeverity; lines: readonly string[] }) {
  const t = useT();
  if (lines.length === 0) return null;
  return (
    <ProblemIndicator
      severity={kind}
      label={t(kind === "error" ? "rules-issues-errors" : "rules-issues-warnings", {
        count: lines.length,
      })}
      title={lines.join("\n")}
    >
      <ul data-part="rules.issues.items">
        {lines.map((line, i) => (
          <li key={i} data-part="rules.issues.item">
            {line}
          </li>
        ))}
      </ul>
    </ProblemIndicator>
  );
}
