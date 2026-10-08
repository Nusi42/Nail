/**
 * The visual editor of a rule's conditions: nested all / any / not groups
 * and one row per condition, each kind with its own fields.
 */
import { useId, useState } from "react";
import type {
  Account,
  AddressField,
  AuthMethod,
  AuthResult,
  Condition,
  FolderRef,
  FolderRole,
  MessageFlag,
  TextMatch,
  TextOp,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useAccounts } from "../../state/queries";
import { CheckField, NumberField, SelectField, TextField, type Choice } from "../../widgets/forms";
import {
  CONDITION_KINDS,
  TEXT_OPS,
  addChild,
  childrenOf,
  conditionAt,
  newCondition,
  removeAt,
  replaceAt,
  textMatch,
  type ConditionKind,
  type ConditionPath,
} from "./model";
import { FOLDER_ROLES } from "../../api/contract";

export const ADDRESS_FIELDS: readonly AddressField[] = [
  "from",
  "to",
  "cc",
  "recipients",
  "reply-to",
  "sender",
];
export const FLAGS: readonly MessageFlag[] = ["seen", "flagged", "answered", "draft"];
export const AUTH_METHODS: readonly AuthMethod[] = ["dkim", "spf", "dmarc"];
export const AUTH_RESULTS: readonly AuthResult[] = [
  "pass",
  "fail",
  "softfail",
  "neutral",
  "none",
  "temperror",
  "permerror",
];

const choices = (values: readonly string[], prefix: string): Choice[] =>
  values.map((value) => ({ value, label: `${prefix}${value}` }));

/** Operator, text and case of a comparison; `optional` adds "any" / "exists". */
function TextMatchFields({
  name,
  value,
  optional,
  onChange,
}: {
  name: string;
  value: TextMatch | null;
  /** Fluent id of the choice meaning "no comparison" (`null`). */
  optional?: string;
  onChange: (value: TextMatch | null) => void;
}) {
  const ops: Choice[] = [
    ...(optional ? [{ value: "", label: optional }] : []),
    ...choices(TEXT_OPS, "rules-op-"),
  ];
  return (
    <>
      <SelectField
        labelId="rules-op"
        name={`${name}-op`}
        value={value?.op ?? ""}
        choices={ops}
        onChange={(op) => {
          onChange(op === "" ? null : { ...(value ?? textMatch()), op: op as TextOp });
        }}
      />
      {value && (
        <>
          <TextField
            labelId="rules-value"
            name={`${name}-value`}
            value={value.value}
            onChange={(text) => {
              onChange({ ...value, value: text });
            }}
          />
          <CheckField
            labelId="rules-case-sensitive"
            name={`${name}-case`}
            checked={value.case_sensitive}
            onChange={(case_sensitive) => {
              onChange({ ...value, case_sensitive });
            }}
          />
        </>
      )}
    </>
  );
}

/** A folder: by path or special role, in the message's account or a named one. */
export function FolderRefFields({
  name,
  value,
  accounts,
  onChange,
}: {
  name: string;
  value: FolderRef;
  accounts: readonly Account[];
  onChange: (value: FolderRef) => void;
}) {
  const accountChoices: Choice[] = [
    { value: "", label: "rules-folder-account-own" },
    ...accounts
      .filter((a) => a.kind !== "virtual")
      .map((a) => ({ value: a.id, label: a.name || a.email, literal: true })),
  ];
  const account_id = value.account_id;
  return (
    <>
      <SelectField
        labelId="rules-folder-mode"
        name={`${name}-mode`}
        value={value.type}
        choices={[
          { value: "path", label: "rules-folder-by-path" },
          { value: "role", label: "rules-folder-by-role" },
        ]}
        onChange={(mode) => {
          onChange(
            mode === "role"
              ? { type: "role", role: "archive", account_id }
              : { type: "path", path: "", account_id },
          );
        }}
      />
      {value.type === "path" ? (
        <TextField
          labelId="rules-folder-path"
          name={`${name}-path`}
          value={value.path}
          onChange={(path) => {
            onChange({ ...value, path });
          }}
        />
      ) : (
        <SelectField
          labelId="rules-folder-role"
          name={`${name}-role`}
          value={value.role}
          choices={choices(FOLDER_ROLES, "rules-role-")}
          onChange={(role) => {
            onChange({ ...value, role: role as FolderRole });
          }}
        />
      )}
      <SelectField
        labelId="rules-folder-account"
        name={`${name}-account`}
        value={account_id ?? ""}
        choices={accountChoices}
        onChange={(id) => {
          onChange({ ...value, account_id: id === "" ? null : id });
        }}
      />
    </>
  );
}

