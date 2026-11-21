/**
 * The rules window's lower panel (R2 design review, items 6 and 7): two
 * tabs under the form, always in view — **Preview** (what Test found on the
 * footer's "Run on" target) and **Recent runs (N)** of the rule.
 *
 * - Preview: a summary that takes the focus after Test, one row per
 *   matching message (subject — a button that opens the message —, sender,
 *   folder, the actions with their values, why it matched, folded), and
 *   "Apply to these N", which runs the rule on exactly those messages.
 *   With several rules selected (R3, ADR 0227) one group per rule, in file
 *   order, each with its own summary and "Apply to these N"; a note says
 *   that each rule was tested on its own (not one after another).
 * - Recent runs: when each run started, what started it, what it ran on,
 *   how many messages matched and its state; Undo where the core can still
 *   undo it, else a hover / focus text saying why not.
 *
 * Parts: `rules.lower`, `rules.lower.tabs`, `rules.lower.tab`
 * (`data-selected`), `rules.lower.panel`; inside, `rules.test.*` (several
 * rules: `rules.test.note`, `rules.test.group`, `rules.test.group.title`)
 * and `rules.runs` / `rules.run.*`.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, type KeyboardEvent, type Ref } from "react";
import type { FolderId, MessageSummary, Rule, RuleRun } from "../../api/generated";
import { DATE_TIME, useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { cachedFolder } from "../../state/mailOps";
import { folderTitle } from "../../widgets/ListHeader";
import { HORIZONTAL, moveRovingFocus } from "../../widgets/rovingFocus";
import { useRuleRuns } from "./api";
import { actionText, runTargetText, whyNoUndo } from "./describe";
import { comparePaths, conditionAt } from "./model";
import type { ConditionHit, ExplainedMatch } from "./types";

/** The panel's tabs. */
export type LowerTab = "preview" | "runs";

/** What Test found for one rule. */
export interface TestGroup {
  /** The rule as tested (the shown one as edited). */
  rule: Rule;
  scanned: number;
  matches: readonly ExplainedMatch[];
}

/** What Test found, per rule in file order, and on what. */
export interface TestResult {
  groups: readonly TestGroup[];
  /** The "Run on" target it looked at (localized). */
  target: string;
}

/** A folder's name as the folder list shows it, `null` while it is not loaded. */
function useFolderName(): (id: FolderId) => string | null {
  const t = useT();
  const queryClient = useQueryClient();
  return (id) => {
    const title = folderTitle(cachedFolder(queryClient, id), null);
    if (title === null) return null;
    return "id" in title ? t(title.id) : title.text;
  };
}

/** How a condition is named in explanations: its kind. */
function conditionLabel(rule: Rule, path: readonly number[]): string {
  const c = conditionAt(rule.conditions, path);
  return c ? `rules-cond-${c.type}` : "rules-cond-all";
}

function Explanation({ rule, hits }: { rule: Rule; hits: readonly ConditionHit[] }) {
  const t = useT();
  // Leaves that matched, in tree order: what made the rule match.
  const reasons = hits
    .filter((h) => h.matched && h.path.length > 0)
    .filter((h) => {
      const c = conditionAt(rule.conditions, h.path);
      return c !== undefined && c.type !== "all" && c.type !== "any" && c.type !== "not";
    })
    .sort((a, b) => comparePaths(a.path, b.path));
  if (reasons.length === 0) return null;
  return (
    <details data-part="rules.test.details">
      <summary data-part="rules.test.details.summary">{t("rules-test-why")}</summary>
      <ul data-part="rules.test.why" aria-label={t("rules-test-why")}>
        {reasons.map((h) => (
          <li key={h.path.join(".")} data-part="rules.test.reason">
            {h.value === null
              ? t("rules-hit", { condition: t(conditionLabel(rule, h.path)) })
              : t("rules-hit-value", {
                  condition: t(conditionLabel(rule, h.path)),
                  value: h.value,
                })}
          </li>
        ))}
      </ul>
    </details>
  );
}

export interface PreviewProps {
  result: TestResult | null;
  /** The summary line takes the focus after Test. */
  summaryRef: Ref<HTMLParagraphElement>;
  busy: boolean;
  /** Opens a message in the main window. */
  onOpen: (message: MessageSummary) => void;
  /** Runs `rule` on exactly these messages. */
  onApply: (rule: Rule, matches: readonly ExplainedMatch[]) => void;
}

