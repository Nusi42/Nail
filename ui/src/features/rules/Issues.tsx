/**
 * The rules window's problems (R1, ADR 0219): errors and missing pieces as
 * the shared compact indicator (`widgets/ProblemIndicator`, ADR 0221), so
 * they never push the window's content around. R2: it sits in the footer,
 * which is always in view, and shows only a count; the lines are its hover
 * text and its popover, whose id (`detailsId`) the fields with a problem
 * and the Save button name in `aria-describedby`.
 *
 * Parts of the list inside the details: `rules.issues.items` /
 * `rules.issues.item`.
 */
import { useT } from "../../i18n/I18nProvider";
import { ProblemIndicator, type ProblemSeverity } from "../../widgets/ProblemIndicator";

export function Issues({
  kind,
  lines,
  detailsId,
}: {
  kind: ProblemSeverity;
  lines: readonly string[];
  /** The details' id (for `aria-describedby` elsewhere); generated when omitted. */
  detailsId?: string | undefined;
}) {
  const t = useT();
  if (lines.length === 0) return null;
  return (
    <ProblemIndicator
      severity={kind}
      detailsId={detailsId}
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
