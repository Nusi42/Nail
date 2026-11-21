/**
 * A labelled picker for forms (settings, the virtual account editor, the
 * rules editor): the shared `Picker` in the `form.field` frame of
 * `../forms.tsx` (label above, optional hint below). `PickerListField` is
 * its multi-value form (R3): chips plus the input, the typed text kept here.
 */
import { useId, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import type { MessageArgs } from "../../i18n/localizer";
import { LabelledField } from "../forms";
import { Picker } from "./Picker";
import type { PickerProps } from "./types";

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
