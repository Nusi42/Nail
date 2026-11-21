/**
 * The shared pickers (R2, ADR 0222): one `Picker` for every field that
 * names a folder, an account, a tag, an address or contact, a mailing list
 * or a header name, used wherever such fields exist (rules editor, virtual
 * account editor, the search field's values, settings). `PickerField` is
 * the labelled form variant; `SearchValues` the search field's helper.
 * Texts in `locales/{en,de}/pickers.ftl`, looks `picker.*` in
 * `defaults/theme.kdl`.
 */
export {
  Picker,
  PICKER_PARTS,
  POPUP_ROOM,
  optionNamed,
  popupPlacement,
  showsLabel,
} from "./Picker";
export { PickerField, type PickerFieldProps } from "./PickerField";
export {
  SEARCH_VALUE_FIELDS,
  SEARCH_VALUES_SHOWN,
  SearchValues,
  applySearchValue,
  useSearchValues,
  searchValueContext,
  searchValueOf,
  searchValueText,
  type SearchValueContext,
  type SearchValuesFound,
  type SearchValuesProps,
} from "./searchValues";
export {
  COMMON_HEADER_NAMES,
  PICKER_ADDRESSES,
  PICKER_SUGGESTIONS,
  accountOptions,
  addressOptions,
  folderTreeOptions,
  optionKey,
  suggestKindOf,
  suggestValues,
  usePickerOptions,
  valueOptions,
  type FolderTreeInput,
  type PickerQuery,
} from "./sources";
export { initialActive, moveActive, parentKeys, pickable, toggled, visibleRows } from "./tree";
export {
  PICKER_KINDS,
  type FolderValue,
  type PickerKind,
  type PickerOption,
  type PickerOptions,
  type PickerProps,
  type PickerScope,
} from "./types";
