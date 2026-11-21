/**
 * A labelled picker for forms (settings, the virtual account editor): the
 * shared `Picker` in the `form.field` frame of `../forms.tsx` (label above,
 * optional hint below).
 */
import { useId } from "react";
import { useT } from "../../i18n/I18nProvider";
import type { MessageArgs } from "../../i18n/localizer";
import { LabelledField } from "../forms";
import { Picker } from "./Picker";
import type { PickerProps } from "./types";

export interface PickerFieldProps extends Omit<PickerProps, "label" | "id" | "describedBy"> {
  /** Fluent id of the label. */
  labelId: string;
  /** Arguments of the label message. */
  args?: MessageArgs;
  /** Fluent id of a hint under the field. */
  hintId?: string;
  /** The field's name (`data-field` of the frame). */
  name: string;
}

export function PickerField({ labelId, args, hintId, name, ...picker }: PickerFieldProps) {
  const t = useT();
  const id = useId();
  const label = t(labelId, args);
  return (
    <LabelledField name={name} id={id} label={label} hint={hintId ? t(hintId) : null}>
      <Picker {...picker} id={id} label={label} describedBy={hintId ? `${id}-hint` : undefined} />
    </LabelledField>
  );
}
