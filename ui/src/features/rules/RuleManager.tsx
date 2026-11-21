/**
 * The `rule-manager` widget: the ordered rule list next to the editor of
 * the selected rule (`./RuleEditor`). Also shown as the `rules` dialog when
 * the layout has no `window "rules"`, and in Settings → Mail & junk — with
 * the same parts, so both look alike (R2 design review, item 15).
 *
 * The list (R2 design review, items 3, 12 and 13): New rule, a filter, and
 * one row per rule — a drag handle, the name over its triggers, the
 * Exchange badge (a compact ⓘ with the reasons as hover / focus text) and
 * the On check box. Reordering: drag a row by its handle, or Alt+Up /
 * Alt+Down on a row (also the arrow keys on the handle).
 *
 * With no rule open the editor's place shows the empty state (item 11):
 * "Create rule from message" and starter rules.
 *
 * Parts: `rules` (the container the responsive rules measure: below about
 * 760 px the list goes over the editor), `rules.panes`, `rules.list`,
 * `rules.list.tools`, `rules.list.filter`, `rules.list.items`,
 * `rules.list.item` (`data-selected`, `data-enabled`, `data-drop`),
 * `rules.list.handle`, `rules.list.main`, `rules.list.select`,
 * `rules.list.triggers`, `rules.empty`, `rules.empty.starters`,
 * `rules.empty.starter`.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type DragEvent, type KeyboardEvent } from "react";
import { useActionEnabled, useActions } from "../../actions/ActionsProvider";
import { errorMessageIdOf } from "../../api/client";
import type { Rule, RuleTrigger } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { Icon } from "../../widgets/Icon";
import { ExchangeRuleBadges } from "../exchange";
import { reorderRules, rulesKeys, saveRule, useRules } from "./api";
import { Issues } from "./Issues";
import { filterRules, moveItemTo, newRule } from "./model";
import { RuleEditor } from "./RuleEditor";
import { STARTER_IDS, starterRule } from "./starters";
import { useRulesUi } from "./store";

/** The action that starts a rule from the selected message. */
const CREATE_FROM_MESSAGE = "rules.create-from-message";

function triggerSummary(triggers: readonly RuleTrigger[]): string[] {
  return triggers.length === 0
    ? ["rules-when-manual"]
    : triggers.map((t) => `rules-when-${t.type}`);
}

/** The move Alt+Up / Alt+Down (or an arrow key on the handle) asks for, else `0`. */
export function reorderStep(
  event: Pick<KeyboardEvent, "key" | "altKey">,
  onHandle: boolean,
): -1 | 0 | 1 {
  if (!event.altKey && !onHandle) return 0;
  if (event.key === "ArrowUp") return -1;
  if (event.key === "ArrowDown") return 1;
  return 0;
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
  const [filter, setFilter] = useState("");
  const [dragged, setDragged] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const refresh = () => queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
  const act = async (work: () => Promise<unknown>) => {
    try {
      await work();
      await refresh();
    } catch (error) {
      onError(errorMessageIdOf(error));
    }
  };
  /** Moves the rule at `from` (of the whole list) to `to`, keeping its focus. */
  const move = (from: number, to: number) => {
    if (to < 0 || to >= rules.length || from === to) return;
    const id = rules[from]?.id;
    void act(async () => {
      await reorderRules(
        client,
        moveItemTo(rules, from, to).map((r) => r.id),
      );
      await refresh();
      // The moved row's handle keeps the focus.
      document
        .querySelector<HTMLElement>(`[data-part="rules.list.handle"][data-rule="${String(id)}"]`)
        ?.focus();
    });
  };
  const shown = filterRules(rules, filter);
  const onKeyDown = (event: KeyboardEvent<HTMLElement>, index: number) => {
    const onHandle = (event.target as HTMLElement).dataset.part === "rules.list.handle";
    const step = reorderStep(event, onHandle);
    if (step === 0) return;
    event.preventDefault();
    move(index, index + step);
  };
  const dropOn = (event: DragEvent<HTMLElement>, index: number) => {
    event.preventDefault();
    if (dragged !== null) move(dragged, index);
    setDragged(null);
    setOver(null);
  };
  return (
    <div data-part="rules.list">
      <div data-part="rules.list.tools">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="new-rule"
          onClick={() => {
            startNew(newRule());
          }}
        >
          {t("rules-new")}
        </button>
        <input
          data-part="rules.list.filter"
          type="search"
          name="rule-filter"
          value={filter}
          placeholder={t("rules-filter")}
          aria-label={t("rules-filter")}
          onChange={(event) => {
            setFilter(event.target.value);
          }}
        />
        <Issues kind="error" lines={error === null ? [] : [t(error)]} />
      </div>
      {rules.length > 0 && shown.length === 0 && (
        <p data-part="rules.hint">{t("rules-filter-none")}</p>
      )}
      {shown.length > 0 && (
        <ol data-part="rules.list.items" aria-label={t("rules-list-label")}>
          {shown.map((rule) => {
            const index = rules.indexOf(rule);
            const filtered = shown.length !== rules.length;
            return (
              // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Alt+Up / Alt+Down bubble up from the row's controls, and the row is where a dragged handle drops.
              <li
                key={rule.id}
                data-part="rules.list.item"
                data-selected={rule.id === selected}
                data-enabled={rule.enabled}
                data-drop={over === index && dragged !== index ? "target" : undefined}
                onKeyDown={(event) => {
                  if (!filtered) onKeyDown(event, index);
                }}
                onDragOver={(event) => {
                  if (dragged === null) return;
                  event.preventDefault();
                  setOver(index);
                }}
                onDrop={(event) => {
                  dropOn(event, index);
                }}
              >
                <button
                  type="button"
                  data-part="rules.list.handle"
                  data-rule={rule.id}
                  draggable={!filtered}
                  disabled={filtered}
                  aria-label={t("rules-move-rule", { name: rule.name })}
                  title={t("rules-move-rule", { name: rule.name })}
                  onDragStart={(event) => {
                    event.dataTransfer.effectAllowed = "move";
                    event.dataTransfer.setData("text/plain", rule.id);
                    setDragged(index);
                  }}
                  onDragEnd={() => {
                    setDragged(null);
                    setOver(null);
                  }}
                >
                  <Icon name="grip-vertical" part="rules.list.handle.icon" />
                </button>
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
                </span>
                <ExchangeRuleBadges ruleId={rule.id} />
                <input
                  data-part="form.checkbox"
                  data-field="rule-enabled"
                  type="checkbox"
                  checked={rule.enabled}
                  aria-label={t("rules-enable-rule", { name: rule.name })}
                  title={t("rules-enable-rule", { name: rule.name })}
                  onChange={(event) =>
                    void act(() => saveRule(client, { ...rule, enabled: event.target.checked }))
                  }
                />
              </li>
            );
          })}
        </ol>
      )}
      {rules.length > 1 && <p data-part="rules.hint">{t("rules-reorder-hint")}</p>}
    </div>
  );
}

