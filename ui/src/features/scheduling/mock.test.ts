import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Invitation, Request, Response } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import type { FeatureEventContext } from "../types";
import { SCHEDULING_EVENTS, handleSchedulingEvent, schedulingKeys } from ".";
import { runtimeIds } from "./messages";
import { SCHEDULING_REQUESTS, actionsFor, decorateInvitation, isSchedulingRequest } from "./mock";
import { DEMO_INVITATIONS, seedSchedulingDemo, invitation } from "./mockSeed";

const TZ = "Europe/Berlin";

function seeded(): MockTransport {
  const t = new MockTransport();
  seedSchedulingDemo(t.data, { tz: TZ });
  return t;
}

async function card(t: MockTransport, id: string): Promise<Invitation> {
  const r = await t.send({ type: "GetInvitation", message_id: id });
  if (r.type !== "Invitation" || !r.invitation) throw new Error(JSON.stringify(r));
  return r.invitation;
}

function scheduled(r: Response) {
  if (r.type !== "Scheduled") throw new Error(JSON.stringify(r));
  return r.result;
}

describe("scheduling mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(SCHEDULING_REQUESTS).size).toBe(SCHEDULING_REQUESTS.length);
    expect(SCHEDULING_REQUESTS).toHaveLength(12);
    expect(isSchedulingRequest({ type: "GetSchedulingSettings" })).toBe(true);
    expect(isSchedulingRequest({ type: "ListGmailLabels", account_id: "gm" })).toBe(false);
  });

  it("answers unsupported like the core before WP85 when switched to", async () => {
    const t = new MockTransport();
    expect(t.data.features.scheduling.invitations).toEqual({});
    expect(t.data.features.scheduling.settings.add_invitations).toBe(true);
    t.data.features.scheduling.unsupported = true;
    const requests: Request[] = [
      { type: "GetInvitation", message_id: "m1" },
      {
        type: "QueryFreeBusy",
        attendees: ["bob@example.org"],
        from: "2026-10-05T00:00:00Z",
        to: "2026-10-06T00:00:00Z",
        identity_id: null,
        ignore_event_id: null,
      },
    ];
    for (const request of requests) {
      expect(await t.send(request)).toMatchObject({ type: "Error", code: "unsupported" });
    }
  });

  it("refetches scheduling queries on scheduling and calendar events only", () => {
    const queryClient = new QueryClient();
    const invalidate = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    handleSchedulingEvent({ type: "InvitationsChanged", message_ids: ["m1"] }, ctx);
    expect(invalidate).toHaveBeenCalledWith({ queryKey: schedulingKeys.all });
    invalidate.mockClear();
    handleSchedulingEvent({ type: "AccountsChanged" }, ctx);
    expect(invalidate).not.toHaveBeenCalled();
    expect(SCHEDULING_EVENTS).toContain("EventsChanged");
  });

  it("lists the core's issue texts", () => {
    expect(runtimeIds()).toContain("itip-issue-sender-not-organizer");
    expect(runtimeIds()).toContain("freebusy-issue-range-too-long");
  });

  it("answers GetInvitation: null for plain mail, not_found for unknown messages", async () => {
    const t = seeded();
    const plain = t.data.messages[0]?.id ?? "";
    expect(await t.send({ type: "GetInvitation", message_id: plain })).toEqual({
      type: "Invitation",
      invitation: null,
    });
    expect(await t.send({ type: "GetInvitation", message_id: "nope" })).toMatchObject({
      type: "Error",
      code: "not_found",
    });
  });

  it("seeds a new invitation that conflicts with the design review", async () => {
    const t = seeded();
    const inv = await card(t, DEMO_INVITATIONS.newRequest);
    expect(inv.state).toBe("new");
    expect(inv.actions).toEqual(
      expect.arrayContaining(["accept", "tentative", "decline", "propose-new-time"]),
    );
    expect(inv.conflicts.map((c) => c.title)).toContain("Design review");
    expect(t.data.messages.some((m) => m.id === DEMO_INVITATIONS.newRequest)).toBe(true);
  });

  it("accepting writes the event, records the reply and updates the card", async () => {
    const t = seeded();
    const events: string[] = [];
    t.onEvent((e) => events.push(e.type));
    const before = t.data.features.pim.events.length;
    const result = scheduled(
      await t.send({
        type: "RespondToInvitation",
        target: { type: "message", message_id: DEMO_INVITATIONS.newRequest },
        response: "accept",
        comment: "See you there",
        send: true,
        calendar_id: null,
      }),
    );
    expect(t.data.features.pim.events.length).toBe(before + 1);
    expect(result.event?.fields.title).toBe("Architecture sync");
    expect(result.event?.fields.attendees.find((a) => a.email === "ada@work.example")?.status).toBe(
      "accepted",
    );
    expect(result.sent).toEqual([
      expect.objectContaining({ method: "reply", recipients: ["grace@navy.example"] }),
    ]);
    expect(result.sent[0]?.outbox_id).not.toBeNull();
    expect(result.invitation).toMatchObject({ state: "current", my_status: "accepted" });
    expect(result.invitation?.actions).toContain("open-event");
    expect(t.data.features.scheduling.comments).toEqual(["See you there"]);
    expect(events).toContain("InvitationsChanged");
  });

  it("declining without sending writes no event and sends nothing", async () => {
    const t = seeded();
    const before = t.data.features.pim.events.length;
    const result = scheduled(
      await t.send({
        type: "RespondToInvitation",
        target: { type: "message", message_id: DEMO_INVITATIONS.newRequest },
        response: "decline",
        comment: null,
        send: false,
        calendar_id: null,
      }),
    );
    expect(result.event).toBeNull();
    expect(result.sent).toEqual([]);
    expect(t.data.features.pim.events.length).toBe(before);
  });

  it("answers one occurrence of a series through the event target", async () => {
    const t = seeded();
    const inv = await card(t, DEMO_INVITATIONS.series);
    expect(inv.recurrence).not.toBeNull();
    expect(inv.event_id).not.toBeNull();
    const eventId = inv.event_id ?? "";
    const result = scheduled(
      await t.send({
        type: "RespondToInvitation",
        target: { type: "event", event_id: eventId, recurrence_id: inv.fields?.start ?? null },
        response: "tentative",
        comment: null,
        send: true,
        calendar_id: null,
      }),
    );
    expect(result.event?.overrides.length).toBe(1);
    expect(result.sent[0]?.method).toBe("reply");
  });

  it("proposes a new time and refuses an end before the start", async () => {
    const t = seeded();
    const inv = await card(t, DEMO_INVITATIONS.newRequest);
    const target = { type: "message" as const, message_id: inv.message_id };
    const start = inv.fields?.start;
    const end = inv.fields?.end;
    if (!start || !end) throw new Error("no fields");
    expect(
      await t.send({ type: "ProposeNewTime", target, start: end, end: start, comment: null }),
    ).toMatchObject({ type: "Error", localized: { id: "pim-issue-end-before-start" } });
    const result = scheduled(
      await t.send({ type: "ProposeNewTime", target, start, end, comment: "Later?" }),
    );
    expect(result.sent[0]?.method).toBe("counter");
  });

  it("forwards, refreshes and validates forward recipients", async () => {
    const t = seeded();
    const target = { type: "message" as const, message_id: DEMO_INVITATIONS.newRequest };
    expect(
      await t.send({ type: "ForwardInvitation", target, to: [], comment: null }),
    ).toMatchObject({ type: "Error", code: "invalid_request" });
    const forwarded = scheduled(
      await t.send({
        type: "ForwardInvitation",
        target,
        to: [{ name: null, email: "edsger@tue.example" }],
        comment: null,
      }),
    );
    expect(forwarded.sent[0]).toMatchObject({
      method: "request",
      recipients: ["edsger@tue.example"],
    });
    const refreshed = scheduled(await t.send({ type: "RefreshInvitation", target }));
    expect(refreshed.sent[0]?.method).toBe("refresh");
  });

  it("applies an update, a cancellation and a reply", async () => {
    const t = seeded();
    const update = scheduled(
      await t.send({
        type: "ApplyInvitation",
        message_id: DEMO_INVITATIONS.update,
        calendar_id: null,
      }),
    );
    expect(update.event?.fields.location).toBe("Room 5");
    expect(update.invitation?.state).toBe("current");

    const cancel = await card(t, DEMO_INVITATIONS.cancel);
    expect(cancel.actions).toContain("remove-from-calendar");
    const removed = scheduled(
      await t.send({ type: "ApplyInvitation", message_id: cancel.message_id, calendar_id: null }),
    );
    expect(removed.removed_event_id).toBe(cancel.event_id);
    expect(t.data.features.pim.events.some((e) => e.id === cancel.event_id)).toBe(false);
    expect(removed.invitation?.actions).not.toContain("remove-from-calendar");

    const reply = scheduled(
      await t.send({
        type: "ApplyInvitation",
        message_id: DEMO_INVITATIONS.reply,
        calendar_id: null,
      }),
    );
    expect(
      reply.event?.fields.attendees.find((a) => a.email === "katherine@nasa.example")?.status,
    ).toBe("accepted");
    expect(
      await t.send({
        type: "ApplyInvitation",
        message_id: DEMO_INVITATIONS.reply,
        calendar_id: null,
      }),
    ).toMatchObject({ type: "Error", code: "invalid_request" });
  });

  it("accepts or declines a counter-proposal", async () => {
    const accept = seeded();
    const counter = await card(accept, DEMO_INVITATIONS.counter);
    expect(counter.actions).toEqual(expect.arrayContaining(["accept-counter", "decline-counter"]));
    const moved = scheduled(
      await accept.send({
        type: "RespondToCounter",
        message_id: counter.message_id,
        accept: true,
        comment: null,
      }),
    );
    expect(moved.event?.fields.start).toEqual(counter.counter?.start);
    expect(moved.sent[0]?.method).toBe("request");
    expect(moved.invitation?.actions).not.toContain("accept-counter");

    const decline = seeded();
    const declined = scheduled(
      await decline.send({
        type: "RespondToCounter",
        message_id: DEMO_INVITATIONS.counter,
        accept: false,
        comment: "Sorry",
      }),
    );
    expect(declined.sent[0]).toMatchObject({
      method: "decline-counter",
      recipients: ["katherine@nasa.example"],
    });
  });

  it("organizer: sends invitations and cancels for some attendees or the meeting", async () => {
    const t = seeded();
    expect(
      await t.send({
        type: "SendInvitations",
        event_id: "ev-standup",
        recurrence_id: null,
        attendees: [],
        comment: null,
      }),
    ).toMatchObject({ type: "Error", code: "invalid_request" });
    const sent = scheduled(
      await t.send({
        type: "SendInvitations",
        event_id: "ev-review",
        recurrence_id: null,
        attendees: [],
        comment: "Agenda attached",
      }),
    );
    expect(sent.sent[0]).toMatchObject({
      method: "request",
      recipients: ["katherine@nasa.example"],
    });
    const partial = scheduled(
      await t.send({
        type: "CancelMeeting",
        event_id: "ev-review",
        scope: "attendees",
        recurrence_id: null,
        attendees: ["katherine@nasa.example"],
        comment: null,
      }),
    );
    expect(partial.event?.fields.attendees).toEqual([]);
    expect(partial.sent[0]?.method).toBe("cancel");
  });

  it("organizer: cancelling the whole meeting removes the event", async () => {
    const t = seeded();
    const result = scheduled(
      await t.send({
        type: "CancelMeeting",
        event_id: "ev-review",
        scope: "meeting",
        recurrence_id: null,
        attendees: [],
        comment: null,
      }),
    );
    expect(result.removed_event_id).toBe("ev-review");
    expect(t.data.features.pim.events.some((e) => e.id === "ev-review")).toBe(false);
    expect(
      await t.send({
        type: "CancelMeeting",
        event_id: "ev-review",
        scope: "occurrence",
        recurrence_id: null,
        attendees: [],
        comment: null,
      }),
    ).toMatchObject({ type: "Error", code: "not_found" });
  });

  it("free/busy: own calendars, demo people, unknown people and errors", async () => {
    const t = seeded();
    const today = t.data.features.scheduling.today ?? "";
    const from = new Date(`${today}T00:00:00Z`).getTime() + 86_400_000 - 4 * 3_600_000;
    const r = await t.send({
      type: "QueryFreeBusy",
      attendees: [
        "ada@work.example",
        "grace@navy.example",
        "nobody@nowhere.example",
        "linus@kernel.example",
      ],
      from: new Date(from).toISOString().replace(".000", ""),
      to: new Date(from + 2 * 86_400_000).toISOString().replace(".000", ""),
      identity_id: null,
      ignore_event_id: null,
    });
    if (r.type !== "FreeBusy") throw new Error(JSON.stringify(r));
    const [own, grace, nobody, linus] = r.result.attendees;
    expect(own?.source).toEqual({ type: "own" });
    expect(own?.periods.length).toBeGreaterThan(0);
    expect(grace?.source.type).toBe("caldav");
    expect(grace?.periods.some((p) => p.kind === "busy")).toBe(true);
    expect(nobody).toMatchObject({ source: { type: "unknown" }, periods: [], error: null });
    expect(linus).toMatchObject({ error: "network", localized: { id: "error-network" } });
  });

  it("free/busy validates like the core", async () => {
    const t = seeded();
    const ask = (attendees: string[], from: string, to: string) =>
      t.send({
        type: "QueryFreeBusy",
        attendees,
        from,
        to,
        identity_id: null,
        ignore_event_id: null,
      });
    expect(await ask([], "2026-10-01T00:00:00Z", "2026-10-02T00:00:00Z")).toMatchObject({
      localized: { id: "freebusy-issue-attendee-count" },
    });
    expect(await ask(["bad"], "2026-10-01T00:00:00Z", "2026-10-02T00:00:00Z")).toMatchObject({
      localized: { id: "freebusy-issue-invalid-attendee" },
    });
    expect(
      await ask(["a@b.example"], "2026-10-02T00:00:00Z", "2026-10-01T00:00:00Z"),
    ).toMatchObject({ localized: { id: "freebusy-issue-invalid-range" } });
    expect(
      await ask(["a@b.example"], "2026-01-01T00:00:00Z", "2026-06-01T00:00:00Z"),
    ).toMatchObject({ localized: { id: "freebusy-issue-range-too-long" } });
  });

  it("stores settings, checks the calendar and announces the change", async () => {
    const t = seeded();
    const events: string[] = [];
    t.onEvent((e) => events.push(e.type));
    const settings = {
      add_invitations: false,
      apply_updates: true,
      apply_replies: false,
      apply_cancellations: true,
      calendar_id: "cal-personal",
      confirm_replies: true,
    };
    expect(await t.send({ type: "SetSchedulingSettings", settings })).toEqual({
      type: "SchedulingSettings",
      settings,
    });
    expect(events).toContain("SchedulingSettingsChanged");
    expect(
      await t.send({
        type: "SetSchedulingSettings",
        settings: { ...settings, calendar_id: "cal-holidays" },
      }),
    ).toMatchObject({ type: "Error", code: "not_found" });
  });

  it("offers actions by role, method and state", () => {
    const base = invitation({ message_id: "m", uid: "u", method: "request", role: "attendee" });
    expect(actionsFor({ ...base, method: "request", role: "attendee", state: "invalid" })).toEqual(
      [],
    );
    expect(actionsFor({ ...base, method: "request", role: "attendee", state: "outdated" })).toEqual(
      ["refresh"],
    );
    expect(
      actionsFor({
        ...base,
        method: "refresh",
        role: "organizer",
        state: "current",
        event_id: "e",
      }),
    ).toEqual(["send-current", "open-event"]);
    expect(actionsFor({ ...base, method: "publish", role: "observer", state: "new" })).toEqual([
      "add-to-calendar",
    ]);
  });

  it("decorates message bodies with their card like GetMessage", () => {
    const t = seeded();
    const body = t.data.bodies.get(DEMO_INVITATIONS.cancel);
    if (!body) throw new Error("no body");
    expect(decorateInvitation(t, body).invitation?.method).toBe("cancel");
    const plain = t.data.bodies.get(t.data.messages[0]?.id ?? "");
    if (!plain) throw new Error("no body");
    expect(decorateInvitation(t, plain).invitation).toBeUndefined();
  });
});
