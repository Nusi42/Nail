/**
 * The shared pickers (R2, ADR 0222): one `Picker` for every field that
 * names a folder, an account, a tag, an address or contact, a mailing list
 * or a header name, used wherever such fields exist (rules editor, virtual
 * folder editor, search field helpers, settings). Owned by the pickers
 * package (WR4); texts in `locales/{en,de}/pickers.ftl`, looks `picker.*`
 * in `defaults/theme.kdl`.
 */
export { Picker, PICKER_PARTS } from "./Picker";
export {
  PICKER_SUGGESTIONS,
  suggestKindOf,
  suggestValues,
  usePickerOptions,
  type PickerQuery,
} from "./sources";
export {
  PICKER_KINDS,
  type FolderValue,
  type PickerKind,
  type PickerOption,
  type PickerOptions,
  type PickerProps,
  type PickerScope,
} from "./types";
