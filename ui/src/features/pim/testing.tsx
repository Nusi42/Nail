/**
 * Test helpers of the PIM feature (only tests import this file).
 */
import type { UserEvent } from "@testing-library/user-event";
import { expect } from "vitest";
import type { Request, Response } from "../../api/generated";
import type { MockTransport } from "../../api/mock";
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

/** A response of the type `T`. */
export type ResponseOf<T extends Response["type"]> = Extract<Response, { type: T }>;

/** Sends `request` to the mock core and expects an answer of `type`. */
export async function mockOk<T extends Response["type"]>(
  t: MockTransport,
  request: Request,
  type: T,
): Promise<ResponseOf<T>> {
  const response = await t.send(request);
  expect(response.type, JSON.stringify(response)).toBe(type);
  return response as ResponseOf<T>;
}
