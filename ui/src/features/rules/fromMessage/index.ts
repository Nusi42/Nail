/**
 * "Values from the selected mail" (R2, ADR 0222): owned by the package WR5
 * inside the rules slot. The rules editor imports only this barrel: the
 * suggestion control beside condition values (`ValuesFromMessage`) and the
 * message's values (`useMessageFacts`, `valuesForCondition`). Which message
 * the window was opened from is the rules store's `origin`
 * (`../store.ts`), set by the entry points that open the window from a
 * message (`rules.create-from-message`). Texts:
 * `locales/{en,de}/rules-from-message.ftl`.
 */
export {
  MESSAGE_VALUE_KINDS,
  getMessageFacts,
  messageFactsKeys,
  useMessageFacts,
  valuesForCondition,
  type MessageValue,
  type MessageValueKind,
} from "./facts";
export { ValuesFromMessage, type ValuesFromMessageProps } from "./ValuesFromMessage";
