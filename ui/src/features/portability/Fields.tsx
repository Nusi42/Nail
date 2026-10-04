/**
 * Small form pieces the wizards share: a group of checkboxes, a group of
 * radio buttons, one checkbox, the wizard's step header and its button row.
 * Everything is `form.*` parts (themed like every other form).
 */
import { useId, type ReactNode } from "react";
import { useT } from "../../i18n/I18nProvider";

export interface Choice<T extends string> {
  value: T;
  /** Display text (already translated or user data). */
  label: string;
  /** Extra text under the label. */
  hint?: string;
  disabled?: boolean;
}

/** A fieldset of checkboxes. */
export function CheckGroup<T extends string>({
  legend,
  field,
  choices,
  selected,
  onChange,
}: {
  legend: string;
  field: string;
  choices: readonly Choice<T>[];
  selected: readonly T[];
  onChange: (next: T[]) => void;
}) {
  const id = useId();
  return (
    <fieldset data-part="form.section" data-field={field}>
      <legend data-part="form.section.title">{legend}</legend>
      <ul data-part="portability.choices">
        {choices.map((choice, index) => {
          const hintId = choice.hint ? `${id}-${String(index)}-hint` : undefined;
          return (
            <li key={choice.value} data-part="portability.choice">
              <label data-part="form.check" data-value={choice.value}>
                <input
                  data-part="form.checkbox"
                  type="checkbox"
                  checked={selected.includes(choice.value)}
                  disabled={choice.disabled}
                  aria-describedby={hintId}
                  onChange={(e) => {
                    onChange(
                      e.target.checked
                        ? [...selected, choice.value]
                        : selected.filter((v) => v !== choice.value),
                    );
                  }}
                />
                <span data-part="form.check.label">{choice.label}</span>
              </label>
              {choice.hint && (
                <p data-part="form.hint" id={hintId}>
                  {choice.hint}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </fieldset>
  );
}

/** A fieldset of radio buttons. */
export function RadioGroup<T extends string>({
  legend,
  field,
  choices,
  value,
  onChange,
}: {
  legend: string;
  field: string;
  choices: readonly Choice<T>[];
  value: T;
  onChange: (next: T) => void;
}) {
  const name = useId();
  return (
    <fieldset data-part="form.section" data-field={field}>
      <legend data-part="form.section.title">{legend}</legend>
      <ul data-part="portability.choices">
        {choices.map((choice, index) => {
          const hintId = choice.hint ? `${name}-${String(index)}-hint` : undefined;
          return (
            <li key={choice.value} data-part="portability.choice">
              <label data-part="form.check" data-value={choice.value}>
                <input
                  data-part="form.radio"
                  type="radio"
                  name={name}
                  checked={value === choice.value}
                  disabled={choice.disabled}
                  aria-describedby={hintId}
                  onChange={() => {
                    onChange(choice.value);
                  }}
                />
                <span data-part="form.check.label">{choice.label}</span>
              </label>
              {choice.hint && (
                <p data-part="form.hint" id={hintId}>
                  {choice.hint}
                </p>
              )}
            </li>
          );
        })}
      </ul>
    </fieldset>
  );
}

/** One checkbox with an optional hint. */
export function Check({
  label,
  field,
  checked,
  onChange,
  hint,
  disabled,
}: {
  label: string;
  field: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  hint?: string;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div data-part="form.field" data-field={field}>
      <label data-part="form.check" data-field={field}>
        <input
          data-part="form.checkbox"
          type="checkbox"
          checked={checked}
          disabled={disabled}
          aria-describedby={hint ? id : undefined}
          onChange={(e) => {
            onChange(e.target.checked);
          }}
        />
        <span data-part="form.check.label">{label}</span>
      </label>
      {hint && (
        <p data-part="form.hint" id={id}>
          {hint}
        </p>
      )}
    </div>
  );
}

/** A labelled `<select>`. */
export function SelectField({
  label,
  field,
  value,
  onChange,
  children,
  disabled,
}: {
  label: string;
  field: string;
  value: string;
  onChange: (value: string) => void;
  children: ReactNode;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div data-part="form.field" data-field={field}>
      <label data-part="form.label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        data-part="form.select"
        data-field={field}
        value={value}
        disabled={disabled}
        onChange={(e) => {
          onChange(e.target.value);
        }}
      >
        {children}
      </select>
    </div>
  );
}

/** A labelled text input. */
export function TextField({
  label,
  field,
  value,
  onChange,
  type = "text",
  autoComplete = "off",
  hint,
  invalid,
}: {
  label: string;
  field: string;
  value: string;
  onChange: (value: string) => void;
  type?: "text" | "password";
  autoComplete?: string;
  hint?: string;
  invalid?: boolean;
}) {
  const id = useId();
  return (
    <div data-part="form.field" data-field={field}>
      <label data-part="form.label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        data-part="form.input"
        data-field={field}
        type={type}
        autoComplete={autoComplete}
        spellCheck={false}
        value={value}
        aria-invalid={invalid ? true : undefined}
        aria-describedby={hint ? `${id}-hint` : undefined}
        onChange={(e) => {
          onChange(e.target.value);
        }}
      />
      {hint && (
        <p data-part="form.hint" id={`${id}-hint`} data-tone={invalid ? "danger" : undefined}>
          {hint}
        </p>
      )}
    </div>
  );
}

/** The wizard's step indicator ("Step 2 of 3: Choose what to import"). */
export function StepHeader({
  step,
  steps,
  titleId,
}: {
  step: number;
  steps: number;
  titleId: string;
}) {
  const t = useT();
  return (
    <header data-part="wizard.header">
      <p data-part="wizard.step">{t("portability-step", { step, steps })}</p>
      <h3 data-part="wizard.title">{t(titleId)}</h3>
    </header>
  );
}

/** The row of wizard buttons. */
export function WizardButtons({ children }: { children: ReactNode }) {
  return <div data-part="wizard.buttons">{children}</div>;
}
