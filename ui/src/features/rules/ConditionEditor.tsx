/**
 * The visual editor of a rule's conditions: nested all / any / not groups
 * and one row per condition, each kind with its own fields.
 *
 * R2 (design review, item 9): the root group is the IF row's "Match all /
 * any of:" — no box and no second legend; nested groups keep their box.
 * "+ Add condition" is a menu button. Every value that names a folder,
 * account, tag, address, mailing list or header name is a shared picker
 * (`./PickerField`); beside each condition the window offers the values of
 * the message it was opened from (`./fromMessage`, `ValuesFromMessage`). A
 * condition that still misses its value is marked (`aria-invalid`) and
 * names the footer's problem badge.
 */
import { useId } from "react";
import {
  ADDRESS_FIELD_VALUES,
  type AddressField,
  AUTH_METHOD_VALUES,
  AUTH_RESULT_VALUES,
  type AuthMethod,
  type AuthResult,
  type Condition,
  type FolderRef,
  type FolderRole,
  MESSAGE_FLAG_VALUES,
  type MessageFlag,
  type TextMatch,
  type TextOp,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { CheckField, NumberField, SelectField, TextField, type Choice } from "../../widgets/forms";
import type { PickerKind } from "../../widgets/pickers";
import { AddMenu } from "./AddMenu";
import { ValuesFromMessage } from "./fromMessage";
import {
  CONDITION_KINDS,
  GROUP_KINDS,
  TEXT_OPS,
  addChild,
  childrenOf,
  conditionAt,
  conditionInvalid,
  newCondition,
  removeAt,
  replaceAt,
  textMatch,
  withPickedValue,
  type ConditionKind,
  type ConditionPath,
  type RuleIssue,
} from "./model";
import { PickerField } from "./PickerField";
import { FOLDER_ROLES } from "../../api/contract";
export {
  ADDRESS_FIELD_VALUES as ADDRESS_FIELDS,
  AUTH_METHOD_VALUES as AUTH_METHODS,
  AUTH_RESULT_VALUES as AUTH_RESULTS,
  MESSAGE_FLAG_VALUES as FLAGS,
} from "../../api/generated";

const choices = (values: readonly string[], prefix: string): Choice[] =>
  values.map((value) => ({ value, label: `${prefix}${value}` }));

/** Whether a field misses its value, and the problem badge that says so. */
export interface Marked {
  invalid?: boolean | undefined;
  describedBy?: string | undefined;
}

/**
 * Operator, text and case of a comparison; `optional` adds "any" /
 * "exists"; `picker` makes the text a shared picker of that kind.
 */
function TextMatchFields({
  name,
  value,
  optional,
  picker,
  invalid,
  describedBy,
  onChange,
}: {
  name: string;
  value: TextMatch | null;
  /** Fluent id of the choice meaning "no comparison" (`null`). */
  optional?: string;
  picker?: PickerKind;
  onChange: (value: TextMatch | null) => void;
} & Marked) {
  const ops: Choice[] = [
    ...(optional ? [{ value: "", label: optional }] : []),
    ...choices(TEXT_OPS, "rules-op-"),
  ];
  const setText = (text: string) => {
    if (value) onChange({ ...value, value: text });
  };
  return (
    <>
      <SelectField
        labelId="rules-op"
        name={`${name}-op`}
        value={value?.op ?? ""}
        width="content"
        choices={ops}
        onChange={(op) => {
          onChange(op === "" ? null : { ...(value ?? textMatch()), op: op as TextOp });
        }}
      />
      {value && (
        <>
          {picker ? (
            <PickerField
              kind={picker}
              labelId="rules-value"
              name={`${name}-value`}
              value={value.value}
              invalid={invalid}
              describedBy={describedBy}
              onChange={setText}
            />
          ) : (
            <TextField
              labelId="rules-value"
              name={`${name}-value`}
              value={value.value}
              invalid={invalid === true}
              onChange={setText}
            />
          )}
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

/**
 * A folder: by path (a folder picker over the named account, else every
 * account) or special role, in the message's account or a named one (an
 * account picker; empty = the message's account).
 */
export function FolderRefFields({
  name,
  value,
  invalid,
  describedBy,
  onChange,
}: {
  name: string;
  value: FolderRef;
  onChange: (value: FolderRef) => void;
} & Marked) {
  const account_id = value.account_id;
  return (
    <>
      <SelectField
        labelId="rules-folder-mode"
        name={`${name}-mode`}
        value={value.type}
        width="content"
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
        <PickerField
          kind="folder"
          labelId="rules-folder-path"
          name={`${name}-path`}
          value={value.path}
          accountId={account_id}
          invalid={invalid}
          describedBy={describedBy}
          onChange={(path) => {
            onChange({ ...value, path });
          }}
        />
      ) : (
        <SelectField
          labelId="rules-folder-role"
          name={`${name}-role`}
          value={value.role}
          width="content"
          choices={choices(FOLDER_ROLES, "rules-role-")}
          onChange={(role) => {
            onChange({ ...value, role: role as FolderRole });
          }}
        />
      )}
      <PickerField
        kind="account"
        labelId="rules-folder-account"
        name={`${name}-account`}
        value={account_id ?? ""}
        placeholderId="rules-folder-account-own"
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
  onChange,
  ...marked
}: {
  name: string;
  condition: Condition;
  onChange: (c: Condition) => void;
} & Marked) {
  const c = condition;
  switch (c.type) {
    case "address":
      return (
        <>
          <SelectField
            labelId="rules-field"
            name={`${name}-field`}
            value={c.field}
            width="content"
            choices={choices(ADDRESS_FIELD_VALUES, "rules-field-")}
            onChange={(field) => {
              onChange({ ...c, field: field as AddressField });
            }}
          />
          <TextMatchFields
            name={name}
            value={c.matches}
            picker="address"
            {...marked}
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
          {...marked}
          onChange={(m) => {
            onChange({ ...c, matches: m ?? textMatch() });
          }}
        />
      );
    case "header":
      return (
        <>
          <PickerField
            kind="header-name"
            labelId="rules-header-name"
            name={`${name}-header`}
            value={c.name}
            {...marked}
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
          picker="mailing-list"
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
          invalid={marked.invalid === true}
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
            width="content"
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
            width="content"
            choices={choices(["before", "after"], "rules-date-")}
            onChange={(cmp) => {
              onChange({ ...c, cmp: cmp as "before" | "after" });
            }}
          />
          <DateField
            name={`${name}-date`}
            value={c.date}
            {...marked}
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
            width="content"
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
            width="content"
            choices={choices(MESSAGE_FLAG_VALUES, "rules-flag-")}
            onChange={(flag) => {
              onChange({ ...c, flag: flag as MessageFlag });
            }}
          />
          <SelectField
            labelId="rules-flag-state"
            name={`${name}-set`}
            value={c.set ? "set" : "unset"}
            width="content"
            choices={choices(["set", "unset"], "rules-flag-state-")}
            onChange={(v) => {
              onChange({ ...c, set: v === "set" });
            }}
          />
        </>
      );
    case "tag":
      return (
        <PickerField
          kind="tag"
          labelId="rules-tag"
          name={`${name}-tag`}
          value={c.tag}
          {...marked}
          onChange={(tag) => {
            onChange({ ...c, tag: tag.trim() });
          }}
        />
      );
    case "account":
      return (
        <PickerField
          kind="account"
          labelId="rules-account"
          name={`${name}-account`}
          value={c.account_id}
          placeholderId="rules-choose"
          {...marked}
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
          {...marked}
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
            width="content"
            choices={choices(AUTH_METHOD_VALUES, "rules-auth-")}
            onChange={(method) => {
              onChange({ ...c, method: method as AuthMethod });
            }}
          />
          <SelectField
            labelId="rules-auth-result"
            name={`${name}-result`}
            value={c.result}
            width="content"
            choices={choices(AUTH_RESULT_VALUES, "rules-auth-result-")}
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
  invalid,
  describedBy,
  onChange,
}: {
  name: string;
  value: string;
  onChange: (value: string) => void;
} & Marked) {
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
        aria-invalid={invalid === true || undefined}
        aria-describedby={invalid === true ? describedBy : undefined}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </div>
  );
}

/** "+ Add condition": a condition or a group added to the group at `path`. */
function AddCondition({ path, onAdd }: { path: ConditionPath; onAdd: (c: Condition) => void }) {
  const t = useT();
  return (
    <AddMenu
      labelId="rules-add-condition"
      field={["add", "c", ...path].join("-")}
      kinds={CONDITION_KINDS}
      groups={GROUP_KINDS}
      labelOf={(kind) => t(`rules-cond-${kind}`)}
      onAdd={(kind) => {
        onAdd(newCondition(kind));
      }}
    />
  );
}

interface NodeProps {
  root: Condition;
  path: ConditionPath;
  issues: readonly RuleIssue[];
  /** The id of the footer's problem details. */
  describedBy: string | undefined;
  onChange: (root: Condition) => void;
}

/** The "Match all / any" choice of the group at `path`. */
function GroupMode({
  root,
  path,
  name,
  condition,
  onChange,
}: {
  root: Condition;
  path: ConditionPath;
  name: string;
  condition: Extract<Condition, { type: "all" | "any" }>;
  onChange: (root: Condition) => void;
}) {
  return (
    <SelectField
      labelId="rules-group-mode"
      name={`${name}-mode`}
      value={condition.type}
      width="content"
      choices={choices(["all", "any"], "rules-group-")}
      onChange={(mode) => {
        onChange(
          replaceAt(root, path, {
            type: mode as "all" | "any",
            conditions: condition.conditions,
          }),
        );
      }}
    />
  );
}

/** The children of the group at `path`, each a row (or a nested box). */
function GroupItems({ kids, ...node }: NodeProps & { kids: readonly Condition[] }) {
  return (
    <ul data-part="rules.group.items">
      {kids.map((_, i) => (
        <li key={i} data-part="rules.group.item">
          <ConditionNode {...node} path={[...node.path, i]} />
        </li>
      ))}
    </ul>
  );
}

/** One condition (a row) or nested group (a box), at `path` of the root. */
function ConditionNode(node: NodeProps) {
  const { root, path, issues, describedBy, onChange } = node;
  const t = useT();
  const condition = conditionAt(root, path);
  if (!condition) return null;
  const name = ["c", ...path].join("-");
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
          <GroupMode
            root={root}
            path={path}
            name={name}
            condition={condition}
            onChange={onChange}
          />
        )}
        <GroupItems {...node} kids={kids} />
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
  const invalid = conditionInvalid(issues, path);
  return (
    <div data-part="rules.condition" data-kind={condition.type}>
      <SelectField
        labelId="rules-condition-kind"
        name={`${name}-kind`}
        value={condition.type}
        width="content"
        choices={choices(CONDITION_KINDS, "rules-cond-")}
        onChange={(kind) => {
          onChange(replaceAt(root, path, newCondition(kind as ConditionKind)));
        }}
      />
      <LeafFields
        name={name}
        condition={condition}
        invalid={invalid}
        describedBy={describedBy}
        onChange={(c) => {
          onChange(replaceAt(root, path, c));
        }}
      />
      <ValuesFromMessage
        condition={condition}
        onPick={(value) => {
          onChange(replaceAt(root, path, withPickedValue(condition, value)));
        }}
      />
      {remove}
    </div>
  );
}

/**
 * The conditions of a rule (the IF row): "Match all / any of:" for the
 * root group, its conditions and nested groups, "+ Add condition". A root
 * that is not an all / any group (a `not`, or a single condition from the
 * text view) is shown as it is.
 */
export function ConditionEditor({
  value,
  issues = [],
  describedBy,
  onChange,
}: {
  value: Condition;
  issues?: readonly RuleIssue[];
  describedBy?: string | undefined;
  onChange: (value: Condition) => void;
}) {
  const node: NodeProps = { root: value, path: [], issues, describedBy, onChange };
  if (value.type !== "all" && value.type !== "any") {
    return (
      <div data-part="rules.conditions">
        <ConditionNode {...node} />
      </div>
    );
  }
  return (
    <div data-part="rules.conditions">
      <GroupMode root={value} path={[]} name="c" condition={value} onChange={onChange} />
      <GroupItems {...node} kids={value.conditions} />
      <AddCondition
        path={[]}
        onAdd={(c) => {
          onChange(addChild(value, [], c));
        }}
      />
    </div>
  );
}
