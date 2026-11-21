/**
 * "Values from the selected mail" (R2, ADR 0222): owned by the package WR5
 * inside the rules slot. The rules editor imports only this barrel: the
 * suggestion controls beside condition and action values
 * (`ValuesFromMessage`, `ActionValuesFromMessage`) and the message's values
 * (`useMessageFacts`, `valuesForCondition`, `valuesForAction`). Which
 * message the window was opened from is the rules store's `origin`
 * (`../store.ts`), set by the entry points that open the window from a
 * message (`rules.create-from-message`, "Add to rule…" of
 * `./entryPoints.ts`); without one the selected message's values are
 * offered. Texts: `locales/{en,de}/rules-from-message.ftl`.
 */
export {
  MAX_ADDRESSES_PER_FIELD,
  MAX_SUBJECT_WORDS,
  MESSAGE_VALUE_KINDS,
  MESSAGE_VALUE_SOURCES,
  MIN_SUBJECT_WORD,
  applyToAction,
  applyToCondition,
  domainOf,
  getMessageFacts,
  messageFactsKeys,
  subjectWords,
  useMessageFacts,
  valuesForAction,
  valuesForCondition,
  type MessageValue,
  type MessageValueContext,
  type MessageValueKind,
  type MessageValueSource,
} from "./facts";
export {
  ADD_TO_RULE_ACTION,
  ADD_TO_RULE_PICK_ACTION,
  addToRuleEntries,
  fromMessageActionInfo,
  fromMessageActions,
  fromMessageMenuSources,
  selectedOrigin,
} from "./entryPoints";
export {
  ActionValuesFromMessage,
  ValuesFromMessage,
  menuEntries,
  valuesMessageOf,
  type ActionValuesFromMessageProps,
  type ValuesFromMessageProps,
  type ValuesMessage,
} from "./ValuesFromMessage";
