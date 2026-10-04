import { QueryClient } from "@tanstack/react-query";
import { resetLocalTimeZone, setLocalTimeZone } from "@internationalized/date";
import type { EventInstance } from "../../api/generated";
import { TransportClient } from "../../api/client";
import { MockTransport } from "../../api/mock";
import { createUiStore } from "../../state/uiStore";
import {
  applyDelete,
  applyMove,
  movedTimes,
  requestDelete,
  requestMove,
  wallDelta,
} from "./changes";
import { candidates, defaultChoice, mergedFields } from "./merge";
import { SCOPE_DIALOG } from "./names";
import { pimStoreFor } from "./store";
import { stringMatching } from "../../test/matchers";

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error("missing test data");
  return value;
}

const BERLIN = "Europe/Berlin";
beforeAll(() => {
  setLocalTimeZone(BERLIN);
});
afterAll(() => {
  resetLocalTimeZone();
});

function instanceOf(over: Partial<EventInstance>): EventInstance {
  return {
    event_id: "e",
    calendar_id: "cal-personal",
    recurrence_id: null,
    start: { type: "zoned", local: "2026-10-05T09:00:00", tz: "America/New_York" },
    end: { type: "zoned", local: "2026-10-05T10:00:00", tz: "America/New_York" },
    start_utc: "2026-10-05T13:00:00Z",
    end_utc: "2026-10-05T14:00:00Z",
    all_day: false,
    title: "Call",
    location: null,
    status: "confirmed",
    transparency: "busy",
    color: null,
    recurring: false,
    overridden: false,
    has_attendees: false,
    has_alarms: false,
    read_only: false,
    ...over,
  };
}

function env() {
  const transport = new MockTransport();
  const store = createUiStore();
  return {
    transport,
    ctx: {
      client: new TransportClient(transport),
      queryClient: new QueryClient(),
      store,
      pim: pimStoreFor(store),
    },
  };
}

describe("moving occurrences", () => {
  it("moves in the viewer's zone and writes the event's own zone", () => {
    const moved = movedTimes(instanceOf({}), 1, 30, BERLIN);
    expect(moved).toEqual({
      start: { type: "zoned", local: "2026-10-06T09:30:00", tz: "America/New_York" },
      end: { type: "zoned", local: "2026-10-06T10:30:00", tz: "America/New_York" },
    });
    const floating = movedTimes(
      instanceOf({
        start: { type: "floating", local: "2026-10-05T09:00:00" },
        end: { type: "floating", local: "2026-10-05T10:00:00" },
      }),
      0,
      -60,
      BERLIN,
    );
    expect(floating.start).toEqual({ type: "floating", local: "2026-10-05T08:00:00" });
    const allDay = movedTimes(
      instanceOf({
        start: { type: "date", date: "2026-10-05" },
        end: { type: "date", date: "2026-10-06" },
      }),
      2,
      0,
      BERLIN,
    );
    expect(allDay).toEqual({
      start: { type: "date", date: "2026-10-07" },
      end: { type: "date", date: "2026-10-08" },
    });
  });

  it("resizes the end but never below 15 minutes", () => {
    expect(movedTimes(instanceOf({}), 0, 45, BERLIN, true).end).toEqual({
      type: "zoned",
      local: "2026-10-05T10:45:00",
      tz: "America/New_York",
    });
    expect(movedTimes(instanceOf({}), 0, -120, BERLIN, true).end).toEqual({
      type: "zoned",
      local: "2026-10-05T09:15:00",
      tz: "America/New_York",
    });
    expect(
      wallDelta({ type: "date", date: "2026-10-01" }, { type: "date", date: "2026-10-04" }),
    ).toEqual({ days: 3, minutes: 0 });
    expect(
      wallDelta(
        { type: "floating", local: "2026-10-01T09:00:00" },
        { type: "floating", local: "2026-10-01T10:30:00" },
      ),
    ).toEqual({
      days: 0,
      minutes: 90,
    });
  });

  it("moves single events at once and asks the scope for recurring ones", async () => {
    const { transport, ctx } = env();
    const lunch = must(
      (
        await ctx.client.request(
          {
            type: "ListEventInstances",
            from: "2020-01-01T00:00:00Z",
            to: "2040-01-01T00:00:00Z",
            calendar_ids: ["cal-personal"],
            tz: BERLIN,
          },
          "EventInstances",
        )
      ).instances.find((i) => i.event_id === "ev-lunch"),
    );
    const { start, end } = movedTimes(lunch, 0, 60, BERLIN);
    await requestMove(ctx, lunch, start, end);
    expect(
      transport.data.features.pim.events.find((e) => e.id === "ev-lunch")?.fields.start,
    ).toEqual(start);

    const standup = must(
      (
        await ctx.client.request(
          {
            type: "ListEventInstances",
            from: "2020-01-01T00:00:00Z",
            to: "2040-01-01T00:00:00Z",
            calendar_ids: ["cal-work"],
            tz: BERLIN,
          },
          "EventInstances",
        )
      ).instances.find((i) => i.event_id === "ev-standup"),
    );
    const target = movedTimes(standup, 0, 30, BERLIN);
    await requestMove(ctx, standup, target.start, target.end);
    expect(ctx.store.getState().windows).toContain(SCOPE_DIALOG);
    expect(ctx.pim.getState().pendingChange).toMatchObject({ kind: "move" });
    await applyMove(ctx, standup, target.start, target.end, "this");
    const event = must(transport.data.features.pim.events.find((e) => e.id === "ev-standup"));
    expect(event.overrides).toHaveLength(1);
    expect(event.overrides[0]?.fields.start).toEqual(target.start);

    await applyMove(ctx, standup, target.start, target.end, "all");
    expect(
      transport.data.features.pim.events.find((e) => e.id === "ev-standup")?.fields.start,
    ).toMatchObject({
      local: stringMatching(/T10:00:00$/),
    });

    await requestDelete(ctx, standup);
    expect(ctx.pim.getState().pendingChange).toMatchObject({ kind: "delete" });
    await applyDelete(ctx, standup, "this");
    expect(
      transport.data.features.pim.events.find((e) => e.id === "ev-standup")?.recurrence?.exceptions,
    ).toHaveLength(1);
  });
});

describe("merging", () => {
  it("unions lists, lets single values be chosen and keeps one preferred entry", () => {
    const pim = new MockTransport().data.features.pim;
    const grace = must(pim.contacts.find((c) => c.id === "ct-grace"));
    const work = must(pim.contacts.find((c) => c.id === "ct-grace-work"));
    const contacts = [grace, work];
    expect(candidates(contacts, "display_name").map((c) => c.value)).toEqual([
      "Grace Hopper",
      "Grace B. Hopper",
    ]);
    const choice = defaultChoice(contacts, "ct-grace");
    expect(choice.single.display_name).toBe(JSON.stringify("Grace Hopper"));
    const merged = mergedFields(contacts, choice);
    expect(merged.display_name).toBe("Grace Hopper");
    expect(merged.emails).toHaveLength(1);
    expect(merged.phones.map((p) => p.value)).toEqual(["+1 555 0100"]);
    expect(merged.organization?.name).toBe("US Navy");
    const other = mergedFields(contacts, {
      ...choice,
      single: { ...choice.single, display_name: JSON.stringify("Grace B. Hopper") },
      excluded: { phones: ["+15550100"] },
    });
    expect(other.display_name).toBe("Grace B. Hopper");
    expect(other.phones).toEqual([]);
    expect(merged.emails.filter((e) => e.preferred)).toHaveLength(1);
  });
});
