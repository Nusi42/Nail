/**
 * The suggestion controls the rules editor places next to a condition's or
 * an action's value field (R2, ADR 0222): a compact menu button listing the
 * values of the mail the rules window was opened from (the rules store's
 * `origin`) — or, when it was opened otherwise, of the selected message,
 * labelled so — grouped by where they come from (From, To, Subject, …).
 * Picking one puts it into the field: `onPick` gets the value and the
 * whole condition / action with it (the comparison and, for addresses, the
 * field that fit it). Renders nothing without a message or without values
 * for the field.
 *
 * Parts: `rules.from-message` (wrapper, `data-from` = `origin` |
 * `selection`) around the shared menu button (`pim.menu-button`); the menu uses the shared
 * `context-menu.*` parts.
 */
import { useQueryClient } from "@tanstack/react-query";
import type { ReactNode } from "react";
import type { Condition, MessageFacts, MessageId, RuleAction } from "../../../api/generated";
import { useT } from "../../../i18n/I18nProvider";
import { findSummary } from "../../../state/mailOps";
import { useAccounts, useFolders } from "../../../state/queries";
import { useUi } from "../../../state/uiStore";
import { PimMenu, type PimMenuEntry } from "../../pim/PimMenu";
import { useRulesUi } from "../store";
import {
  applyToAction,
  applyToCondition,
  useMessageFacts,
  valuesForAction,
  valuesForCondition,
  type MessageValue,
  type MessageValueContext,
  type MessageValueKind,
  type MessageValueSource,
} from "./facts";

export interface ValuesFromMessageProps {
  /** The condition whose value the picked value replaces. */
  condition: Condition;
  /**
   * Called with the picked value and the condition with it (comparison and
   * address field set to fit the value).
   */
  onPick: (value: string, condition: Condition) => void;
}

export interface ActionValuesFromMessageProps {
  /** The action whose value (folder, tag) the picked value replaces. */
  action: RuleAction;
  /** Called with the picked value and the action with it. */
  onPick: (value: string, action: RuleAction) => void;
}

/** The message whose values are offered, and why it is that one. */
export interface ValuesMessage {
  messageId: MessageId;
  /** `origin`: the window was opened from it; `selection`: it is selected. */
  from: "origin" | "selection";
}

/** The window's origin message, else the selected message, else `null`. */
export function valuesMessageOf(
  origin: { messageId: MessageId } | null,
  selected: MessageId | null,
): ValuesMessage | null {
  if (origin) return { messageId: origin.messageId, from: "origin" };
  return selected === null ? null : { messageId: selected, from: "selection" };
}

/** Fluent ids of the menu's group headers. */
const SOURCE_LABELS: Readonly<Record<MessageValueSource, string>> = {
  from: "rules-from-message-source-from",
  sender: "rules-from-message-source-sender",
  "reply-to": "rules-from-message-source-reply-to",
  to: "rules-from-message-source-to",
  cc: "rules-from-message-source-cc",
  subject: "rules-from-message-source-subject",
  "list-id": "rules-from-message-source-list-id",
  account: "rules-from-message-source-account",
  folder: "rules-from-message-source-folder",
  header: "rules-from-message-source-header",
  tag: "rules-from-message-source-tag",
};

/** Fluent ids of the items, by kind (argument `$value`: what the list shows). */
const KIND_LABELS: Readonly<Record<MessageValueKind, string>> = {
  address: "rules-from-message-kind-address",
  name: "rules-from-message-kind-name",
  domain: "rules-from-message-kind-domain",
  subject: "rules-from-message-kind-subject",
  "subject-word": "rules-from-message-kind-subject-word",
  "mailing-list": "rules-from-message-kind-mailing-list",
  account: "rules-from-message-kind-account",
  folder: "rules-from-message-kind-folder",
  "header-name": "rules-from-message-kind-header-name",
  tag: "rules-from-message-kind-tag",
};

/** The menu's entries: a header per source, then its values. */
export function menuEntries(
  values: readonly MessageValue[],
  t: (id: string, args?: Record<string, string>) => string,
  onSelect: (value: MessageValue) => void,
): PimMenuEntry[] {
  const entries: PimMenuEntry[] = [];
  let source: MessageValueSource | null = null;
  values.forEach((v, index) => {
    if (v.source !== source) {
      source = v.source;
      entries.push({ type: "header", id: `source-${v.source}`, label: t(SOURCE_LABELS[v.source]) });
    }
    entries.push({
      type: "item",
      id: `value-${String(index)}`,
      label: t(KIND_LABELS[v.kind], { value: v.label }),
      onSelect: () => {
        onSelect(v);
      },
    });
  });
  return entries;
}

/** The message the controls offer, its facts and what they need besides. */
function useValuesSource(): {
  message: ValuesMessage | null;
  facts: MessageFacts | undefined;
  context: MessageValueContext;
} {
  const origin = useRulesUi((s) => s.origin);
  const selected = useUi((s) => s.selectedMessageId);
  const message = valuesMessageOf(origin, selected);
  const facts = useMessageFacts(message?.messageId ?? null).data;
  const accounts = useAccounts().data;
  const folders = useFolders(facts?.account_id ?? null).data;
  const queryClient = useQueryClient();
  const tags = facts ? findSummary(queryClient, facts.message_id)?.tags : undefined;
  return { message, facts, context: { accounts, folders, tags } };
}

/** The menu button over `values`, or nothing without values. */
function ValuesMenu({
  message,
  values,
  onPick,
}: {
  message: ValuesMessage;
  values: readonly MessageValue[];
  onPick: (value: MessageValue) => void;
}): ReactNode {
  const t = useT();
  if (values.length === 0) return null;
  const label = t(
    message.from === "origin" ? "rules-from-message-values" : "rules-from-message-values-selected",
  );
  return (
    <span data-part="rules.from-message" data-from={message.from}>
      <PimMenu
        label={label}
        icon="mail-open"
        field="rules-from-message-values"
        entries={menuEntries(values, t, onPick)}
      />
    </span>
  );
}

/** "Values from this mail" beside a condition's value field. */
export function ValuesFromMessage({ condition, onPick }: ValuesFromMessageProps): ReactNode {
  const { message, facts, context } = useValuesSource();
  if (!message || !facts) return null;
  return (
    <ValuesMenu
      message={message}
      values={valuesForCondition(facts, condition, context)}
      onPick={(v) => {
        onPick(v.value, applyToCondition(condition, v));
      }}
    />
  );
}

/** "Values from this mail" beside an action's folder or tag field. */
export function ActionValuesFromMessage({
  action,
  onPick,
}: ActionValuesFromMessageProps): ReactNode {
  const { message, facts, context } = useValuesSource();
  if (!message || !facts) return null;
  return (
    <ValuesMenu
      message={message}
      values={valuesForAction(facts, action, context)}
      onPick={(v) => {
        onPick(v.value, applyToAction(action, v));
      }}
    />
  );
}
