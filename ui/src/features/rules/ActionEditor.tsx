/**
 * The visual editor of a rule's actions: an ordered list, each action
 * kind with its fields, reordering and removal.
 */
import { useState } from "react";
import type { Account, RuleAction } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useAccounts } from "../../state/queries";
import {
  CheckField,
  SelectField,
  TextAreaField,
  TextField,
  type Choice,
} from "../../widgets/forms";
import { FolderRefFields } from "./ConditionEditor";
import { ACTION_KINDS, moveItem, newAction, parseAddresses, type ActionKind } from "./model";

const kindChoices: Choice[] = ACTION_KINDS.map((value) => ({ value, label: `rules-do-${value}` }));

function AddressesField({
  name,
  value,
  onChange,
}: {
  name: string;
  value: readonly string[];
  onChange: (to: string[]) => void;
}) {
  // Keep the typed text while editing; the list is derived from it.
  const [text, setText] = useState(value.join(", "));
  return (
    <TextField
      labelId="rules-recipients"
      name={name}
      value={text}
      onChange={(next) => {
        setText(next);
        onChange(parseAddresses(next));
      }}
    />
  );
}

function ActionFields({
  name,
  action,
  accounts,
  onChange,
}: {
  name: string;
  action: RuleAction;
  accounts: readonly Account[];
  onChange: (a: RuleAction) => void;
}) {
  const a = action;
  const onOff = (field: string, value: boolean, prefix: string, set: (v: boolean) => void) => (
    <SelectField
      labelId={`${prefix}-label`}
      name={`${name}-${field}`}
      value={value ? "on" : "off"}
      choices={[
        { value: "on", label: `${prefix}-on` },
        { value: "off", label: `${prefix}-off` },
      ]}
      onChange={(v) => set(v === "on")}
    />
  );
  switch (a.type) {
    case "move":
    case "copy":
      return (
        <FolderRefFields
          name={name}
          value={a.folder}
          accounts={accounts}
          onChange={(folder) => onChange({ ...a, folder })}
        />
      );
    case "delete":
      return (
        <CheckField
          labelId="rules-delete-permanent"
          name={`${name}-permanent`}
          checked={a.permanent}
          onChange={(permanent) => onChange({ ...a, permanent })}
        />
      );
    case "tag":
    case "untag":
      return (
        <TextField
          labelId="rules-tag"
          name={`${name}-tag`}
          value={a.tag}
          onChange={(tag) => onChange({ ...a, tag: tag.trim() })}
        />
      );
    case "flag":
      return onOff("flagged", a.flagged, "rules-set-flag", (flagged) =>
        onChange({ ...a, flagged }),
      );
    case "mark-read":
      return onOff("read", a.read, "rules-set-read", (read) => onChange({ ...a, read }));
    case "notify":
      return onOff("notify", a.notify, "rules-set-notify", (notify) => onChange({ ...a, notify }));
    case "forward":
    case "redirect":
      return (
        <AddressesField
          name={`${name}-to`}
          value={a.to}
          onChange={(to) => onChange({ ...a, to })}
        />
      );
    case "reply":
      return (
        <TextAreaField
          labelId="rules-reply-template"
          name={`${name}-template`}
          value={a.template}
          onChange={(template) => onChange({ ...a, template })}
        />
      );
    case "archive":
    case "stop":
      return null;
  }
}

/** The actions of a rule, in order. */
export function ActionEditor({
  value,
  onChange,
}: {
  value: readonly RuleAction[];
  onChange: (value: RuleAction[]) => void;
}) {
  const t = useT();
  const accounts = useAccounts();
  const [kind, setKind] = useState<ActionKind>("move");
  const set = (index: number, action: RuleAction) =>
    onChange(value.map((a, i) => (i === index ? action : a)));
  return (
    <div data-part="rules.actions">
      <ol data-part="rules.actions.items">
        {value.map((action, index) => (
          <li key={index} data-part="rules.action" data-kind={action.type}>
            <SelectField
              labelId="rules-action-kind"
              name={`a-${index}-kind`}
              value={action.type}
              choices={kindChoices}
              onChange={(k) => set(index, newAction(k as ActionKind))}
            />
            <ActionFields
              name={`a-${index}`}
              action={action}
              accounts={accounts.data ?? []}
              onChange={(a) => set(index, a)}
            />
            <button
              type="button"
              data-part="form.button"
              data-action="action-up"
              disabled={index === 0}
              onClick={() => onChange(moveItem(value, index, -1))}
            >
              {t("rules-move-up")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="action-down"
              disabled={index === value.length - 1}
              onClick={() => onChange(moveItem(value, index, 1))}
            >
              {t("rules-move-down")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="remove-action"
              onClick={() => onChange(value.filter((_, i) => i !== index))}
            >
              {t("rules-remove")}
            </button>
          </li>
        ))}
      </ol>
      <div data-part="rules.add">
        <SelectField
          labelId="rules-action-kind"
          name="add-action"
          value={kind}
          choices={kindChoices}
          onChange={(k) => setKind(k as ActionKind)}
        />
        <button
          type="button"
          data-part="form.button"
          data-action="add-action"
          onClick={() => onChange([...value, newAction(kind)])}
        >
          {t("rules-add-action")}
        </button>
      </div>
    </div>
  );
}
