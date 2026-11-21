/**
 * The rules window in a pop-out (R2 review): every webview has its own UI
 * store, so what the main page's message menus asked for — the mail the
 * window is opened from (`origin`) and the rule to show ("Add to rule… →
 * <rule>", "Create rule from message") — reaches a popped-out rules window
 * only as its pop-out context. The main page's preparer hands it over when
 * the window pops out and, as it refreshes, whenever the main page opens
 * the window again while it is popped out; the pop-out page applies each
 * handover once (`useRulesHandoff`, in the rules window).
 */
import { useEffect } from "react";
import type { JsonValue, Rule, RuleId } from "../../api/generated";
import { useClient } from "../../state/queries";
import { usePopOutContext } from "../popout/hooks";
import { isPopOutPage, registerPopOutPreparer } from "../popout/model";
import { RULES_WINDOW } from "./closeGuard";
import { rulesStore, type RulesOrigin, type RulesUiState } from "./store";

/** What the main page hands a popped-out rules window. */
export interface RulesHandoff {
  /** Tells one handover from the next: the pop-out applies each once. */
  id: string;
  /** The mail the window is opened from, `null` when opened otherwise. */
  origin: RulesOrigin | null;
  /** The rule to show; `null` keeps the one the pop-out shows. */
  editing: { kind: "saved"; id: RuleId } | { kind: "new"; rule: Rule } | null;
}

/** The `picks` count each main-page store last handed over. */
const handed = new WeakMap<object, number>();
let handoffs = 0;

/**
 * The handover of the main page's rules state: the origin, and the rule
 * only when one was asked for (`picks`) since the last handover — a rule
 * the main page showed long ago must not replace the one the user opened
 * in the pop-out.
 */
export function rulesHandoff(state: RulesUiState, key: object): RulesHandoff {
  const asked = handed.get(key) !== state.picks;
  handed.set(key, state.picks);
  const editing = asked ? state.editing : null;
  return {
    id: `${Date.now().toString(36)}-${String(++handoffs)}`,
    origin: state.origin,
    editing:
      editing === null
        ? null
        : editing.kind === "saved"
          ? { kind: "saved", id: editing.id }
          : { kind: "new", rule: editing.rule },
  };
}

/** The handover in a pop-out context, `null` when there is none. */
export function handoffOf(context: JsonValue | null): RulesHandoff | null {
  if (context === null || typeof context !== "object" || Array.isArray(context)) return null;
  const { id, origin, editing } = context;
  if (typeof id !== "string" || origin === undefined || editing === undefined) return null;
  return context as unknown as RulesHandoff;
}

/** Does what a handover asks for in the pop-out page's store. */
export function applyHandoff(state: RulesUiState, handoff: RulesHandoff): void {
  state.setOrigin(handoff.origin);
  const editing = handoff.editing;
  if (editing?.kind === "saved") state.select(editing.id);
  else if (editing?.kind === "new") state.startNew(editing.rule);
}

/** The handover each pop-out page's store applied last. */
const applied = new WeakMap<object, string>();

/**
 * In the rules window: applies the handover of the pop-out context, once
 * per handover. Does nothing in the main page (no pop-out context).
 */
export function useRulesHandoff(): void {
  const client = useClient();
  const handoff = handoffOf(usePopOutContext());
  useEffect(() => {
    if (handoff === null) return;
    const store = rulesStore(client);
    if (applied.get(store) === handoff.id) return;
    applied.set(store, handoff.id);
    applyHandoff(store.getState(), handoff);
  }, [client, handoff]);
}

/**
 * Registers the rules window's pop-out preparer in the main page (an
 * effect; renders nothing): it hands over the main page's rules state.
 */
export function RulesPopOutPreparer() {
  const client = useClient();
  useEffect(() => {
    if (isPopOutPage()) return;
    const store = rulesStore(client);
    return registerPopOutPreparer(
      RULES_WINDOW,
      () => rulesHandoff(store.getState(), store) as unknown as JsonValue,
      { refresh: true },
    );
  }, [client]);
  return null;
}
