/**
 * The rule editor's footer (R2 design review, items 2, 4 and 5): fixed
 * under the form, so its buttons are always in view.
 *
 * - **Status** (left): the shared problem badge (what keeps the rule from
 *   being saved, or why the text does not read; hover / focus lists the
 *   problems), "Saved" (fades, and goes with the next edit) or what the
 *   last run did, with Undo for all of it.
 * - **Delete…** (destructive, asks first) for a saved rule.
 * - **Run on** (the selected messages, the current folder, where the rule
 *   applies), **Run** ("Save and run" while the rule has unsaved changes),
 *   **Test** and **Save** (primary). Save stays focusable while something
 *   is missing (`aria-disabled`) and names the problem badge.
 *
 * Parts: `rules.footer`, `rules.footer.status`, `rules.footer.state`
 * (`data-state` `saved` / `ran` / `undone` / `error`), `rules.footer.start`,
 * `rules.footer.end`, `rules.footer.target`.
 */
import { useId } from "react";
import { useT } from "../../i18n/I18nProvider";
import { Issues } from "./Issues";
import type { RunOn } from "./runOn";
import type { RunTargetKind } from "./runTargets";

/** What the status line says. */
export type FooterState =
  | { kind: "saved" }
  | { kind: "ran"; text: string; undo: (() => void) | null }
  | { kind: "undone"; text: string }
  | null;

export interface FooterProps {
  /** Problems, localized; `severity` error when the rule cannot be read at all. */
  problems: readonly string[];
  severity: "error" | "warning";
  /** The problem details' id (fields and Save name it). */
  detailsId: string;
  state: FooterState;
  busy: boolean;
  /** Delete…; omitted for a new rule. */
  onDelete?: (() => void) | undefined;
  runOn: RunOn;
  /** The rule has unsaved changes: Run saves it first. */
  unsaved: boolean;
  onRun: () => void;
  onTest: () => void;
}

export function Footer({
  problems,
  severity,
  detailsId,
  state,
  busy,
  onDelete,
  runOn,
  unsaved,
  onRun,
  onTest,
}: FooterProps) {
  const t = useT();
  const targetId = useId();
  const blocked = problems.length > 0;
  const runChoice = runOn.choices.find((c) => c.kind === runOn.kind);
  return (
    <footer data-part="rules.footer">
      <div data-part="rules.footer.status">
        <Issues kind={severity} lines={problems} detailsId={detailsId} />
        <span data-part="rules.footer.state" data-state={state?.kind} role="status">
          {state === null ? "" : state.kind === "saved" ? t("rules-saved") : state.text}
        </span>
        {state?.kind === "ran" && state.undo !== null && (
          <button
            type="button"
            data-part="form.button"
            data-action="undo-runs"
            disabled={busy}
            onClick={state.undo}
          >
            {t("rules-undo-all")}
          </button>
        )}
      </div>
      <div data-part="rules.footer.start">
        {onDelete && (
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            data-action="delete-rule"
            disabled={busy}
            onClick={onDelete}
          >
            {t("rules-delete")}
          </button>
        )}
      </div>
      <div data-part="rules.footer.end">
        <span data-part="rules.footer.target">
          <label data-part="form.label" htmlFor={targetId}>
            {t("rules-target-legend")}
          </label>
          <select
            id={targetId}
            data-part="form.select"
            data-width="content"
            name="run-target"
            value={runOn.kind}
            title={runChoice?.why ?? undefined}
            onChange={(event) => {
              runOn.choose(event.target.value as RunTargetKind);
            }}
          >
            {runOn.choices.map((c) => (
              <option
                key={c.kind}
                value={c.kind}
                disabled={c.why !== null}
                title={c.why ?? undefined}
              >
                {c.label}
              </option>
            ))}
          </select>
        </span>
        <button
          type="button"
          data-part="form.button"
          data-action="run-rules"
          disabled={busy}
          aria-disabled={blocked || undefined}
          aria-describedby={blocked ? detailsId : undefined}
          onClick={onRun}
        >
          {t(unsaved ? "rules-save-and-run" : "rules-run")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="test-rule"
          disabled={busy}
          onClick={onTest}
        >
          {t("rules-test")}
        </button>
        <button
          type="submit"
          data-part="form.button"
          data-variant="primary"
          data-action="save-rule"
          disabled={busy}
          aria-disabled={blocked || undefined}
          aria-describedby={blocked ? detailsId : undefined}
        >
          {t("rules-save")}
        </button>
      </div>
    </footer>
  );
}