/** The total matches of a test of several rules (a message once per rule it matches). */
export function totalMatches(groups: readonly Pick<TestGroup, "matches">[]): number {
  return groups.reduce((n, g) => n + g.matches.length, 0);
}

interface GroupProps extends Pick<PreviewProps, "busy" | "onOpen" | "onApply"> {
  group: TestGroup;
  target: string;
  /** Only the first summary takes the focus after Test. */
  summaryRef?: Ref<HTMLParagraphElement> | undefined;
}

/** One rule's matches: its summary, "Apply to these N", the matching messages. */
function MatchGroup({ group, target, summaryRef, busy, onOpen, onApply }: GroupProps) {
  const t = useT();
  const folderName = useFolderName();
  const { rule } = group;
  const count = group.matches.length;
  return (
    <>
      <div data-part="rules.test.head">
        <p
          data-part="rules.test.summary"
          role={summaryRef ? "status" : undefined}
          ref={summaryRef}
          tabIndex={summaryRef ? -1 : undefined}
        >
          {t("rules-test-summary-on", { matched: count, scanned: group.scanned, target })}
        </p>
        {count > 0 && (
          <button
            type="button"
            data-part="form.button"
            data-action="apply-matches"
            disabled={busy}
            onClick={() => {
              onApply(rule, group.matches);
            }}
          >
            {t("rules-apply-matches", { count })}
          </button>
        )}
      </div>
      {count === 0 ? (
        <p data-part="rules.hint">{t("rules-test-none")}</p>
      ) : (
        <ul data-part="rules.test.matches">
          {group.matches.map((m) => (
            <li key={m.message.id} data-part="rules.test.match">
              <button
                type="button"
                data-part="rules.test.subject"
                title={t("rules-test-open")}
                onClick={() => {
                  onOpen(m.message);
                }}
              >
                {m.message.subject}
              </button>
              <span data-part="rules.test.from">
                {m.message.from?.name ?? m.message.from?.email ?? ""}
              </span>
              <span data-part="rules.test.folder">{folderName(m.message.folder_id) ?? ""}</span>
              <ul data-part="rules.test.actions" aria-label={t("rules-test-actions")}>
                {m.actions.map((a, i) => (
                  <li key={i} data-part="rules.test.action" data-kind={a.type}>
                    {actionText(t, a)}
                  </li>
                ))}
              </ul>
              {m.explanation.length > 0 && <Explanation rule={rule} hits={m.explanation} />}
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

function Preview({ result, summaryRef, ...group }: PreviewProps) {
  const t = useT();
  if (result === null) {
    return (
      <p data-part="rules.hint" ref={summaryRef} tabIndex={-1}>
        {t("rules-preview-empty")}
      </p>
    );
  }
  const [only] = result.groups;
  if (result.groups.length === 1 && only) {
    return (
      <section data-part="rules.test" aria-label={t("rules-test-title")}>
        <MatchGroup {...group} group={only} target={result.target} summaryRef={summaryRef} />
      </section>
    );
  }
  return (
    <section data-part="rules.test" aria-label={t("rules-test-title")}>
      <p data-part="rules.test.summary" role="status" ref={summaryRef} tabIndex={-1}>
        {t("rules-test-summary-many", {
          rules: result.groups.length,
          matched: totalMatches(result.groups),
          target: result.target,
        })}
      </p>
      <p data-part="rules.test.note">{t("rules-test-separately")}</p>
      {result.groups.map((g) => (
        <section
          key={g.rule.id}
          data-part="rules.test.group"
          data-rule={g.rule.id}
          aria-label={g.rule.name}
        >
          <h3 data-part="rules.test.group.title">{g.rule.name}</h3>
          <MatchGroup {...group} group={g} target={result.target} />
        </section>
      ))}
    </section>
  );
}

function RunRow({
  run,
  busy,
  onUndo,
}: {
  run: RuleRun;
  busy: boolean;
  onUndo: (run: RuleRun) => void;
}) {
  const t = useT();
  const id = useId();
  const dates = useDateFormat();
  const folderName = useFolderName();
  const why = whyNoUndo(run);
  return (
    <li data-part="rules.run" data-state={run.state}>
      <time data-part="rules.run.time" dateTime={run.started_at}>
        {dates.dateTime(run.started_at, DATE_TIME)}
      </time>
      <span data-part="rules.run.summary">
        {t("rules-run-summary", {
          cause: t(`rules-run-cause-${run.cause}`),
          matched: run.matched,
          state: t(`rules-run-state-${run.state}`),
        })}
      </span>
      <span data-part="rules.run.target">{runTargetText(t, run.target, folderName)}</span>
      {why === null ? (
        <button
          type="button"
          data-part="form.button"
          data-action="undo-run"
          disabled={busy}
          onClick={() => {
            onUndo(run);
          }}
        >
          {t("rules-undo")}
        </button>
      ) : (
        // Undo that cannot: it stays focusable, so the reason reaches the
        // keyboard (description) as well as the pointer (hover text).
        <>
          <button
            type="button"
            data-part="form.button"
            data-action="undo-run"
            aria-disabled="true"
            title={t(why)}
            aria-describedby={`${id}-why`}
          >
            {t("rules-undo")}
          </button>
          <span data-part="rules.run.why" id={`${id}-why`}>
            {t(why)}
          </span>
        </>
      )}
    </li>
  );
}

function RecentRuns({
  runs,
  busy,
  onUndo,
}: {
  runs: readonly RuleRun[];
  busy: boolean;
  onUndo: (run: RuleRun) => void;
}) {
  const t = useT();
  return (
    <section data-part="rules.runs" aria-label={t("rules-runs-title")}>
      {runs.length === 0 ? (
        <p data-part="rules.hint">{t("rules-runs-none")}</p>
      ) : (
        <ul data-part="rules.runs.items">
          {runs.map((run) => (
            <RunRow key={run.id} run={run} busy={busy} onUndo={onUndo} />
          ))}
        </ul>
      )}
    </section>
  );
}

export interface LowerPanelProps extends PreviewProps {
  tab: LowerTab;
  onTab: (tab: LowerTab) => void;
  /** The saved rule's id; `null` while it is new (no runs yet). */
  ruleId: string | null;
  onUndo: (run: RuleRun) => void;
}

const TABS: readonly LowerTab[] = ["preview", "runs"];

export function LowerPanel({ tab, onTab, ruleId, onUndo, ...preview }: LowerPanelProps) {
  const t = useT();
  const id = useId();
  const runs = useRuleRuns(ruleId).data ?? [];
  const label = (which: LowerTab) =>
    which === "preview" ? t("rules-tab-preview") : t("rules-tab-runs", { count: runs.length });
  return (
    <div data-part="rules.lower">
      {/* eslint-disable-next-line jsx-a11y/interactive-supports-focus -- the tabs take the focus (roving tabindex), not the tab list itself. */}
      <div
        data-part="rules.lower.tabs"
        role="tablist"
        aria-label={t("rules-lower-label")}
        onKeyDown={(event: KeyboardEvent<HTMLElement>) => {
          moveRovingFocus(event, '[role="tab"]', HORIZONTAL);
          const focused = document.activeElement;
          const next = TABS.find((which) => focused?.id === `${id}-tab-${which}`);
          if (next !== undefined && next !== tab) onTab(next);
        }}
      >
        {TABS.map((which) => (
          <button
            key={which}
            type="button"
            role="tab"
            id={`${id}-tab-${which}`}
            data-part="rules.lower.tab"
            data-tab={which}
            data-selected={which === tab}
            aria-selected={which === tab}
            aria-controls={`${id}-panel`}
            tabIndex={which === tab ? 0 : -1}
            onClick={() => {
              onTab(which);
            }}
          >
            {label(which)}
          </button>
        ))}
      </div>
      <div
        data-part="rules.lower.panel"
        role="tabpanel"
        id={`${id}-panel`}
        aria-labelledby={`${id}-tab-${tab}`}
      >
        {tab === "preview" ? (
          <Preview {...preview} />
        ) : (
          <RecentRuns runs={runs} busy={preview.busy} onUndo={onUndo} />
        )}
      </div>
    </div>
  );
}
