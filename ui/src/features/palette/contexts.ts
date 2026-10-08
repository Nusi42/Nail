/**
 * Where the user is when the palette opens — the contexts that decide
 * which actions are available (`ActionAvailability`, ADR 0094): the
 * keyboard context last focused (`list`, `reader`, `composer`,
 * `calendar`, …), whether a message is selected (reader actions), the
 * composer, open PIM windows, and the desktop app.
 */
import type { ActionAvailability } from "../../api/generated";
import type { UiState } from "../../state/uiStore";
import { PIM_KEY_CONTEXTS } from "../shell/model";
import { ACTION_AVAILABILITIES } from "./messages";

const PIM: readonly ActionAvailability[] = PIM_KEY_CONTEXTS;

function isAvailability(value: string): value is ActionAvailability {
  return (ACTION_AVAILABILITIES as readonly string[]).includes(value);
}

/** The contexts that apply in this UI state (`global` always). */
export function contextsOf(
  state: Pick<
    UiState,
    "keyContext" | "selectedMessageId" | "messageOrder" | "composer" | "windows"
  >,
  desktop: boolean,
): Set<ActionAvailability> {
  const out = new Set<ActionAvailability>(["global"]);
  if (isAvailability(state.keyContext)) out.add(state.keyContext);
  if (state.messageOrder.length > 0) out.add("list");
  if (state.selectedMessageId !== null) out.add("reader");
  if (state.composer !== null) out.add("composer");
  for (const name of state.windows) {
    for (const pim of PIM) if (name === pim || name.startsWith(`${pim}-`)) out.add(pim);
  }
  if (desktop) out.add("desktop");
  return out;
}
