/**
 * Editing one rule (R2 design review of the rules window, ADR 0222):
 *
 * - **Header:** the name as a heading field, On / Off, and the Editor / Text
 *   switch (a radio group: arrow keys move and switch).
 * - **Body:** the compact form (`./RuleForm`: WHEN / WHERE / IF / THEN) or
 *   the rule as `rules.kdl` text (`./TextView`, highlighted, with line
 *   numbers).
 * - **Lower panel** (`./LowerPanel`): Preview (Test's matches on the "Run
 *   on" target; "Apply to these N") and Recent runs.
 * - **Footer** (`./Footer`), always in view: the problem badge, "Saved" or
 *   what a run did, Delete…, Run on, Run, Test, Save.
 *
 * Text and Editor: switching to Text writes the rule as text (the latest
 * rule, even when it changed while the text was being written); switching
 * back reads the text as a *draft* (`RuleFromText { draft: true }`), so an
 * unfinished rule (no name yet, empty values) comes back with its gaps as
 * problems instead of being lost; unchanged text is not read at all. Text
 * that does not read keeps the text view, shows why (with line and column)
 * and offers to discard the text changes. Edited text is read in the
 * background once typing pauses, so the footer judges the rule Save would
 * write in both views.
 *
 * Several rules selected in the list (R3, ADR 0227): Run runs them all in
 * file order on the "Run on" target (one `RunRules`; the shown rule saved
 * first when it is one of them and has changes), and asks first with the
 * total preview count when one of them moves or deletes mail. Test tests
 * each on its own (one `TestRule` per rule) and the Preview groups the
 * matches by rule; it does not simulate the rules one after another.
 * Save, Delete and the Text view act on the shown rule only.
 *
 * Unsaved changes: the editor tells the rules store (`setDirty`), so
 * switching rules or closing the window asks first (`./store.ts`,
 * `./closeGuard.ts`). Delete and a run that moves or deletes mail ask too,
 * inside the window (`./Confirm`). Any edit clears "Saved" and the
 * preview, which would describe an older rule.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import type { ConfigIssue, MessageSummary, Rule, RuleId, RuleRun } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useBusyAction } from "../../state/busyAction";
import { TYPING_PAUSE_MS, useDebounced } from "../../state/debounce";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { Segmented } from "../../widgets/primitives";
import {
  deleteRule,
  ruleFromText,
  ruleToText,
  rulesKeys,
  RULE_SCOPE_TARGET,
  runRules,
  saveRule,
  testRule,
  undoRuleRun,
  type RuleRunTarget,
} from "./api";
import { Confirm } from "./Confirm";
import { Footer, type FooterState } from "./Footer";
import { LowerPanel, type LowerTab, type TestGroup, type TestResult } from "./LowerPanel";
import { isDestructive, ruleIssues } from "./model";
import { RuleForm, RuleHeader } from "./RuleForm";
import { refreshRuns } from "./runMenu";
import { useRunOn } from "./runOn";
import {
  outcomeMessage,
  outcomeOf,
  ruleRunTargetOf,
  undoRuns,
  type RunOutcome,
} from "./runTargets";
import { useRulesUi } from "./store";
import { TextView } from "./TextView";
import type { ExplainedMatch } from "./types";

/** Maximum number of matches a test shows. */
export const TEST_LIMIT = 50;
/**
 * The `limit` of the count before a run that moves or deletes mail: the
 * largest the protocol takes (`u32`). The core caps it at what a run on
 * the same target looks at, so the count covers every message the run
 * acts on.
 */
export const COUNT_LIMIT = 0xffff_ffff;

type View = "visual" | "text";

/** The root condition as a group, so the editor can add to it. */
function asGroup(c: Rule["conditions"]): Rule["conditions"] {
  return c.type === "all" || c.type === "any" ? c : { type: "all", conditions: [c] };
}

/** A rule as the editor holds it (its root condition a group). */
function editable(rule: Rule): Rule {
  return { ...rule, conditions: asGroup(rule.conditions) };
}

/** Whether the editor's rule differs from the saved one. */
export function ruleChanged(edited: Rule, saved: Rule): boolean {
  return JSON.stringify(edited) !== JSON.stringify(saved);
}