/** The fields of one (non-group) condition. */
function LeafFields({
  name,
  condition,
  accounts,
  onChange,
}: {
  name: string;
  condition: Condition;
  accounts: readonly Account[];
  onChange: (c: Condition) => void;
}) {
  const c = condition;
  switch (c.type) {
    case "address":
      return (
        <>
          <SelectField
            labelId="rules-field"
            name={`${name}-field`}
            value={c.field}
            choices={choices(ADDRESS_FIELDS, "rules-field-")}
            onChange={(field) => {
              onChange({ ...c, field: field as AddressField });
            }}
          />
          <TextMatchFields
            name={name}
            value={c.matches}
            onChange={(m) => {
              onChange({ ...c, matches: m ?? textMatch() });
            }}
          />
        </>
      );
    case "subject":
    case "body":
      return (
        <TextMatchFields
          name={name}
          value={c.matches}
          onChange={(m) => {
            onChange({ ...c, matches: m ?? textMatch() });
          }}
        />
      );
    case "header":
      return (
        <>
          <TextField
            labelId="rules-header-name"
            name={`${name}-header`}
            value={c.name}
            onChange={(header) => {
              onChange({ ...c, name: header.trim() });
            }}
          />
          <TextMatchFields
            name={name}
            value={c.matches}
            optional="rules-op-exists"
            onChange={(matches) => {
              onChange({ ...c, matches });
            }}
          />
        </>
      );
    case "list-id":
      return (
        <TextMatchFields
          name={name}
          value={c.matches}
          optional="rules-op-exists"
          onChange={(matches) => {
            onChange({ ...c, matches });
          }}
        />
      );
    case "query":
      return (
        <TextField
          labelId="rules-query"
          name={`${name}-query`}
          value={c.query}
          onChange={(query) => {
            onChange({ ...c, query });
          }}
        />
      );
    case "attachment":
      return (
        <>
          <TextMatchFields
            name={`${name}-name`}
            value={c.name}
            optional="rules-op-any-name"
            onChange={(m) => {
              onChange({ ...c, name: m });
            }}
          />
          <TextMatchFields
            name={`${name}-mime`}
            value={c.mime}
            optional="rules-op-any-type"
            onChange={(m) => {
              onChange({ ...c, mime: m });
            }}
          />
        </>
      );
    case "size":
      return (
        <>
          <SelectField
            labelId="rules-compare"
            name={`${name}-cmp`}
            value={c.cmp}
            choices={choices(["greater", "less"], "rules-size-")}
            onChange={(cmp) => {
              onChange({ ...c, cmp: cmp as "greater" | "less" });
            }}
          />
          <NumberField
            labelId="rules-size-kb"
            name={`${name}-kb`}
            value={Math.round(c.bytes / 1024)}
            onChange={(kb) => {
              onChange({ ...c, bytes: (kb ?? 0) * 1024 });
            }}
          />
        </>
      );
    case "date":
      return (
        <>
          <SelectField
            labelId="rules-compare"
            name={`${name}-cmp`}
            value={c.cmp}
            choices={choices(["before", "after"], "rules-date-")}
            onChange={(cmp) => {
              onChange({ ...c, cmp: cmp as "before" | "after" });
            }}
          />
          <DateField
            name={`${name}-date`}
            value={c.date}
            onChange={(date) => {
              onChange({ ...c, date });
            }}
          />
        </>
      );
    case "age":
      return (
        <>
          <SelectField
            labelId="rules-compare"
            name={`${name}-cmp`}
            value={c.cmp}
            choices={choices(["greater", "less"], "rules-age-")}
            onChange={(cmp) => {
              onChange({ ...c, cmp: cmp as "greater" | "less" });
            }}
          />
          <NumberField
            labelId="rules-days"
            name={`${name}-days`}
            value={c.days}
            onChange={(days) => {
              onChange({ ...c, days: days ?? 0 });
            }}
          />
        </>
      );
    case "flag":
      return (
        <>
          <SelectField
            labelId="rules-flag"
            name={`${name}-flag`}
            value={c.flag}
            choices={choices(FLAGS, "rules-flag-")}
            onChange={(flag) => {
              onChange({ ...c, flag: flag as MessageFlag });
            }}
          />
          <SelectField
            labelId="rules-flag-state"
            name={`${name}-set`}
            value={c.set ? "set" : "unset"}
            choices={choices(["set", "unset"], "rules-flag-state-")}
            onChange={(v) => {
              onChange({ ...c, set: v === "set" });
            }}
          />
        </>
      );
    case "tag":
      return (
        <TextField
          labelId="rules-tag"
          name={`${name}-tag`}
          value={c.tag}
          onChange={(tag) => {
            onChange({ ...c, tag: tag.trim() });
          }}
        />
      );
    case "account":
      return (
        <SelectField
          labelId="rules-account"
          name={`${name}-account`}
          value={c.account_id}
          choices={[
            { value: "", label: "rules-choose" },
            ...accounts.map((a) => ({ value: a.id, label: a.name || a.email, literal: true })),
          ]}
          onChange={(account_id) => {
            onChange({ ...c, account_id });
          }}
        />
      );
    case "folder":
      return (
        <FolderRefFields
          name={name}
          value={c.folder}
          accounts={accounts}
          onChange={(folder) => {
            onChange({ ...c, folder });
          }}
        />
      );
    case "auth":
      return (
        <>
          <SelectField
            labelId="rules-auth-method"
            name={`${name}-method`}
            value={c.method}
            choices={choices(AUTH_METHODS, "rules-auth-")}
            onChange={(method) => {
              onChange({ ...c, method: method as AuthMethod });
            }}
          />
          <SelectField
            labelId="rules-auth-result"
            name={`${name}-result`}
            value={c.result}
            choices={choices(AUTH_RESULTS, "rules-auth-result-")}
            onChange={(result) => {
              onChange({ ...c, result: result as AuthResult });
            }}
          />
        </>
      );
    default:
      return null;
  }
}

