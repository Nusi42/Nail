/**
 * A labelled picker for forms (settings, the virtual account editor, the
 * rules editor): the shared `Picker` in the `form.field` frame of
 * `../forms.tsx` (label above, optional hint below).
 */
import { useId } from "react";
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
