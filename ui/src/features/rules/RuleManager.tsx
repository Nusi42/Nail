/**
 * The `rule-manager` widget: the ordered rule list next to the editor of
 * the focused rule (`./RuleEditor`). Also shown as the `rules` dialog when
 * the layout has no `window "rules"`, and in Settings → Mail & junk — with
 * the same parts, so both look alike (R2 design review, item 15).
 *
 * The list (R2 design review, items 3, 12 and 13; R3, ADR 0227): New rule,
 * a filter, and a multi-select listbox with one option per rule — a drag
 * handle, the name over its triggers, the Exchange badge (one ⓘ with the
 * summed reasons; its popover lists each account) and the On mark.
 *
 * - Selecting (`widgets/listSelection.ts`): Click selects one rule, Ctrl+click adds
 *   or removes one, Shift+click and Shift+Up / Down extend from the anchor,
 *   Space adds or removes the focused rule, Ctrl+A selects every listed
 *   rule, Escape goes back to the focused rule alone. The editor shows the
 *   focused rule; Run and Test act on every selected rule.
 * - Reordering: drag a row by its handle, or Alt+Up / Alt+Down on a row.
 * - An option's content is not interactive (ARIA: its children are
 *   presentational), so the handle and the On mark are pointer targets;
 *   the keyboard reorders with Alt+arrows and switches a rule on or off in
 *   the editor's header. The row's description says when a rule is off
 *   and where it runs on Exchange accounts.
 *
 * With no rule open the editor's place shows the empty state (item 11):
 * "Create rule from message" and starter rules.
 *
 * Parts: `rules` (the container the responsive rules measure: below about
 * 760 px the list goes over the editor), `rules.panes`, `rules.list`,
 * `rules.list.tools`, `rules.list.filter`, `rules.list.items`,
 * `rules.list.item` (`data-selected`, `data-current`, `data-enabled`,
 * `data-drop`), `rules.list.handle`, `rules.list.main`, `rules.list.name`,
 * `rules.list.triggers`, `rules.list.about`, `rules.list.off`,
 * `rules.list.enabled` (`data-enabled`), `rules.empty`,
 * `rules.empty.starters`, `rules.empty.starter`.
 */
