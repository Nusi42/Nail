/**
 * The rule editor's compact form (R2 design review, items 8 and 9): the
 * header (the rule's name as a heading field, On / Off beside it) and the
 * rows WHEN (the triggers, inline, with their numbers), WHERE (the account
 * it applies to), IF (the conditions) and THEN (the actions).
 *
 * Parts: `rules.header`, `rules.header.name`, `rules.form`,
 * `rules.form.section` (`data-section`), `rules.form.label`,
 * `rules.form.body`, `rules.triggers`, `rules.trigger`.
 */
import { useId, type ReactNode } from "react";
import type { Rule, RuleTrigger } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { CheckField, NumberField } from "../../widgets/forms";
import { Switch } from "../../widgets/primitives";
import { ActionEditor } from "./ActionEditor";
import { ConditionEditor } from "./ConditionEditor";
import { TRIGGER_KINDS, toggleTrigger, type RuleIssue, type TriggerKind } from "./model";
import { PickerField } from "./PickerField";

interface Editing {
  rule: Rule;
  onChange: (rule: Rule) => void;
}

/** The name as the editor's heading, with On / Off; `children` is the view switch. */
export function RuleHeader({
  rule,
  onChange,
  nameInvalid,
  describedBy,
  children,
}: Editing & {
  nameInvalid: boolean;
  describedBy: string;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <div data-part="rules.header">
      <input
        data-part="rules.header.name"
        name="rule-name"
        type="text"
        autoComplete="off"
        value={rule.name}
        placeholder={t("rules-name-placeholder")}
        aria-label={t("rules-name")}
        aria-invalid={nameInvalid || undefined}
        aria-describedby={nameInvalid ? describedBy : undefined}
        onChange={(event) => {
          onChange({ ...rule, name: event.target.value });
        }}
      />
      <Switch
        name="rule-enabled"
        label={t("rules-enabled")}
        checked={rule.enabled}
        onChange={(enabled) => {
          onChange({ ...rule, enabled });
        }}
      />
      {children}
    </div>
  );
}

/** One labelled row of the form. */
function FormRow({
  section,
  labelId,
  children,
}: {
  section: string;
  labelId: string;
  children: ReactNode;
}) {
  const t = useT();
  const id = useId();
  return (
    <div data-part="rules.form.section" data-section={section} role="group" aria-labelledby={id}>
      <span data-part="rules.form.label" id={id}>
        {t(labelId)}
      </span>
      <div data-part="rules.form.body">{children}</div>
    </div>
  );
}

/** WHEN: each trigger a check box, a schedule's or an age's number right beside it. */
function Triggers({ rule, onChange }: Editing) {
  const has = (kind: TriggerKind) => rule.triggers.find((t) => t.type === kind);
  const setTrigger = (next: RuleTrigger) => {
    onChange({ ...rule, triggers: rule.triggers.map((t) => (t.type === next.type ? next : t)) });
  };
  const schedule = has("schedule");
  const older = has("older-than");
  return (
    <div data-part="rules.triggers">
      {TRIGGER_KINDS.map((kind) => (
        <span key={kind} data-part="rules.trigger" data-kind={kind}>
          <CheckField
            labelId={`rules-trigger-${kind}`}
            name={`trigger-${kind}`}
            checked={has(kind) !== undefined}
            onChange={(on) => {
              onChange(toggleTrigger(rule, kind, on));
            }}
          />
          {kind === "schedule" && schedule?.type === "schedule" && (
            <NumberField
              labelId="rules-schedule-minutes"
              name="schedule-minutes"
              min={1}
              value={schedule.interval_minutes}
              onChange={(n) => {
                setTrigger({ type: "schedule", interval_minutes: n ?? 0 });
              }}
            />
          )}
          {kind === "older-than" && older?.type === "older-than" && (
            <NumberField
              labelId="rules-older-than-days"
              name="older-than-days"
              min={1}
              value={older.days}
              onChange={(n) => {
                setTrigger({ type: "older-than", days: n ?? 0 });
              }}
            />
          )}
        </span>
      ))}
    </div>
  );
}

/** WHERE: every account, or one (an account picker); a custom scope is edited as text. */
function Scope({ rule, onChange }: Editing) {
  const t = useT();
  const [only] = rule.scope;
  const simple = rule.scope.length === 0 || (rule.scope.length === 1 && only?.type === "account");
  if (!simple) return <p data-part="rules.hint">{t("rules-scope-custom")}</p>;
  return (
    <PickerField
      kind="account"
      labelId="rules-scope"
      name="scope"
      value={only?.type === "account" ? only.account_id : ""}
      placeholderId="rules-scope-everywhere"
      onChange={(id) => {
        onChange({ ...rule, scope: id === "" ? [] : [{ type: "account", account_id: id }] });
      }}
    />
  );
}

/** The rows WHEN, WHERE, IF and THEN. */
export function RuleForm({
  rule,
  onChange,
  issues,
  describedBy,
}: Editing & { issues: readonly RuleIssue[]; describedBy: string }) {
  return (
    <div data-part="rules.form">
      <FormRow section="when" labelId="rules-form-when">
        <Triggers rule={rule} onChange={onChange} />
      </FormRow>
      <FormRow section="where" labelId="rules-form-where">
        <Scope rule={rule} onChange={onChange} />
      </FormRow>
      <FormRow section="if" labelId="rules-form-if">
        <ConditionEditor
          value={rule.conditions}
          issues={issues}
          describedBy={describedBy}
          onChange={(conditions) => {
            onChange({ ...rule, conditions });
          }}
        />
      </FormRow>
      <FormRow section="then" labelId="rules-form-then">
        <ActionEditor
          value={rule.actions}
          issues={issues}
          describedBy={describedBy}
          onChange={(actions) => {
            onChange({ ...rule, actions });
          }}
        />
      </FormRow>
    </div>
  );
}