/** No rule open: start one from the selected message, a starter, or a blank rule. */
function EmptyState({ hasRules }: { hasRules: boolean }) {
  const t = useT();
  const actions = useActions();
  const fromMessage = useActionEnabled(CREATE_FROM_MESSAGE);
  const startNew = useRulesUi((s) => s.startNew);
  return (
    <div data-part="rules.empty">
      <p data-part="rules.hint">{t(hasRules ? "rules-select-hint" : "rules-empty")}</p>
      <button
        type="button"
        data-part="form.button"
        data-action="create-from-message"
        disabled={!fromMessage}
        title={fromMessage ? undefined : t("rules-no-message")}
        onClick={() => void actions.dispatch(CREATE_FROM_MESSAGE, [])}
      >
        {t("rules-action-create-from-message")}
      </button>
      <section data-part="rules.empty.starters" aria-label={t("rules-starters")}>
        <h3 data-part="rules.empty.title">{t("rules-starters")}</h3>
        <ul data-part="rules.empty.list">
          {STARTER_IDS.map((id) => (
            <li key={id} data-part="rules.empty.starter">
              <button
                type="button"
                data-part="form.button"
                data-action="starter"
                data-starter={id}
                onClick={() => {
                  startNew(starterRule(id, t));
                }}
              >
                {t(`rules-starter-${id}-name`)}
              </button>
              <span data-part="rules.hint">{t(`rules-starter-${id}-hint`)}</span>
            </li>
          ))}
        </ul>
      </section>
    </div>
  );
}

/** The rule list and the editor. */
export function RuleManagerView() {
  const t = useT();
  const rules = useRules();
  const editing = useRulesUi((s) => s.editing);
  const saved = useRulesUi((s) => s.saved);
  const asking = useRulesUi((s) => s.asking);
  const [error, setError] = useState<string | null>(null);
  const list = rules.data ?? [];
  const selectedRule =
    editing?.kind === "saved" ? list.find((r) => r.id === editing.id) : undefined;
  const initial = editing?.kind === "new" ? editing.rule : selectedRule;
  const current = editing?.kind === "saved" ? editing.id : null;
  return (
    <section data-part="rules" aria-label={t("rules-title")}>
      <div data-part="rules.panes">
        <div data-part="rules.side" inert={asking}>
          <RuleList rules={list} selected={current} error={error} onError={setError} />
        </div>
        {initial ? (
          <RuleEditor
            key={editing?.key}
            initial={initial}
            onSaved={(rule) => {
              saved(rule.id);
            }}
            onDeleted={() => {
              setError(null);
            }}
          />
        ) : (
          <EmptyState hasRules={list.length > 0} />
        )}
      </div>
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
