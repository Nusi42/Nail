/**
 * Small controls of the remote feature beyond `widgets/forms.tsx`: a text
 * input with autofocus / autocomplete / input mode (login forms), a
 * checkbox that can be disabled with a reason, and a two-step confirm
 * button. Labels are Fluent ids; styling is the theme's (`form.*` parts).
 */
import { useId, useState, type ReactNode } from "react";
import type { MessageArgs } from "../../i18n/localizer";
import { useT } from "../../i18n/I18nProvider";

export function InputField({
  labelId,
  args,
  hintId,
  hint,
  name,
  value,
  onChange,
  type = "text",
  autoComplete = "off",
  inputMode,
  autoFocus = false,
  disabled = false,
  readOnly = false,
  invalid = false,
  maxLength,
}: {
  labelId: string;
  args?: MessageArgs;
  hintId?: string;
  /** Hint text already translated (wins over `hintId`). */
  hint?: string;
  name: string;
  value: string;
  onChange: (value: string) => void;
  type?: "text" | "password";
  autoComplete?: string;
  inputMode?: "text" | "numeric";
  autoFocus?: boolean;
  disabled?: boolean;
  readOnly?: boolean;
  invalid?: boolean;
  maxLength?: number;
}) {
  const t = useT();
  const id = useId();
  const hintText = hint ?? (hintId ? t(hintId) : null);
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {t(labelId, args)}
      </label>
      <input
        id={id}
        data-part="form.input"
        type={type}
        name={name}
        value={value}
        autoComplete={autoComplete}
        inputMode={inputMode}
        // Each login step moves the focus to its first field.
        autoFocus={autoFocus}
        disabled={disabled}
        readOnly={readOnly}
        maxLength={maxLength}
        spellCheck={false}
        aria-invalid={invalid || undefined}
        aria-describedby={hintText ? `${id}-hint` : undefined}
        onChange={(event) => onChange(event.target.value)}
      />
      {hintText && (
        <span data-part="form.hint" id={`${id}-hint`}>
          {hintText}
        </span>
      )}
    </div>
  );
}

/** A checkbox; when `lockedReason` is set it is disabled and says why. */
export function Toggle({
  labelId,
  args,
  name,
  checked,
  onChange,
  lockedReason,
}: {
  labelId: string;
  args?: MessageArgs;
  name: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  lockedReason?: string | null;
}) {
  const t = useT();
  const id = useId();
  const locked = lockedReason !== undefined && lockedReason !== null;
  return (
    <div data-part="form.field" data-field={name} data-state={locked ? "locked" : undefined}>
      <label data-part="form.check">
        <input
          data-part="form.checkbox"
          type="checkbox"
          name={name}
          checked={checked}
          disabled={locked}
          aria-describedby={locked ? `${id}-reason` : undefined}
          onChange={(event) => onChange(event.target.checked)}
        />
        <span data-part="form.check.label">{t(labelId, args)}</span>
      </label>
      {locked && (
        <span data-part="form.hint" data-state="locked" id={`${id}-reason`}>
          {lockedReason}
        </span>
      )}
    </div>
  );
}

/**
 * A button that asks before doing something irreversible: the first press
 * shows the question with "yes" and "cancel"; Escape cancels.
 */
export function ConfirmButton({
  labelId,
  labelArgs,
  questionId,
  questionArgs,
  yesId,
  action,
  onConfirm,
  variant = "danger",
  disabled = false,
}: {
  labelId: string;
  labelArgs?: MessageArgs;
  questionId: string;
  questionArgs?: MessageArgs;
  yesId: string;
  action: string;
  onConfirm: () => unknown;
  variant?: "danger" | "primary";
  disabled?: boolean;
}): ReactNode {
  const t = useT();
  const [asking, setAsking] = useState(false);
  if (!asking) {
    return (
      <button
        type="button"
        data-part="form.button"
        data-action={action}
        data-variant={variant}
        disabled={disabled}
        onClick={() => setAsking(true)}
      >
        {t(labelId, labelArgs)}
      </button>
    );
  }
  return (
    <span
      data-part="remote-confirm"
      role="group"
      aria-label={t(questionId, questionArgs)}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.stopPropagation();
          setAsking(false);
        }
      }}
    >
      <span data-part="remote-confirm.text">{t(questionId, questionArgs)}</span>
      <button
        type="button"
        data-part="form.button"
        data-action={action}
        data-variant={variant}
        // The question replaced the button: keep the keyboard focus here.
        autoFocus
        onClick={() => {
          setAsking(false);
          void onConfirm();
        }}
      >
        {t(yesId)}
      </button>
      <button
        type="button"
        data-part="form.button"
        data-action="cancel"
        onClick={() => setAsking(false)}
      >
        {t("remote-cancel")}
      </button>
    </span>
  );
}
