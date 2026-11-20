/**
 * The `rule-manager` widget: the ordered rule list (enable, reorder,
 * select, new, check for running) next to the editor of the selected rule,
 * whose run panel (R1) runs it or the checked rules; with no rule open the
 * run panel stands alone, for the checked rules. Also shown as the `rules`
 * dialog when the layout has no `window "rules"`.
 *
 * The list is a grid with a header row naming its columns ("Run", "Rule",
 * "On"), so the two check boxes of a row are told apart at a glance and
 * keep their room (review R1).
 */
import { useQueryClient } from "@tanstack/react-query";
import { ExchangeRuleBadges } from "../exchange";
import { useEffect, useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { Rule, RuleTrigger } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { reorderRules, rulesKeys, saveRule, useRules } from "./api";
import { moveItem, newRule } from "./model";
import { Issues } from "./Issues";
import { RuleEditor } from "./RuleEditor";
import { RunPanel } from "./RunPanel";
import { useRulesUi } from "./store";

function triggerSummary(triggers: readonly RuleTrigger[]): string[] {
  return triggers.length === 0
    ? ["rules-when-manual"]
    : triggers.map((t) => `rules-when-${t.type}`);
}

function RuleList({
  rules,
  selected,
  error,
  onError,
}: {
  rules: readonly Rule[];
  selected: string | null;
  error: string | null;
  onError: (id: string) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const select = useRulesUi((s) => s.select);
  const startNew = useRulesUi((s) => s.startNew);
  const picked = useRulesUi((s) => s.picked);
  const pick = useRulesUi((s) => s.pick);
  const refresh = () => queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
  const act = async (work: () => Promise<unknown>) => {
    try {
      await work();
      await refresh();
    } catch (error) {
      onError(errorMessageIdOf(error));
    }
  };
  const move = (index: number, delta: -1 | 1) =>
    act(() =>
      reorderRules(
        client,
        moveItem(rules, index, delta).map((r) => r.id),
      ),
    );
  return (
    <div data-part="rules.list">
      <div data-part="rules.buttons">
        <button
          type="button"
          data-part="form.button"
          data-action="new-rule"
          onClick={() => {
            startNew(newRule());
          }}
        >
          {t("rules-new")}
        </button>
        <Issues kind="error" lines={error === null ? [] : [t(error)]} />
      </div>
      {rules.length === 0 ? (
        <p data-part="rules.hint">{t("rules-empty")}</p>
      ) : (
        <ol data-part="rules.list.items" aria-label={t("rules-list-label")}>
          {/* Column names for the eye; each control has its own accessible name. */}
          <li data-part="rules.list.head" aria-hidden="true">
            <span data-part="rules.list.head.cell">{t("rules-list-head-run")}</span>
            <span data-part="rules.list.head.cell">{t("common-rule")}</span>
            <span data-part="rules.list.head.cell">{t("common-on")}</span>
          </li>
          {rules.map((rule, index) => (
            <li
              key={rule.id}
              data-part="rules.list.item"
              data-selected={rule.id === selected}
              data-enabled={rule.enabled}
            >
              <input
                data-part="form.checkbox"
                data-field="rule-pick"
                type="checkbox"
                checked={picked.includes(rule.id)}
                aria-label={t("rules-pick-rule", { name: rule.name })}
                title={t("rules-pick-rule", { name: rule.name })}
                onChange={(event) => {
                  pick(rule.id, event.target.checked);
                }}
              />
              <span data-part="rules.list.main">
                <button
                  type="button"
                  data-part="rules.list.select"
                  aria-pressed={rule.id === selected}
                  onClick={() => {
                    select(rule.id);
                  }}
                >
                  {rule.name}
                </button>
                <span data-part="rules.list.triggers">
                  {triggerSummary(rule.triggers)
                    .map((id) => t(id))
                    .join(t("rules-list-separator"))}
                </span>
                <ExchangeRuleBadges ruleId={rule.id} />
              </span>
              <label data-part="form.check">
                <input
                  data-part="form.checkbox"
                  type="checkbox"
                  checked={rule.enabled}
                  aria-label={t("rules-enable-rule", { name: rule.name })}
                  onChange={(event) =>
                    void act(() => saveRule(client, { ...rule, enabled: event.target.checked }))
                  }
                />
              </label>
              <button
                type="button"
                data-part="form.button"
                data-action="rule-up"
                aria-label={t("rules-move-rule-up", { name: rule.name })}
                disabled={index === 0}
                onClick={() => void move(index, -1)}
              >
                {t("common-move-up")}
              </button>
              <button
                type="button"
                data-part="form.button"
                data-action="rule-down"
                aria-label={t("rules-move-rule-down", { name: rule.name })}
                disabled={index === rules.length - 1}
                onClick={() => void move(index, 1)}
              >
                {t("common-move-down")}
              </button>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

/** The rule list and the editor. */
export function RuleManagerView() {
  const t = useT();
  const rules = useRules();
  const editing = useRulesUi((s) => s.editing);
  const select = useRulesUi((s) => s.select);
  const saved = useRulesUi((s) => s.saved);
  const prunePicked = useRulesUi((s) => s.prunePicked);
  const [error, setError] = useState<string | null>(null);
  const list = rules.data ?? [];
  // A deleted rule is no longer checked for running.
  useEffect(() => {
    if (rules.data) prunePicked(rules.data.map((r) => r.id));
  }, [rules.data, prunePicked]);
  const selectedRule =
    editing?.kind === "saved" ? list.find((r) => r.id === editing.id) : undefined;
  const initial = editing?.kind === "new" ? editing.rule : selectedRule;
  const current = editing?.kind === "saved" ? editing.id : null;
  return (
    <section data-part="rules" aria-label={t("rules-title")}>
      <RuleList rules={list} selected={current} error={error} onError={setError} />
      {initial ? (
        <RuleEditor
          key={editing?.key}
          initial={initial}
          onSaved={(rule) => {
            saved(rule.id);
          }}
          onDeleted={() => {
            select(null);
          }}
        />
      ) : (
        <div data-part="rules.editor" data-state="none">
          <p data-part="rules.hint">{t("rules-select-hint")}</p>
          <RunPanel edited={null} />
        </div>
      )}
    </section>
  );
}

/** The `rule-manager` layout widget. */
export function RuleManager() {
  return <RuleManagerView />;
}

/** The `rules` dialog (window frame and title come from the window layer). */
export function RulesDialog() {
  return <RuleManagerView />;
}