/** A text-view error with its position. */
function issueText(t: ReturnType<typeof useT>, e: ConfigIssue): string {
  return t("common-error-at-line", {
    line: e.line,
    column: e.column,
    message: e.localized ? t(e.localized.id, e.localized.args) : e.message,
  });
}

/** Edited text as last read: the draft rule (`null`: it does not read) and the problems. */
interface TextRead {
  text: string;
  rule: Rule | null;
  errors: readonly ConfigIssue[];
}

/**
 * A question the editor asks over its content: before a run that moves or
 * deletes mail, the rules that run and their total preview count.
 */
type Question =
  | { kind: "delete" }
  | {
      kind: "run";
      rules: readonly Rule[];
      count: number;
      target: string;
      go: () => void;
    };

/** The Run button's text: how many rules, and whether the shown rule is saved first. */
export function runLabel(
  t: ReturnType<typeof useT>,
  { count, savesFirst }: { count: number; savesFirst: boolean },
): string {
  if (count > 1) return t(savesFirst ? "rules-save-and-run-many" : "rules-run-many", { count });
  return t(savesFirst ? "rules-save-and-run" : "rules-run");
}

/**
 * Edits `initial` (a saved rule, or a new one with an empty id). Calls
 * `onSaved` with the saved rule and `onDeleted` after a delete. `selection`:
 * the saved rules selected in the list when there are several (in file
 * order), which Run and Test act on; else empty (the shown rule).
 */
