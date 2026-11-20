/**
 * Editing one rule: name, triggers, scope, the condition tree and the
 * actions (visual editor), or the same rule as `rules.kdl` text; testing
 * it against existing mail; running it (and the rules checked in the list)
 * now with the run panel (`./RunPanel.tsx`) under the buttons, which saves
 * unsaved changes first; its recent runs and their undo.
 *
 * Switching to the text view and back without changing the text keeps the
 * rule as it was: only edited text is parsed (R1: a new rule's text did not
 * read back). Edited text is read in the background after a typing pause,
 * so the run panel names and judges the rule Save would write, in both
 * views; saving from the text view writes the saved rule back as the text.
 * Problems and errors are the compact `Issues` indicator in the view
 * switch's row, so they never push the editor around.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type {
  Condition,
  ConfigIssue,
  Rule,
  RuleAction,
  RuleRun,
  RuleTrigger,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { TYPING_PAUSE_MS, useDebounced } from "../../state/debounce";
import { useAccounts, useClient } from "../../state/queries";
import { CheckField, NumberField, Section, SelectField, TextField } from "../../widgets/forms";
import { ActionEditor } from "./ActionEditor";
import {
  deleteRule,
  ruleFromText,
  ruleToText,
  rulesKeys,
  saveRule,
  testRule,
  undoRuleRun,
  useRuleRuns,
} from "./api";
import { ConditionEditor } from "./ConditionEditor";
import { Issues } from "./Issues";
import { RunPanel } from "./RunPanel";
import {
  TRIGGER_KINDS,
  comparePaths,
  conditionAt,
  ruleProblems,
  toggleTrigger,
  type TriggerKind,
} from "./model";
import type { ConditionHit, ExplainedMatch } from "./types";

/** Maximum number of matches a test shows. */
export const TEST_LIMIT = 50;

type Notice = { kind: "ok" | "error"; id: string } | null;

/** The root condition as a group, so the editor can add to it. */
function asGroup(c: Condition): Condition {
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

function TriggerFields({ rule, onChange }: { rule: Rule; onChange: (rule: Rule) => void }) {
  const has = (kind: TriggerKind) => rule.triggers.find((t) => t.type === kind);
  const setTrigger = (next: RuleTrigger) => {
    onChange({
      ...rule,
      triggers: rule.triggers.map((t) => (t.type === next.type ? next : t)),
    });
  };
  const schedule = has("schedule");
  const older = has("older-than");
  return (
    <Section titleId="rules-section-triggers">
      {TRIGGER_KINDS.map((kind) => (
        <CheckField
          key={kind}
          labelId={`rules-trigger-${kind}`}
          name={`trigger-${kind}`}
          checked={has(kind) !== undefined}
          onChange={(on) => {
            onChange(toggleTrigger(rule, kind, on));
          }}
        />
      ))}
      {schedule?.type === "schedule" && (
        <NumberField
          labelId="rules-schedule-minutes"
          name="schedule-minutes"
          min={1}
          value={schedule.interval_minutes}
          onChange={(n) => {
            setTrigger({ type: "schedule", interval_minutes: n ?? 0 });
          }}
        />
      )}
      {older?.type === "older-than" && (
        <NumberField
          labelId="rules-older-than-days"
          name="older-than-days"
          min={1}
          value={older.days}
          onChange={(n) => {
            setTrigger({ type: "older-than", days: n ?? 0 });
          }}
        />
      )}
    </Section>
  );
}

function ScopeField({ rule, onChange }: { rule: Rule; onChange: (rule: Rule) => void }) {
  const t = useT();
  const accounts = useAccounts();
  const [only] = rule.scope;
  const simple = rule.scope.length === 0 || (rule.scope.length === 1 && only?.type === "account");
  return (
    <Section titleId="rules-section-scope">
      {simple ? (
        <SelectField
          labelId="rules-scope"
          name="scope"
          value={only?.type === "account" ? only.account_id : ""}
          choices={[
            { value: "", label: "rules-scope-everywhere" },
            ...(accounts.data ?? []).map((a) => ({
              value: a.id,
              label: a.name || a.email,
              literal: true,
            })),
          ]}
          onChange={(id) => {
            onChange({ ...rule, scope: id === "" ? [] : [{ type: "account", account_id: id }] });
          }}
        />
      ) : (
        <p data-part="rules.hint">{t("rules-scope-custom")}</p>
      )}
    </Section>
  );
}

/** The editor's text view: the rule in `rules.kdl` syntax. */
function TextView({
  text,
  onText,
  onApply,
}: {
  text: string;
  onText: (text: string) => void;
  onApply: () => void;
}) {
  const t = useT();
  return (
    <div data-part="rules.text">
      <label data-part="form.field">
        <span data-part="form.label">{t("rules-text-label")}</span>
        <textarea
          data-part="rules.text.source"
          name="rule-text"
          spellCheck={false}
          value={text}
          onChange={(event) => {
            onText(event.target.value);
          }}
        />
      </label>
      <button type="button" data-part="form.button" data-action="apply-text" onClick={onApply}>
        {t("rules-text-apply")}
      </button>
    </div>
  );
}

/** A text-view error with its position. */
function issueText(t: ReturnType<typeof useT>, e: ConfigIssue): string {
  return t("common-error-at-line", {
    line: e.line,
    column: e.column,
    message: e.localized ? t(e.localized.id, e.localized.args) : e.message,
  });
}

/** How a condition is named in explanations: its kind, plus a group's mode. */
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
  );
}

