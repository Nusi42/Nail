/**
 * A labelled shared picker (`widgets/pickers`, ADR 0222) in the rule
 * editor: every field that names a folder, an account, a tag, an address,
 * a mailing list or a header name browses, searches and picks its value;
 * free text stays allowed. The label is a Fluent id like the other editor
 * fields (`widgets/forms`); a problem marks the field (`invalid`) and names
 * the footer's problem badge (`describedBy`).
 */
import { useId } from "react";
import type { AccountId } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { LabelledField } from "../../widgets/forms";
import { Picker, type PickerKind } from "../../widgets/pickers";

export interface PickerFieldProps {
  kind: PickerKind;
  /** Fluent id of the label. */
  labelId: string;
  /** `data-field` of the row (tests, theme rules). */
  name: string;
  value: string;
  onChange: (value: string) => void;
  /** Fluent id of the placeholder (what an empty value means). */
  placeholderId?: string | undefined;
  /** Folder pickers: the account whose folders are offered (`null`: every account). */
  accountId?: AccountId | null | undefined;
  invalid?: boolean | undefined;
  describedBy?: string | undefined;
}

export function PickerField({
  kind,
  labelId,
  name,
  value,
  onChange,
  placeholderId,
  accountId,
  invalid,
  describedBy,
}: PickerFieldProps) {
  const t = useT();
  const id = useId();
  const label = t(labelId);
  return (
    <LabelledField name={name} id={id} label={label}>
      <Picker
        kind={kind}
        id={id}
        label={label}
        value={value}
        placeholder={placeholderId === undefined ? undefined : t(placeholderId)}
        accountId={accountId}
        folderValue="path"
        invalid={invalid}
        describedBy={invalid === true ? describedBy : undefined}
        onChange={(next) => {
          onChange(next);
        }}
      />
    </LabelledField>
  );
}
