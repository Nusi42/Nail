/**
 * The rule editor's footer (R2 design review, items 2, 4 and 5; R6, ADR
 * 0239): fixed under the form, so its buttons are always in view.
 *
 * - **Status** (first line): the shared problem badge (what keeps the rule
 *   from being saved, or why the text does not read; hover / focus lists
 *   the problems), "Saved" (fades, and goes with the next edit) or what the
 *   last run did, with Undo for all of it.
 * - **The shared footer** (`DialogFooter`): `Delete rule…` (destructive,
 *   asks first) for a saved rule at the start; Run on (the selected
 *   messages, the current folder, where the rule applies), Run ("Save and
 *   run" while the rule has unsaved changes; "Run 3 rules" with several
 *   rules selected in the list, R3) and Test rule in the middle, so a
 *   changing Run label never moves Save; Save rule (the primary, the form's
 *   submit button) at the end. The rules
 *   window is a tool window: Save rule keeps it open and there is no
 *   footer Close (× is enough).
 *
 * Parts: `rules.footer`, `rules.footer.status`, `rules.footer.state`
 * (`data-state` `saved` / `ran` / `undone` / `error`), then `dialog.footer`
 * with `rules.footer.tools` and `rules.footer.target`.
 */
import { useId } from "react";
import { useT } from "../../i18n/I18nProvider";
import { DialogFooter } from "../../widgets/dialog";
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
  /** Run's text (`runLabel`: how many rules, saving first). */
  runText: string;
  /** What keeps the shown rule from being saved also keeps Run from running. */
  runBlocked: boolean;
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
  runText,
  runBlocked,
  onRun,
  onTest,
}: FooterProps) {
  const t = useT();
  const targetId = useId();
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
      <DialogFooter
        {...(onDelete === undefined
          ? {}
          : {
              start: {
                label: t("rules-delete"),
                destructive: true,
                action: "delete-rule",
                disabled: busy,
                onClick: onDelete,
              },
            })}
        primary={{ label: t("rules-save"), submit: true, action: "save-rule", disabled: busy }}
      >
        <div data-part="rules.footer.tools">
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
            disabled={busy || runBlocked}
            onClick={onRun}
          >
            {runText}
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
        </div>
      </DialogFooter>
    </footer>
  );
}
