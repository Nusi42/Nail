/**
 * The suggestion control the rules editor places next to a condition's
 * value field (R2, ADR 0222): when the rules window was opened from a
 * message, it lists that message's values for the condition (sender,
 * recipients, subject, mailing list, …) and puts the picked one into the
 * field. Renders nothing without an origin message.
 *
 * Contract stub: renders nothing; the "values from the selected mail"
 * package (WR5) builds it (button `rules-from-message-values`, parts
 * `rules.from-message`, `rules.from-message.button`,
 * `rules.from-message.list`, `rules.from-message.option`).
 */
import type { ReactNode } from "react";
import type { Condition } from "../../../api/generated";

export interface ValuesFromMessageProps {
  /** The condition whose value the picked value replaces. */
  condition: Condition;
  /** Called with the picked value. */
  onPick: (value: string) => void;
}

// Contract stub (ADR 0222): WR5 implements it.
export const ValuesFromMessage: (props: ValuesFromMessageProps) => ReactNode = () => null;
