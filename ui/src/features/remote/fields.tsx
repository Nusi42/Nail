/**
 * Small controls of the remote feature beyond `widgets/forms.tsx`: a text
 * input with autofocus / autocomplete / input mode (login forms), a
 * checkbox that can be disabled with a reason, and a two-step confirm
 * button. Labels are Fluent ids; styling is the theme's (`form.*` parts).
 */
import { useId, useState, type ComponentProps, type ReactNode } from "react";
import type { RemoteAccessPreset } from "../../api/generated";
import { SelectField, TextField } from "../../widgets/forms";
import { PRESETS } from "./model";
import type { MessageArgs } from "../../i18n/localizer";
import { useT } from "../../i18n/I18nProvider";

/** A text input that never spell-checks (logins, codes, names). */
export function InputField(props: Omit<ComponentProps<typeof TextField>, "spellCheck">) {
  return <TextField {...props} spellCheck={false} />;
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
          onChange={(event) => {
            onChange(event.target.checked);
          }}
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
        onClick={() => {
          setAsking(true);
        }}
      >
        {t(labelId, labelArgs)}
      </button>
    );
  }
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape from the question's buttons takes the question back.
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
        // eslint-disable-next-line jsx-a11y/no-autofocus -- the question replaced the button: keep the keyboard focus here.
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
        onClick={() => {
          setAsking(false);
        }}
      >
        {t("common-cancel")}
      </button>
    </span>
  );
}

/** The access preset of a new user or a paired device. */
export function AccessPresetField({
  name,
  preset,
  onChange,
}: {
  name: string;
  preset: RemoteAccessPreset;
  onChange: (preset: RemoteAccessPreset) => void;
}) {
  return (
    <SelectField
      labelId="remote-access"
      name={name}
      value={preset}
      onChange={(v) => {
        onChange(v as RemoteAccessPreset);
      }}
      choices={PRESETS.map((p) => ({ value: p, label: `remote-preset-${p}` }))}
    />
  );
}

/** Save and Cancel of an inline rename form. */
export function RenameActions({ disabled, onCancel }: { disabled: boolean; onCancel: () => void }) {
  const t = useT();
  return (
    <>
      <button type="submit" data-part="form.button" data-action="save-name" disabled={disabled}>
        {t("common-save")}
      </button>
      <button type="button" data-part="form.button" data-action="cancel" onClick={onCancel}>
        {t("common-cancel")}
      </button>
    </>
  );
}