export function RuleEditor({
  initial,
  selection,
  onSaved,
  onDeleted,
}: {
  initial: Rule;
  selection: readonly Rule[];
  onSaved: (rule: Rule) => void;
  onDeleted: () => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ui = useUiStoreApi();
  const detailsId = useId();
  const [rule, setRuleState] = useState<Rule>(() => editable(initial));
  // The rule as last saved (a new rule: as it started).
  const [savedRule, setSavedRule] = useState<Rule>(() => editable(initial));
  const [view, setView] = useState<View>("visual");
  const [text, setTextState] = useState("");
  // The text as the core wrote it: unchanged text needs no reading.
  const [written, setWritten] = useState("");
  const [read, setRead] = useState<TextRead | null>(null);
  // Switching back to Editor failed: the text does not read.
  const [stuck, setStuck] = useState(false);
  const [footer, setFooter] = useState<FooterState>(null);
  const [tested, setTested] = useState<TestResult | null>(null);
  const [tab, setTab] = useState<LowerTab>("preview");
  const [question, setQuestion] = useState<Question | null>(null);
  const { busy, problem, run } = useBusyAction();
  const runOn = useRunOn();
  const pending = useRulesUi((s) => s.pending);
  const discard = useRulesUi((s) => s.discard);
  const keepEditing = useRulesUi((s) => s.keepEditing);
  const setDirty = useRulesUi((s) => s.setDirty);
  const setAsking = useRulesUi((s) => s.setAsking);
  const forget = useRulesUi((s) => s.forget);
  const summary = useRef<HTMLParagraphElement>(null);
  const focusSummary = useRef(false);
  // The rule now, for work that awaits (the text of the latest rule).
  const latest = useRef(rule);
  useEffect(() => {
    latest.current = rule;
  }, [rule]);
  const isNew = savedRule.id === "";
  /** Whether `r` is the rule shown here (a selected rule's saved copy, or itself). */
  const isShown = (r: Rule) => r.id === savedRule.id;
  // Run and Test act on the selected rules, else on the shown one.
  const many = selection.length > 1;
  const shownRuns = !many || selection.some(isShown);

  /** Something changed: "Saved" and the preview describe an older rule. */
  const edited = () => {
    setFooter(null);
    setTested(null);
  };
  const setRule = (next: Rule) => {
    latest.current = next;
    setRuleState(next);
    edited();
  };
  const setText = (next: string) => {
    setTextState(next);
    setStuck(false);
    edited();
  };

  /** A rule read from the text: it keeps this rule's identity whatever the text says. */
  const own = (parsed: Rule): Rule => editable({ ...parsed, id: rule.id, source: rule.source });

  /** Reads `source` as a draft (and remembers it). */
  const readText = async (source: string): Promise<TextRead> => {
    const parsed = await ruleFromText(client, source, true);
    const result = { text: source, rule: parsed.rule, errors: parsed.errors };
    setRead(result);
    return result;
  };

  // Edited text is read once typing pauses, so the footer knows what Save would write.
  const textEdited = view === "text" && text !== written;
  const typed = useDebounced(text, TYPING_PAUSE_MS);
  useEffect(() => {
    if (view !== "text" || typed !== text || typed === written) return;
    let live = true;
    ruleFromText(client, typed, true).then(
      (parsed) => {
        if (live) setRead({ text: typed, rule: parsed.rule, errors: parsed.errors });
      },
      () => {
        if (live) setRead({ text: typed, rule: null, errors: [] });
      },
    );
    return () => {
      live = false;
    };
  }, [client, view, typed, text, written]);

  // The rule Save would write: `undefined` while edited text is not read
  // yet, `null` when it does not read.
  const draft: Rule | null | undefined = !textEdited
    ? rule
    : read?.text === text
      ? read.rule && own(read.rule)
      : undefined;
  const issues = draft ? ruleIssues(draft) : [];
  const textErrors = textEdited && read?.text === text ? read.errors : [];
  const dirty = draft === undefined || draft === null || ruleChanged(draft, savedRule);
  const unsaved = isNew || dirty;
  // Run saves the shown rule first when it runs and has changes.
  const savesFirst = unsaved && shownRuns;

  // Unsaved changes ask before the rule or the window goes.
  useEffect(() => {
    setDirty(dirty);
  }, [dirty, setDirty]);
  useEffect(
    () => () => {
      setDirty(false);
    },
    [setDirty],
  );
  const asking = question !== null || pending !== null;
  useEffect(() => {
    setAsking(asking);
  }, [asking, setAsking]);
  useEffect(
    () => () => {
      setAsking(false);
    },
    [setAsking],
  );

  // After Test, the preview's summary has the focus.
  useEffect(() => {
    if (!focusSummary.current) return;
    focusSummary.current = false;
    summary.current?.focus();
  }, [tested]);

  const problems = [
    ...(problem === null ? [] : [t(problem.id, problem.args)]),
    ...textErrors.map((e) => issueText(t, e)),
    ...(textErrors.length > 0 ? [] : [...new Set(issues.map((i) => i.id))].map((id) => t(id))),
  ];
  const severity = problem !== null || draft === null ? "error" : "warning";

  /** The rule as edited (reading edited text); `null` when the text does not read. */
  const current = async (): Promise<Rule | null> => {
    if (!textEdited) return rule;
    const result = read?.text === text ? read : await readText(text);
    return result.rule && own(result.rule);
  };

  /** Saves the rule as edited; the saved rule, or `null` when it is not ready. */
  const persist = async (): Promise<Rule | null> => {
    const next = await current();
    if (!next || ruleIssues(next).length > 0) return null;
    const saved = editable(await saveRule(client, next));
    if (view === "text") {
      // The text now shows the rule as saved, so it counts as unchanged.
      const savedText = await ruleToText(client, saved);
      setTextState(savedText);
      setWritten(savedText);
    }
    latest.current = saved;
    setRuleState(saved);
    setSavedRule(saved);
    setFooter({ kind: "saved" });
    await queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
    onSaved(saved);
    return saved;
  };

  const save = () =>
    run(async () => {
      await persist();
    });

  const showText = () =>
    run(async () => {
      // The latest rule's text, even when it changed while this one was written.
      let source = latest.current;
      let next = await ruleToText(client, source);
      while (latest.current !== source) {
        source = latest.current;
        next = await ruleToText(client, source);
      }
      setTextState(next);
      setWritten(next);
      setRead(null);
      setStuck(false);
      setView("text");
    });

  const showVisual = () =>
    run(async () => {
      if (text !== written) {
        const result = read?.text === text ? read : await readText(text);
        if (!result.rule) {
          setStuck(true);
          return;
        }
        const next = own(result.rule);
        latest.current = next;
        setRuleState(next);
      }
      setView("visual");
    });

  /** Switches the view (ignored while a switch or a save is under way). */
  const switchView = (next: View) => {
    if (busy || next === view) return;
    void (next === "text" ? showText() : showVisual());
  };

  /** Drops the text changes and goes back to the rule as it was. */
  const discardText = () => {
    setTextState(written);
    setRead(null);
    setStuck(false);
    setView("visual");
  };

  /** The core's target of the "Run on" choice; `null` when it has no messages. */
  const runTarget = async (): Promise<RuleRunTarget | null> => {
    const chosen = runOn.target();
    return chosen === null ? null : ruleRunTargetOf(client, chosen);
  };

  /**
   * The rules Run and Test act on, in file order, the shown one as edited;
   * `null` when the shown rule is one of them and its text does not read.
   */
  const actedOn = async (): Promise<Rule[] | null> => {
    const next = shownRuns ? await current() : null;
    if (shownRuns && !next) return null;
    if (!many) return next ? [next] : null;
    return selection.map((r) => (isShown(r) && next ? next : r));
  };

  /**
   * What each of `rules` matches on `target` (at most `limit` matches each):
   * the very messages a run there acts on. Each rule on its own, not one
   * after another.
   */
  const testEach = (
    rules: readonly Rule[],
    target: RuleRunTarget | null,
    limit: number,
  ): Promise<TestGroup[]> =>
    Promise.all(
      rules.map(async (r) => ({
        rule: r,
        ...(await testRule(client, r, target ?? RULE_SCOPE_TARGET, limit)),
      })),
    );

  const test = () =>
    run(async () => {
      const rules = await actedOn();
      if (!rules) return;
      const groups = await testEach(rules, await runTarget(), TEST_LIMIT);
      focusSummary.current = true;
      setTested({ groups, target: runOn.label });
      setTab("preview");
    });

  const undoOutcome = (outcome: RunOutcome) =>
    run(async () => {
      await undoRuns(client, outcome.undoable);
      setFooter({ kind: "undone", text: t("rules-undone-all") });
      await refreshRuns(queryClient);
    });

  /**
   * Runs `rules` on `target` (one `RunRules`, file order); the shown rule
   * is saved first when it is one of them and has unsaved changes.
   */
  const runNow = (rules: readonly Rule[], target: RuleRunTarget) =>
    run(async () => {
      const ids: RuleId[] = [];
      for (const r of rules) {
        if (!isShown(r)) {
          ids.push(r.id);
          continue;
        }
        const id = unsaved ? (await persist())?.id : savedRule.id;
        if (id === undefined) return;
        ids.push(id);
      }
      const outcome = outcomeOf(await runRules(client, ids, target));
      const message = outcomeMessage(outcome);
      setFooter({
        kind: "ran",
        text: t(message.id, message.args),
        undo:
          outcome.undoable.length > 0
            ? () => {
                void undoOutcome(outcome);
              }
            : null,
      });
      await refreshRuns(queryClient);
    });

  /**
   * Runs `rules` on `target`; when one of them moves or deletes mail, asks
   * first with how many matches they have there together (`count`, else
   * tested).
   */
  const requestRun = async (
    rules: readonly Rule[],
    target: RuleRunTarget | null,
    label: string,
    count?: number,
  ) => {
    if (target === null || rules.some((r) => ruleIssues(r).length > 0)) return;
    if (!rules.some(isDestructive)) {
      await runNow(rules, target);
      return;
    }
    const matched =
      count ??
      (await testEach(rules, target, COUNT_LIMIT)).reduce((n, g) => n + g.matches.length, 0);
    setQuestion({
      kind: "run",
      rules,
      count: matched,
      target: label,
      go: () => void runNow(rules, target),
    });
  };

  const runOnTarget = () =>
    run(async () => {
      const rules = await actedOn();
      if (rules) await requestRun(rules, await runTarget(), runOn.label);
    });

  const apply = (rule: Rule, matches: readonly ExplainedMatch[]) =>
    run(() =>
      requestRun(
        [rule],
        { messageIds: matches.map((m) => m.message.id), folderId: null },
        t("rules-target-matches", { count: matches.length }),
        matches.length,
      ),
    );

  const undoRun = (target: RuleRun) =>
    run(async () => {
      await undoRuleRun(client, target.id);
      setFooter({ kind: "undone", text: t("rules-undone") });
      await refreshRuns(queryClient);
    });

  const remove = () =>
    run(async () => {
      await deleteRule(client, savedRule.id);
      await queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
      await refreshRuns(queryClient);
      forget();
      onDeleted();
    });

  const open = (message: MessageSummary) => {
    const state = ui.getState();
    state.selectAccount(message.account_id);
    state.selectFolder(message.folder_id);
    state.selectMessage(message.id);
  };

  const shownName = draft?.name ?? rule.name;

  return (
    <form
      data-part="rules.editor"
      data-new={isNew}
      data-view={view}
      aria-label={t("common-rule")}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div data-part="rules.editor.content" inert={asking}>
        <RuleHeader
          rule={rule}
          onChange={setRule}
          nameInvalid={issues.some((i) => i.at.kind === "name")}
          describedBy={detailsId}
        >
          <Segmented
            name="rule-view"
            label={t("common-view")}
            value={view}
            options={[
              { value: "visual", label: t("rules-view-visual") },
              { value: "text", label: t("rules-view-text") },
            ]}
            onChange={(next) => {
              switchView(next as View);
            }}
          />
        </RuleHeader>
        <div data-part="rules.editor.body">
          {view === "text" ? (
            <>
              {stuck && (
                <p data-part="rules.text.stuck" role="alert">
                  {t("rules-text-broken")}
                  <button
                    type="button"
                    data-part="form.button"
                    data-action="discard-text"
                    onClick={discardText}
                  >
                    {t("rules-text-discard")}
                  </button>
                </p>
              )}
              <TextView
                text={text}
                onText={setText}
                errorLines={textErrors.map((e) => e.line)}
                describedBy={detailsId}
              />
            </>
          ) : (
            <RuleForm rule={rule} onChange={setRule} issues={issues} describedBy={detailsId} />
          )}
        </div>
        <LowerPanel
          tab={tab}
          onTab={setTab}
          ruleId={isNew ? null : savedRule.id}
          result={tested}
          summaryRef={summary}
          busy={busy}
          onOpen={open}
          onApply={(r, matches) => void apply(r, matches)}
          onUndo={(target) => void undoRun(target)}
        />
        <Footer
          problems={problems}
          severity={severity}
          detailsId={detailsId}
          state={footer}
          busy={busy}
          onDelete={
            isNew
              ? undefined
              : () => {
                  setQuestion({ kind: "delete" });
                }
          }
          runOn={runOn}
          runText={runLabel(t, { count: many ? selection.length : 1, savesFirst })}
          runBlocked={shownRuns && problems.length > 0}
          onRun={() => void runOnTarget()}
          onTest={() => void test()}
        />
      </div>
      {question?.kind === "delete" && (
        <Confirm
          kind="delete"
          question={t("rules-confirm-delete", { name: savedRule.name })}
          confirmLabel={t("rules-delete-confirm")}
          cancelLabel={t("common-cancel")}
          danger
          onCancel={() => {
            setQuestion(null);
          }}
          onConfirm={() => {
            setQuestion(null);
            void remove();
          }}
        />
      )}
      {question?.kind === "run" && (
        <Confirm
          kind="run"
          question={
            question.rules.length === 1
              ? t("rules-confirm-run", {
                  name: question.rules[0]?.name ?? shownName,
                  count: question.count,
                  target: question.target,
                })
              : t("rules-confirm-run-many", {
                  rules: question.rules.length,
                  count: question.count,
                  target: question.target,
                })
          }
          confirmLabel={runLabel(t, {
            count: question.rules.length,
            savesFirst: unsaved && question.rules.some(isShown),
          })}
          cancelLabel={t("common-cancel")}
          danger
          onCancel={() => {
            setQuestion(null);
          }}
          onConfirm={() => {
            setQuestion(null);
            question.go();
          }}
        />
      )}
      {pending !== null && question === null && (
        <Confirm
          kind="discard"
          question={t("rules-confirm-discard", { name: shownName })}
          confirmLabel={t("rules-discard")}
          cancelLabel={t("common-keep-editing")}
          danger
          onCancel={keepEditing}
          onConfirm={discard}
        />
      )}
    </form>
  );
}
