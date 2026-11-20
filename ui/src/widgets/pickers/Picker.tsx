/**
 * The shared picker (R2, ADR 0222): a text field with a browse / search /
 * select list of the values its kind offers (a folder tree, accounts,
 * tags, addresses and contacts, mailing lists, header names). Free text
 * stays allowed.
 *
 * Contract stub: renders the plain text field; the list, the tree, the
 * keyboard model and the browse button are the pickers package's (WR4).
 *
 * Parts: `picker` (`data-kind`; the host may name another root part with
 * `part`), `picker.input`; the package adds `picker.browse`,
 * `picker.popup`, `picker.search`, `picker.list`, `picker.option`
 * (`data-depth`, `data-selected`), `picker.option.label`,
 * `picker.option.detail`, `picker.option.toggle` and `picker.empty`
 * (`PICKER_PARTS`; looks in `defaults/theme.kdl`).
 */
import type { PickerProps } from "./types";

/** Every part a picker renders (the theme styles these). */
export const PICKER_PARTS = [
  "picker",
  "picker.input",
  "picker.browse",
  "picker.popup",
  "picker.search",
  "picker.list",
  "picker.option",
  "picker.option.label",
  "picker.option.detail",
  "picker.option.toggle",
  "picker.empty",
] as const;

export function Picker({
  kind,
  value,
  onChange,
  label,
  id,
  placeholder,
  disabled,
  invalid,
  describedBy,
  part,
}: PickerProps) {
  return (
    <span data-part={part ?? "picker"} data-kind={kind}>
      <input
        data-part="picker.input"
        type="text"
        id={id}
        value={value}
        aria-label={label}
        placeholder={placeholder}
        disabled={disabled}
        aria-invalid={invalid === true ? true : undefined}
        aria-describedby={describedBy}
        autoComplete="off"
        onChange={(event) => {
          onChange(event.target.value, null);
        }}
      />
    </span>
  );
}