function TestResults({
  rule,
  result,
}: {
  rule: Rule;
  result: { scanned: number; matches: ExplainedMatch[] };
}) {
  const t = useT();
  return (
    <section data-part="rules.test" aria-label={t("rules-test-title")}>
      <p data-part="rules.test.summary" role="status">
        {t("rules-test-summary", { matched: result.matches.length, scanned: result.scanned })}
      </p>
      {result.matches.length === 0 ? (
        <p data-part="rules.hint">{t("rules-test-none")}</p>
      ) : (
        <ul data-part="rules.test.matches">
          {result.matches.map((m) => (
            <li key={m.message.id} data-part="rules.test.match">
              <span data-part="rules.test.subject">{m.message.subject}</span>
              <span data-part="rules.test.from">
                {m.message.from?.name ?? m.message.from?.email ?? ""}
              </span>
              <ul data-part="rules.test.actions" aria-label={t("rules-test-actions")}>
                {m.actions.map((a: RuleAction, i) => (
                  <li key={i} data-part="rules.test.action" data-kind={a.type}>
                    {t(`rules-do-${a.type}`)}
                  </li>
                ))}
              </ul>
              {m.explanation.length > 0 && <Explanation rule={rule} hits={m.explanation} />}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function RunHistory({ ruleId, onNotice }: { ruleId: string; onNotice: (n: Notice) => void }) {
  const t = useT();
  const client = useClient();
  const runs = useRuleRuns(ruleId);
  const queryClient = useQueryClient();
  const undo = async (run: RuleRun) => {
    try {
      await undoRuleRun(client, run.id);
      onNotice({ kind: "ok", id: "rules-undone" });
      await queryClient.invalidateQueries({ queryKey: rulesKeys.allRuns });
    } catch (error) {
      onNotice({ kind: "error", id: errorMessageIdOf(error) });
    }
  };
  const list = runs.data ?? [];
  return (
    <section data-part="rules.runs" aria-label={t("rules-runs-title")}>
      <h3 data-part="rules.runs.title">{t("rules-runs-title")}</h3>
      {list.length === 0 ? (
        <p data-part="rules.hint">{t("rules-runs-none")}</p>
      ) : (
        <ul data-part="rules.runs.items">
          {list.map((run) => (
            <li key={run.id} data-part="rules.run" data-state={run.state}>
              <span data-part="rules.run.summary">
                {t("rules-run-summary", {
                  cause: t(`rules-run-cause-${run.cause}`),
                  matched: run.matched,
                  state: t(`rules-run-state-${run.state}`),
                })}
              </span>
              {run.undoable && (
                <button
                  type="button"
                  data-part="form.button"
                  data-action="undo-run"
                  onClick={() => void undo(run)}
                >
                  {t("rules-undo")}
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/**
 * Edits `initial` (a saved rule, or a new one with an empty id). Calls
 * `onSaved` with the saved rule and `onDeleted` after a delete.
 */
export function RuleEditor({
  initial,
  onSaved,
  onDeleted,
}: {
  initial: Rule;
  onSaved: (rule: Rule) => void;
  onDeleted: () => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [rule, setRule] = useState<Rule>(() => editable(initial));
  // The rule as last saved (what "Run now" would run).
  const [savedRule, setSavedRule] = useState<Rule>(() => editable(initial));
  const [view, setView] = useState<"visual" | "text">("visual");
  const [text, setText] = useState("");
  // The text as the core wrote it: unchanged text needs no parsing.
  const [written, setWritten] = useState("");
  const [textErrors, setTextErrors] = useState<ConfigIssue[]>([]);
  // Edited text as last read in the background (`rule` null: it does not read).
  const [read, setRead] = useState<{ text: string; rule: Rule | null } | null>(null);
  const [notice, setNotice] = useState<Notice>(null);
  const [busy, setBusy] = useState(false);
  const [tested, setTested] = useState<{ scanned: number; matches: ExplainedMatch[] } | null>(null);
  const isNew = initial.id === "";
  const problems = ruleProblems(rule);
  const errors = [
    ...(notice?.kind === "error" ? [t(notice.id)] : []),
    ...(view === "text" ? textErrors.map((e) => issueText(t, e)) : []),
  ];

  const run = async (work: () => Promise<void>) => {
    setBusy(true);
    setNotice(null);
    try {
      await work();
    } catch (error) {
      setNotice({ kind: "error", id: errorMessageIdOf(error) });
    } finally {
      setBusy(false);
    }
  };

  /** A rule read from the text: it keeps this rule's identity whatever the text says. */
  const own = (parsed: Rule): Rule => editable({ ...parsed, id: rule.id, source: rule.source });

  /** Reads the text view into the rule; `null` when it has errors. */
  const fromText = async (): Promise<Rule | null> => {
    const parsed = await ruleFromText(client, text);
    setTextErrors(parsed.errors);
    if (!parsed.rule) return null;
    const next = own(parsed.rule);
    setRule(next);
    return next;
  };

  // Edited text is read once typing pauses (quietly: errors show on Save,
  // Apply or the switch back), so the run panel knows what Save would write.
  const textEdited = view === "text" && text !== written;
  const typed = useDebounced(text, TYPING_PAUSE_MS);
  useEffect(() => {
    if (view !== "text" || typed !== text || typed === written) return;
    let live = true;
    const settle = (read: Rule | null) => {
      if (live) setRead({ text: typed, rule: read });
    };
    ruleFromText(client, typed).then(
      (parsed) => {
        settle(parsed.rule);
      },
      () => {
        settle(null);
      },
    );
    return () => {
      live = false;
    };
  }, [client, view, typed, text, written]);

  const showText = () =>
    run(async () => {
      const next = await ruleToText(client, rule);
      setText(next);
      setWritten(next);
      setTextErrors([]);
      setView("text");
    });

  const showVisual = () =>
    run(async () => {
      if (text === written || (await fromText()) !== null) {
        setTextErrors([]);
        setView("visual");
      }
    });

  const current = async (): Promise<Rule | null> =>
    view === "text" && text !== written ? fromText() : rule;

  /** Saves the rule as edited; the saved rule, or `null` when it is not ready. */
  const persist = async (): Promise<Rule | null> => {
    const next = await current();
    if (!next || ruleProblems(next).length > 0) return null;
    const saved = editable(await saveRule(client, next));
    if (view === "text") {
      // The text now shows the rule as saved, so it counts as unchanged.
      const savedText = await ruleToText(client, saved);
      setText(savedText);
      setWritten(savedText);
    }
    setRule(saved);
    setSavedRule(saved);
    setNotice({ kind: "ok", id: "rules-saved" });
    await queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
    onSaved(saved);
    return saved;
  };

  const save = () =>
    run(async () => {
      await persist();
    });

  // The rule Save would write: `undefined` while edited text is not read
  // yet, `null` when it does not read.
  const draft: Rule | null | undefined = !textEdited
    ? rule
    : read?.text === text
      ? read.rule && own(read.rule)
      : undefined;
  const unsaved = isNew || !draft || ruleChanged(draft, savedRule);
  const blocked = draft === null || (draft !== undefined && ruleProblems(draft).length > 0);

  const test = () =>
    run(async () => {
      const next = await current();
      if (!next) return;
      setTested(await testRule(client, next, null, TEST_LIMIT));
    });

  const remove = () =>
    run(async () => {
      await deleteRule(client, rule.id);
      await queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
      onDeleted();
    });

  return (
    <form
      data-part="rules.editor"
      data-new={isNew}
      aria-label={t("common-rule")}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      <div data-part="rules.views" role="group" aria-label={t("common-view")}>
        <button
          type="button"
          data-part="rules.view"
          aria-pressed={view === "visual"}
          disabled={busy || view === "visual"}
          onClick={() => void showVisual()}
        >
          {t("rules-view-visual")}
        </button>
        <button
          type="button"
          data-part="rules.view"
          aria-pressed={view === "text"}
          disabled={busy || view === "text"}
          onClick={() => void showText()}
        >
          {t("rules-view-text")}
        </button>
        <Issues kind="error" lines={errors} />
        <Issues kind="warning" lines={view === "visual" ? problems.map((id) => t(id)) : []} />
        {notice?.kind === "ok" && (
          <span data-part="rules.notice" data-kind="ok" role="status">
            {t(notice.id)}
          </span>
        )}
      </div>
      {view === "text" ? (
        <TextView
          text={text}
          onText={setText}
          onApply={() =>
            void run(async () => {
              await fromText();
            })
          }
        />
      ) : (
        <>
          <TextField
            labelId="rules-name"
            name="rule-name"
            value={rule.name}
            required
            onChange={(name) => {
              setRule({ ...rule, name });
            }}
          />
          <CheckField
            labelId="rules-enabled"
            name="rule-enabled"
            checked={rule.enabled}
            onChange={(enabled) => {
              setRule({ ...rule, enabled });
            }}
          />
          <TriggerFields rule={rule} onChange={setRule} />
          <ScopeField rule={rule} onChange={setRule} />
          <Section titleId="rules-section-conditions">
            <ConditionEditor
              value={rule.conditions}
              onChange={(conditions) => {
                setRule({ ...rule, conditions });
              }}
            />
          </Section>
          <Section titleId="rules-section-actions">
            <ActionEditor
              value={rule.actions}
              onChange={(actions) => {
                setRule({ ...rule, actions });
              }}
            />
          </Section>
        </>
      )}
      <div data-part="rules.buttons">
        <button
          type="submit"
          data-part="form.button"
          data-action="save-rule"
          disabled={busy || (view === "visual" && problems.length > 0)}
        >
          {t("rules-save")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="test-rule"
          disabled={busy}
          onClick={() => void test()}
        >
          {t("rules-test")}
        </button>
        {!isNew && (
          <button
            type="button"
            data-part="form.button"
            data-action="delete-rule"
            disabled={busy}
            onClick={() => void remove()}
          >
            {t("rules-delete")}
          </button>
        )}
      </div>
      <RunPanel
        edited={{
          id: isNew ? null : rule.id,
          name: draft?.name ?? rule.name,
          unsaved,
          blocked,
          save: async () => (await persist())?.id ?? null,
        }}
      />
      {tested && <TestResults rule={rule} result={tested} />}
      {!isNew && <RunHistory ruleId={rule.id} onNotice={setNotice} />}
    </form>
  );
}
