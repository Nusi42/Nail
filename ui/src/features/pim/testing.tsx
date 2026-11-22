/**
 * Test helpers of the PIM feature (only tests import this file).
 */
import type { UserEvent } from "@testing-library/user-event";
import type { Env } from "../../test/harness";
import { confirmQuestion } from "../../widgets/dialog/testing";
import type { PimConfirm } from "./Confirm";

/**
 * Answers the open PIM question `kind` (R6: a destructive confirmation
 * window) with its verb `verb`, after checking that Cancel has the focus.
 */
export function confirmPim(env: Env, user: UserEvent, kind: PimConfirm, verb: string) {
  return confirmQuestion(env, user, kind, verb);
}
