/**
 * The `rule-manager` widget: the ordered rule list (enable, reorder,
 * select, new) next to the editor of the selected rule. Also shown as the
 * `rules` dialog when the layout has no `window "rules"`.
 */
import { useQueryClient } from "@tanstack/react-query";
import { ExchangeRuleBadges } from "../exchange";
import { useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { Rule, RuleTrigger } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { reorderRules, rulesKeys, saveRule, useRules } from "./api";
import { moveItem, newRule } from "./model";
import { RuleEditor } from "./RuleEditor";
import { useRulesUi } from "./store";

function triggerSummary(triggers: readonly RuleTrigger[]): string[] {
  return triggers.length === 0
    ? ["rules-when-manual"]
    : triggers.map((t) => `rules-when-${t.type}`);
}

function RuleList({
  rules,
  selected,
  onError,
}: {
  rules: readonly Rule[];
  selected: string | null;
  onError: (id: string) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const select = useRulesUi((s) => s.select);
  const startNew = useRulesUi((s) => s.startNew);
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
      {rules.length === 0 ? (
        <p data-part="rules.hint">{t("rules-empty")}</p>
      ) : (
        <ol data-part="rules.list.items" aria-label={t("rules-list-label")}>
          {rules.map((rule, index) => (
            <li
              key={rule.id}
              data-part="rules.list.item"
              data-selected={rule.id === selected}
              data-enabled={rule.enabled}
            >
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
  const [error, setError] = useState<string | null>(null);
  const list = rules.data ?? [];
  const selectedRule =
    editing?.kind === "saved" ? list.find((r) => r.id === editing.id) : undefined;
  const initial = editing?.kind === "new" ? editing.rule : selectedRule;
  return (
    <section data-part="rules" aria-label={t("rules-title")}>
      <RuleList
        rules={list}
        selected={editing?.kind === "saved" ? editing.id : null}
        onError={setError}
      />
      {error && (
        <p data-part="rules.notice" data-kind="error" role="alert">
          {t(error)}
        </p>
      )}
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
        <p data-part="rules.hint">{t("rules-select-hint")}</p>
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
