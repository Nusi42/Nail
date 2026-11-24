/**
 * Test helpers of the PIM feature (only tests import this file).
 */
import type { UserEvent } from "@testing-library/user-event";
import { waitFor } from "@testing-library/react";
import { expect } from "vitest";
import type { CalendarEvent, EventInstance, Request, Response } from "../../api/generated";
import { must } from "../../test/must";
import type { MockTransport } from "../../api/mock";
import { renderWithProviders, type Env } from "../../test/harness";
import { EventEditorDialog } from "./EventEditor";
import { EVENT_DIALOG } from "./names";
import { pimStoreFor } from "./store";
import { localZone } from "./time";
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

/** Waits until the mock core holds an event titled `title`; answers it. */
export async function eventTitled(env: Env, title: string): Promise<CalendarEvent> {
  const find = () => env.transport.data.features.pim.events.find((e) => e.fields.title === title);
  await waitFor(() => {
    expect(find()).toBeDefined();
  });
  return must(find());
}

/** Opens the event editor on the whole event `id` of the mock core (rendered on its own). */
export function openEventEditor(env: Env, id: string, close: () => void = () => undefined) {
  const event = must(env.transport.data.features.pim.events.find((e) => e.id === id));
  pimStoreFor(env.store).getState().editEvent({
    mode: "edit",
    eventId: event.id,
    recurrenceId: null,
    start: event.fields.start,
    end: event.fields.end,
  });
  return renderWithProviders(<EventEditorDialog name={EVENT_DIALOG} close={close} />, env);
}

/** The occurrences of event `id` the mock core expands (a month back to a year on). */
export async function instancesOf(env: Env, id: string): Promise<EventInstance[]> {
  const from = new Date();
  from.setMonth(from.getMonth() - 1);
  const to = new Date();
  to.setFullYear(to.getFullYear() + 1);
  const answer = await env.client.request(
    {
      type: "ListEventInstances",
      from: from.toISOString(),
      to: to.toISOString(),
      calendar_ids: null,
      tz: localZone(),
    },
    "EventInstances",
  );
  return answer.instances.filter((i) => i.event_id === id);
}
