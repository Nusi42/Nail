/**
 * The shared labelled picker (`widgets/pickers`, ADR 0222) as the rule
 * editor uses it: folders are named by path, the placeholder is a Fluent id
 * (what an empty value means), and a problem marks the field (`invalid`)
 * and names the footer's problem badge (`describedBy`).
 */
import { useT } from "../../i18n/I18nProvider";
import {
  PickerField as SharedPickerField,
  type PickerFieldProps as SharedPickerFieldProps,
} from "../../widgets/pickers";

export interface PickerFieldProps extends Omit<
  SharedPickerFieldProps,
  "placeholder" | "folderValue" | "onChange"
> {
  onChange: (value: string) => void;
  /** Fluent id of the placeholder (what an empty value means). */
  placeholderId?: string | undefined;
}

export function PickerField({
  placeholderId,
  invalid,
  describedBy,
  onChange,
  ...field
}: PickerFieldProps) {
  const t = useT();
  return (
    <SharedPickerField
      {...field}
      folderValue="path"
      placeholder={placeholderId === undefined ? undefined : t(placeholderId)}
      invalid={invalid}
      describedBy={invalid === true ? describedBy : undefined}
      onChange={(next) => {
        onChange(next);
      }}
    />
  );
}
