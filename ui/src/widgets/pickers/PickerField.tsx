/**
 * A labelled picker for forms (settings, the virtual account editor, the
 * rules editor): the shared `Picker` in the `form.field` frame of
 * `../forms.tsx` (label above, optional hint below). `PickerListField` is
 * its multi-value form (R3): chips plus the input, the typed text kept here.
 * `NullablePickerField` is the form whose empty text means "none" (`null`),
 * and `AccountChoiceField` the strict account field a setting is for.
 */
import { useId, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import type { MessageArgs } from "../../i18n/localizer";
import { LabelledField } from "../forms";
import { Picker } from "./Picker";
import type { AccountId } from "../../api/generated";
import type { PickerOption, PickerProps } from "./types";

export interface PickerFieldProps extends Omit<PickerProps, "label" | "id"> {
  /** Fluent id of the label. */
  labelId: string;
  /** Arguments of the label message. */
  args?: MessageArgs;
  /** Fluent id of a hint under the field. */
  hintId?: string | undefined;
  /** The field's name (`data-field` of the frame). */
  name: string;
}

export function PickerField({
  labelId,
  args,
  hintId,
  name,
  describedBy,
  ...picker
}: PickerFieldProps) {
  const t = useT();
  const id = useId();
  const label = t(labelId, args);
  const described = [hintId ? `${id}-hint` : undefined, describedBy].filter(Boolean).join(" ");
  return (
    <LabelledField name={name} id={id} label={label} hint={hintId ? t(hintId) : null}>
      <Picker
        {...picker}
        id={id}
        label={label}
        describedBy={described === "" ? undefined : described}
      />
    </LabelledField>
  );
}

export interface PickerListFieldProps extends Omit<
  PickerFieldProps,
  "value" | "onChange" | "multiple" | "strict"
> {
  /** The values chosen so far. */
  values: readonly string[];
  /** Called with the new values when one is added or removed. */
  onChange: (values: string[]) => void;
}

/** A labelled multi-value picker: several addresses, accounts or folders. */
export function PickerListField({ values, onChange, ...field }: PickerListFieldProps) {
  const [text, setText] = useState("");
  return (
    <PickerField
      {...field}
      value={text}
      onChange={(next) => {
        setText(next);
      }}
      multiple={{ values, onChange }}
    />
  );
}

export interface NullablePickerFieldProps extends Omit<PickerFieldProps, "value" | "onChange"> {
  /** The value, `null` for none (the field shows an empty text). */
  value: string | null;
  /** Called with the new value; an empty text becomes `null`. */
  onChange: (value: string | null, option: PickerOption | null) => void;
}

/** A labelled picker whose empty text means "none" (`null`): an optional id or address. */
export function NullablePickerField({ value, onChange, ...field }: NullablePickerFieldProps) {
  return (
    <PickerField
      {...field}
      value={value ?? ""}
      onChange={(next, option) => {
        onChange(next === "" ? null : next, option);
      }}
    />
  );
}

export interface AccountChoiceFieldProps extends Omit<
  NullablePickerFieldProps,
  "kind" | "strict" | "accounts" | "onChange"
> {
  /** Offer only these accounts (default: every real account). */
  accounts?: readonly { id: AccountId }[] | undefined;
  /** Called with the chosen account's id, `null` when the field is emptied. */
  onChange: (id: AccountId | null) => void;
}

/** The account a setting is for: only an offered account, or none (`null`). */
export function AccountChoiceField({ accounts, onChange, ...field }: AccountChoiceFieldProps) {
  return (
    <NullablePickerField
      {...field}
      kind="account"
      strict
      accounts={accounts?.map((a) => a.id)}
      onChange={(id) => {
        onChange(id);
      }}
    />
  );
}