function DateField({
  name,
  value,
  onChange,
}: {
  name: string;
  value: string;
  onChange: (value: string) => void;
}) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {t("common-date")}
      </label>
      <input
        id={id}
        data-part="form.input"
        type="date"
        name={name}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </div>
  );
}

/** Adds a condition or a group to a group. */
function AddCondition({ path, onAdd }: { path: ConditionPath; onAdd: (c: Condition) => void }) {
  const t = useT();
  const [kind, setKind] = useState<ConditionKind>("address");
  return (
    <div data-part="rules.add">
      <SelectField
        labelId="rules-condition-kind"
        name={`add-${path.join("-")}`}
        value={kind}
        choices={choices(CONDITION_KINDS, "rules-cond-")}
        onChange={(k) => {
          setKind(k as ConditionKind);
        }}
      />
      <button
        type="button"
        data-part="form.button"
        data-action="add-condition"
        onClick={() => {
          onAdd(newCondition(kind));
        }}
      >
        {t("rules-add-condition")}
      </button>
    </div>
  );
}

/** One condition (a row) or group (a nested box), at `path` of the root. */
function ConditionNode({
  root,
  path,
  accounts,
  onChange,
}: {
  root: Condition;
  path: ConditionPath;
  accounts: readonly Account[];
  onChange: (root: Condition) => void;
}) {
  const t = useT();
  const condition = conditionAt(root, path);
  if (!condition) return null;
  const name = `c${path.length === 0 ? "" : `-${path.join("-")}`}`;
  const remove =
    path.length > 0 ? (
      <button
        type="button"
        data-part="form.button"
        data-action="remove-condition"
        onClick={() => {
          onChange(removeAt(root, path));
        }}
      >
        {t("common-remove")}
      </button>
    ) : null;
  if (condition.type === "all" || condition.type === "any" || condition.type === "not") {
    const kids = childrenOf(condition);
    return (
      <fieldset data-part="rules.group" data-mode={condition.type} data-depth={path.length}>
        <legend data-part="rules.group.title">{t(`rules-group-${condition.type}`)}</legend>
        {condition.type !== "not" && (
          <SelectField
            labelId="rules-group-mode"
            name={`${name}-mode`}
            value={condition.type}
            choices={choices(["all", "any"], "rules-group-")}
            onChange={(mode) => {
              onChange(
                replaceAt(root, path, {
                  type: mode as "all" | "any",
                  conditions: kids,
                }),
              );
            }}
          />
        )}
        <ul data-part="rules.group.items">
          {kids.map((_, i) => (
            <li key={i} data-part="rules.group.item">
              <ConditionNode
                root={root}
                path={[...path, i]}
                accounts={accounts}
                onChange={onChange}
              />
            </li>
          ))}
        </ul>
        {(condition.type !== "not" || kids.length === 0) && (
          <AddCondition
            path={path}
            onAdd={(c) => {
              onChange(addChild(root, path, c));
            }}
          />
        )}
        {remove}
      </fieldset>
    );
  }
  return (
    <div data-part="rules.condition" data-kind={condition.type}>
      <SelectField
        labelId="rules-condition-kind"
        name={`${name}-kind`}
        value={condition.type}
        choices={choices(CONDITION_KINDS, "rules-cond-")}
        onChange={(kind) => {
          onChange(replaceAt(root, path, newCondition(kind as ConditionKind)));
        }}
      />
      <LeafFields
        name={name}
        condition={condition}
        accounts={accounts}
        onChange={(c) => {
          onChange(replaceAt(root, path, c));
        }}
      />
      {remove}
    </div>
  );
}

/** The conditions of a rule: the root group and everything in it. */
export function ConditionEditor({
  value,
  onChange,
}: {
  value: Condition;
  onChange: (value: Condition) => void;
}) {
  const accounts = useAccounts();
  return (
    <div data-part="rules.conditions">
      <ConditionNode root={value} path={[]} accounts={accounts.data ?? []} onChange={onChange} />
    </div>
  );
}
