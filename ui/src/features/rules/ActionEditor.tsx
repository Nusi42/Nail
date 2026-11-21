/**
 * The visual editor of a rule's actions (the THEN row): an ordered, numbered
 * list, each action kind with its fields, reordering and removal, and
 * "+ Add action" (a menu button, R2 design review item 9). Folders, tags
 * and recipients are shared pickers (`./PickerField`), and the folder and tag
 * fields offer the values of the mail the window was opened from
 * (`./fromMessage`); an action that still
 * misses its value is marked and names the footer's problem badge.
 */
import { useState } from "react";
import type { RuleAction } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { CheckField, SelectField, TextAreaField, type Choice } from "../../widgets/forms";
import { AddMenu } from "./AddMenu";
import { ActionValuesFromMessage } from "./fromMessage";
import { FolderRefFields, type Marked } from "./ConditionEditor";
import {
  ACTION_KINDS,
  actionInvalid,
  moveItem,
  newAction,
  parseAddresses,
  type ActionKind,
  type RuleIssue,
} from "./model";
import { PickerField } from "./PickerField";

const kindChoices: Choice[] = ACTION_KINDS.map((value) => ({ value, label: `rules-do-${value}` }));

function AddressesField({
  name,
  value,
  onChange,
  ...marked
}: {
  name: string;
  value: readonly string[];
  onChange: (to: string[]) => void;
} & Marked) {
  // Keep the typed text while editing; the list is derived from it.
  const [text, setText] = useState(value.join(", "));
  return (
    <PickerField
      kind="address"
      labelId="rules-recipients"
      name={name}
      value={text}
      {...marked}
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
  onChange,
  ...marked
}: {
  name: string;
  action: RuleAction;
  onChange: (a: RuleAction) => void;
} & Marked) {
  const a = action;
  const onOff = (
    field: string,
    value: boolean,
    prefix: "rules-set-flag" | "rules-set-read" | "rules-set-notify",
    set: (v: boolean) => void,
  ) => (
    <SelectField
      labelId={`${prefix}-label`}
      name={`${name}-${field}`}
      value={value ? "on" : "off"}
      choices={[
        { value: "on", label: `${prefix}-on` },
        { value: "off", label: `${prefix}-off` },
      ]}
      onChange={(v) => {
        set(v === "on");
      }}
    />
  );
  switch (a.type) {
    case "move":
    case "copy":
      return (
        <FolderRefFields
          name={name}
          value={a.folder}
          {...marked}
          onChange={(folder) => {
            onChange({ ...a, folder });
          }}
        />
      );
    case "delete":
      return (
        <CheckField
          labelId="rules-delete-permanent"
          name={`${name}-permanent`}
          checked={a.permanent}
          onChange={(permanent) => {
            onChange({ ...a, permanent });
          }}
        />
      );
    case "tag":
    case "untag":
      return (
        <PickerField
          kind="tag"
          labelId="rules-tag"
          name={`${name}-tag`}
          value={a.tag}
          {...marked}
          onChange={(tag) => {
            onChange({ ...a, tag: tag.trim() });
          }}
        />
      );
    case "flag":
      return onOff("flagged", a.flagged, "rules-set-flag", (flagged) => {
        onChange({ ...a, flagged });
      });
    case "mark-read":
      return onOff("read", a.read, "rules-set-read", (read) => {
        onChange({ ...a, read });
      });
    case "notify":
      return onOff("notify", a.notify, "rules-set-notify", (notify) => {
        onChange({ ...a, notify });
      });
    case "forward":
    case "redirect":
      return (
        <AddressesField
          name={`${name}-to`}
          value={a.to}
          {...marked}
          onChange={(to) => {
            onChange({ ...a, to });
          }}
        />
      );
    case "reply":
      return (
        <TextAreaField
          labelId="rules-reply-template"
          name={`${name}-template`}
          value={a.template}
          onChange={(template) => {
            onChange({ ...a, template });
          }}
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
  issues = [],
  describedBy,
  onChange,
}: {
  value: readonly RuleAction[];
  issues?: readonly RuleIssue[];
  describedBy?: string | undefined;
  onChange: (value: RuleAction[]) => void;
}) {
  const t = useT();
  const set = (index: number, action: RuleAction) => {
    onChange(value.map((a, i) => (i === index ? action : a)));
  };
  return (
    <div data-part="rules.actions">
      <ol data-part="rules.actions.items">
        {value.map((action, index) => (
          <li key={index} data-part="rules.action" data-kind={action.type}>
            <SelectField
              labelId="rules-action-kind"
              name={`a-${String(index)}-kind`}
              value={action.type}
              width="content"
              choices={kindChoices}
              onChange={(k) => {
                set(index, newAction(k as ActionKind));
              }}
            />
            <ActionFields
              name={`a-${String(index)}`}
              action={action}
              invalid={actionInvalid(issues, index)}
              describedBy={describedBy}
              onChange={(a) => {
                set(index, a);
              }}
            />
            <ActionValuesFromMessage
              action={action}
              onPick={(_value, next) => {
                set(index, next);
              }}
            />
            <span data-part="rules.action.tools">
              <button
                type="button"
                data-part="form.button"
                data-action="action-up"
                disabled={index === 0}
                onClick={() => {
                  onChange(moveItem(value, index, -1));
                }}
              >
                {t("common-move-up")}
              </button>
              <button
                type="button"
                data-part="form.button"
                data-action="action-down"
                disabled={index === value.length - 1}
                onClick={() => {
                  onChange(moveItem(value, index, 1));
                }}
              >
                {t("common-move-down")}
              </button>
              <button
                type="button"
                data-part="form.button"
                data-action="remove-action"
                onClick={() => {
                  onChange(value.filter((_, i) => i !== index));
                }}
              >
                {t("common-remove")}
              </button>
            </span>
          </li>
        ))}
      </ol>
      <AddMenu
        labelId="common-add-action"
        field="add-action"
        kinds={ACTION_KINDS}
        labelOf={(kind) => t(`rules-do-${kind}`)}
        onAdd={(kind) => {
          onChange([...value, newAction(kind)]);
        }}
      />
    </div>
  );
}