import { useQueryClient } from "@tanstack/react-query";
import {
  useEffect,
  useId,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import { useActionEnabled, useActions } from "../../actions/ActionsProvider";
import { errorMessageIdOf } from "../../api/client";
import type { Rule, RuleId, RuleTrigger } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { Icon } from "../../widgets/Icon";
import { ExchangeRuleBadges } from "../exchange";
import { reorderRules, rulesKeys, saveRule, useRules } from "./api";
import { Issues } from "./Issues";
import { filterRules, moveItemTo, newRule } from "./model";
import { RuleEditor } from "./RuleEditor";
import {
  gestureOf,
  inListOrder,
  pick,
  pickAll,
  stepFrom,
  type ListSelection,
} from "../../widgets/listSelection";
import { STARTER_IDS, starterRule } from "./starters";
import { useRulesHandoff } from "./popOut";
import { useRulesUi, type RuleSelection } from "./store";

/** The action that starts a rule from the selected message. */
const CREATE_FROM_MESSAGE = "rules.create-from-message";

function triggerSummary(triggers: readonly RuleTrigger[]): string[] {
  return triggers.length === 0
    ? ["rules-when-manual"]
    : triggers.map((t) => `rules-when-${t.type}`);
}

/** The move Alt+Up / Alt+Down asks for, else `0`. */
export function reorderStep(event: Pick<KeyboardEvent, "key" | "altKey">): -1 | 0 | 1 {
  if (!event.altKey) return 0;
  if (event.key === "ArrowUp") return -1;
  if (event.key === "ArrowDown") return 1;
  return 0;
}

/** The list keys that move the focus, and how far (an end for Home / End). */
const FOCUS_STEPS: Readonly<Record<string, "first" | "last" | -1 | 1>> = {
  ArrowUp: -1,
  ArrowDown: 1,
  Home: "first",
  End: "last",
};

/**
 * The selection a key asks for in the list (`order`: the listed ids), or
 * `null` when the key does not select: arrows / Home / End move the focus
 * (Shift extends the range), Space adds or removes the focused rule,
 * Ctrl+A (⌘A) selects every listed rule, Escape keeps only the focused
 * rule (nothing to do when it is the only one).
 */
export function selectionForKey(
  event: Pick<KeyboardEvent, "key" | "shiftKey" | "ctrlKey" | "metaKey" | "altKey">,
  order: readonly string[],
  current: RuleSelection,
  focus: string | null,
): ListSelection | null {
  if (event.altKey) return null;
  const step = FOCUS_STEPS[event.key];
  if (step !== undefined) {
    const next =
      step === "first"
        ? order[0]
        : step === "last"
          ? order[order.length - 1]
          : stepFrom(order, focus, step);
    if (next === undefined) return null;
    return pick(order, current, next, event.shiftKey ? "range" : "single");
  }
  if (focus === null || !order.includes(focus)) return null;
  if (event.key === " ") return pick(order, current, focus, "toggle");
  if (event.key.toLowerCase() === "a" && (event.ctrlKey || event.metaKey)) {
    return pickAll(order, focus);
  }
  if (event.key === "Escape") {
    const alone = current.selected.length === 1 && current.selected[0] === focus;
    return alone ? null : pick(order, current, focus, "single");
  }
  return null;
}

/**
 * The selected saved rules Run and Test act on, in file order, when there
 * are several (only those the filter lists); else none (the editor's rule).
 */
export function selectedRules(shown: readonly Rule[], selected: readonly RuleId[]): Rule[] {
  const ids = new Set(
    inListOrder(
      shown.map((r) => r.id),
      selected,
    ),
  );
  const rules = shown.filter((r) => ids.has(r.id));
  return rules.length > 1 ? rules : [];
}

interface RowProps {
  rule: Rule;
  index: number;
  selected: boolean;
  current: boolean;
  /** The row takes the Tab stop of the list (roving tabindex). */
  tabStop: boolean;
  /** Reordering is off while the list is filtered. */
  filtered: boolean;
  drop: boolean;
  onPick: (event: MouseEvent, id: RuleId) => void;
  onEnabled: (rule: Rule, enabled: boolean) => void;
  onDragFrom: (index: number | null) => void;
  onDragOver: (event: DragEvent<HTMLElement>, index: number) => void;
  onDrop: (event: DragEvent<HTMLElement>, index: number) => void;
}

/** One rule of the list: an option, its content not interactive. */
function RuleRow({
  rule,
  index,
  selected,
  current,
  tabStop,
  filtered,
  drop,
  onPick,
  onEnabled,
  onDragFrom,
  onDragOver,
  onDrop,
}: RowProps) {
  const t = useT();
  const id = useId();
  return (
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the listbox handles the keyboard for its options (roving tabindex).
    <li
      role="option"
      data-part="rules.list.item"
      data-rule={rule.id}
      data-selected={selected}
      data-current={current}
      data-enabled={rule.enabled}
      data-drop={drop ? "target" : undefined}
      aria-selected={selected}
      aria-labelledby={`${id}-name ${id}-triggers`}
      aria-describedby={`${id}-about`}
      tabIndex={tabStop ? 0 : -1}
      onMouseDown={(event) => {
        // Shift+click extends the selection, it does not select text.
        if (event.shiftKey) event.preventDefault();
      }}
      onClick={(event) => {
        onPick(event, rule.id);
      }}
      onDragOver={(event) => {
        onDragOver(event, index);
      }}
      onDrop={(event) => {
        onDrop(event, index);
      }}
    >
      <span
        data-part="rules.list.handle"
        data-disabled={filtered || undefined}
        draggable={!filtered}
        aria-hidden="true"
        title={filtered ? undefined : t("rules-move-rule", { name: rule.name })}
        onDragStart={(event) => {
          event.dataTransfer.effectAllowed = "move";
          event.dataTransfer.setData("text/plain", rule.id);
          onDragFrom(index);
        }}
        onDragEnd={() => {
          onDragFrom(null);
        }}
      >
        <Icon name="grip-vertical" part="rules.list.handle.icon" />
      </span>
      <span data-part="rules.list.main">
        <span data-part="rules.list.name" id={`${id}-name`}>
          {rule.name}
        </span>
        <span data-part="rules.list.triggers" id={`${id}-triggers`}>
          {triggerSummary(rule.triggers)
            .map((which) => t(which))
            .join(t("rules-list-separator"))}
        </span>
      </span>
      <div data-part="rules.list.about" id={`${id}-about`}>
        {!rule.enabled && <span data-part="rules.list.off">{t("rules-list-off")}</span>}
        <ExchangeRuleBadges ruleId={rule.id} />
      </div>
      {/* A pointer shortcut inside an option (which holds nothing interactive); the keyboard switches the rule in the header of the editor. */}
      <span
        data-part="rules.list.enabled"
        data-enabled={rule.enabled}
        aria-hidden="true"
        title={t("rules-list-toggle", { name: rule.name })}
        onClick={(event) => {
          event.stopPropagation();
          onEnabled(rule, !rule.enabled);
        }}
      >
        <Icon name="square-check-big" part="rules.list.enabled.icon" />
      </span>
    </li>
  );
}

function RuleList({
  rules,
  shown,
  filter,
  onFilter,
  focus,
  error,
  onError,
}: {
  rules: readonly Rule[];
  /** The rules the filter lets through, in file order. */
  shown: readonly Rule[];
  filter: string;
  onFilter: (filter: string) => void;
  /** The rule the editor shows (the focused option), `null` for none or a new rule. */
  focus: RuleId | null;
  error: string | null;
  onError: (id: string) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const selection = useRulesUi((s) => s.selection);
  const choose = useRulesUi((s) => s.choose);
  const startNew = useRulesUi((s) => s.startNew);
  const [dragged, setDragged] = useState<number | null>(null);
  const [over, setOver] = useState<number | null>(null);
  const listRef = useRef<HTMLOListElement>(null);
  const focusNext = useRef(false);
  const refresh = () => queryClient.invalidateQueries({ queryKey: rulesKeys.rules });
  const act = async (work: () => Promise<unknown>) => {
    try {
      await work();
      await refresh();
    } catch (error) {
      onError(errorMessageIdOf(error));
    }
  };
  const order = shown.map((r) => r.id);
  const filtered = shown.length !== rules.length;
  const tabStop = focus !== null && order.includes(focus) ? focus : (order[0] ?? null);
  const focusRow = (id: RuleId | null) => {
    if (id === null) return;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-part="rules.list.item"][data-rule="${CSS.escape(id)}"]`)
      ?.focus();
  };
  // A key moved the focus: the newly focused option takes it.
  useEffect(() => {
    if (!focusNext.current) return;
    focusNext.current = false;
    focusRow(focus);
  }, [focus]);
  /** Moves the rule at `from` (of the whole list) to `to`, keeping its focus. */
  const move = (from: number, to: number) => {
    if (to < 0 || to >= rules.length || from === to) return;
    const id = rules[from]?.id ?? null;
    void act(async () => {
      await reorderRules(
        client,
        moveItemTo(rules, from, to).map((r) => r.id),
      );
      await refresh();
      // The moved row keeps the focus.
      focusRow(id);
    });
  };
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const step = reorderStep(event);
    if (step !== 0) {
      if (filtered || tabStop === null) return;
      event.preventDefault();
      const index = rules.findIndex((r) => r.id === tabStop);
      if (index >= 0) move(index, index + step);
      return;
    }
    const next = selectionForKey(event, order, selection, tabStop);
    if (next === null) return;
    event.preventDefault();
    // Escape goes back to one rule here; it does not close the window too.
    event.stopPropagation();
    focusNext.current = next.focus !== focus;
    choose(next);
  };
  const dropOn = (event: DragEvent<HTMLElement>, index: number) => {
    event.preventDefault();
    if (dragged !== null) move(dragged, index);
    setDragged(null);
    setOver(null);
  };
  const selected = new Set(selection.selected);
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
            onFilter(event.target.value);
          }}
        />
        <Issues kind="error" lines={error === null ? [] : [t(error)]} />
      </div>
      {rules.length > 0 && shown.length === 0 && (
        <p data-part="rules.hint">{t("rules-filter-none")}</p>
      )}
      {shown.length > 0 && (
        <ol
          ref={listRef}
          role="listbox"
          aria-multiselectable="true"
          data-part="rules.list.items"
          aria-label={t("rules-list-label")}
          onKeyDown={onKeyDown}
        >
          {shown.map((rule) => {
            const index = rules.indexOf(rule);
            return (
              <RuleRow
                key={rule.id}
                rule={rule}
                index={index}
                selected={selected.has(rule.id)}
                current={rule.id === focus}
                tabStop={rule.id === tabStop}
                filtered={filtered}
                drop={over === index && dragged !== index}
                onPick={(event, id) => {
                  choose(pick(order, selection, id, gestureOf(event)));
                }}
                onEnabled={(r, enabled) => void act(() => saveRule(client, { ...r, enabled }))}
                onDragFrom={(from) => {
                  setDragged(from);
                  if (from === null) setOver(null);
                }}
                onDragOver={(event, at) => {
                  if (dragged === null) return;
                  event.preventDefault();
                  setOver(at);
                }}
                onDrop={dropOn}
              />
            );
          })}
        </ol>
      )}
      {rules.length > 1 && (
        <>
          <p data-part="rules.hint">{t("rules-reorder-hint")}</p>
          <p data-part="rules.hint">{t("rules-select-many-hint")}</p>
        </>
      )}
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
  // Popped out: what the main page handed over (`./popOut.ts`).
  useRulesHandoff();
  const rules = useRules();
  const editing = useRulesUi((s) => s.editing);
  const saved = useRulesUi((s) => s.saved);
  const asking = useRulesUi((s) => s.asking);
  const selection = useRulesUi((s) => s.selection);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const list = rules.data ?? [];
  const shown = filterRules(list, filter);
  const selectedRule =
    editing?.kind === "saved" ? list.find((r) => r.id === editing.id) : undefined;
  const initial = editing?.kind === "new" ? editing.rule : selectedRule;
  const current = editing?.kind === "saved" ? editing.id : null;
  return (
    <section data-part="rules" aria-label={t("rules-title")}>
      <div data-part="rules.panes">
        <div data-part="rules.side" inert={asking}>
          <RuleList
            rules={list}
            shown={shown}
            filter={filter}
            onFilter={setFilter}
            focus={current}
            error={error}
            onError={setError}
          />
        </div>
        {initial ? (
          <RuleEditor
            key={editing?.key}
            initial={initial}
            selection={selectedRules(shown, selection.selected)}
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
