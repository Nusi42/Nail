/**
 * The quick look (R7 M4 / S1, ADR 0246) against the mock core: what it
 * shows (time with length and zone, calendar, place, people with their
 * answers, the call link), its keys (Enter edits, Delete deletes, Escape
 * closes), answering an unanswered invitation with what it overlaps, an
 * organizer's "Send update", and axe.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { CalendarEvent, EventInstance } from "../../api/generated";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { violations } from "../../test/axe";
import { must } from "../../test/must";
import { seedSchedulingDemo } from "../scheduling/mockSeed";
import { EventPopoverHost, focusAfterDelete, occurrenceFields } from "./EventPopover";
import { EVENT_DIALOG, SCOPE_DIALOG } from "./names";
import { PimToasts } from "./Reminders";
import { instancesOf } from "./testing";
import { pimStoreFor } from "./store";

const ANCHOR = { left: 10, top: 10, width: 80, height: 40 };

/** Opens the quick look on occurrence `index` of event `id` (the host must be rendered). */
async function showQuickLook(env: Env, id: string, index = 0) {
  const instance = must((await instancesOf(env, id))[index]);
  act(() => {
    pimStoreFor(env.store).getState().setQuickLook({ instance, anchor: ANCHOR });
  });
  const popover = screen.getByRole("dialog", { name: instance.title });
  await settleQueries(env);
  return { popover, instance };
}

/** Renders the host and opens the quick look. */
async function openQuickLook(env: Env, id: string, index = 0) {
  renderWithProviders(<EventPopoverHost />, env);
  return showQuickLook(env, id, index);
}

/**
 * An unanswered invitation from Grace at the time of "Sprint planning",
 * which also overlaps "Lunch with Grace": the user is asked, no answer.
 */
function addInvitation(env: Env): void {
  const events = env.transport.data.features.pim.events;
  const planning = must(events.find((e) => e.id === "ev-planning"));
  const invitation: CalendarEvent = structuredClone(planning);
  invitation.id = "ev-invite";
  const person = {
    kind: "individual" as const,
    delegated_to: [],
    delegated_from: [],
  };
  invitation.fields = {
    ...invitation.fields,
    title: "Roadmap review",
    alarms: [],
    organizer: { email: "grace@navy.example", name: "Grace Hopper" },
    attendees: [
      {
        ...person,
        email: "grace@navy.example",
        name: "Grace Hopper",
        role: "chair",
        status: "accepted",
        rsvp: false,
      },
      {
        ...person,
        email: "ada@work.example",
        name: "Ada Lovelace",
        role: "required",
        status: "needs-action",
        rsvp: true,
      },
    ],
  };
  events.push(invitation);
}

/** Accepts the invitation from the quick look with the toasts shown (undo-send delay `secs`). */
async function acceptWithToasts(secs?: number) {
  const user = userEvent.setup();
  const env = createEnv();
  if (secs !== undefined) env.transport.data.accountsConfig.undo_send_secs = secs;
  addInvitation(env);
  renderWithProviders(<PimToasts />, env);
  const { popover } = await openQuickLook(env, "ev-invite");
  const answers = within(popover).getByRole("group", { name: "Your answer" });
  await user.click(within(answers).getByRole("button", { name: "Accept" }));
  return { env, user };
}

describe("the quick look", () => {
  it("shows nothing until a view opens a quick look, then closes it on Escape", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<EventPopoverHost />, env);
    expect(screen.queryByRole("dialog")).toBeNull();
    const { popover } = await showQuickLook(env, "ev-lunch");
    expect(popover).toHaveAttribute("data-part", "pim.popover");
    await user.keyboard("{Escape}");
    expect(pimStoreFor(env.store).getState().quickLook).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("answers when, where, who and how to join", async () => {
    const env = createEnv();
    const { popover } = await openQuickLook(env, "ev-standup", 3);
    expect(within(popover).getByRole("heading", { name: "Stand-up" })).toBeInTheDocument();
    // The range with its length and zone.
    expect(within(popover).getByText(/ · 15 minutes · /)).toBeInTheDocument();
    expect(within(popover).getByText("Room 4")).toBeInTheDocument();
    expect(within(popover).getByText("2 people · 1 accepted, 1 maybe")).toBeInTheDocument();
    expect(within(popover).getByText("Grace Hopper (organizer)")).toBeInTheDocument();
    expect(within(popover).getByText("Ada Lovelace (you)")).toBeInTheDocument();
    expect(within(popover).getByRole("link", { name: "Join" })).toHaveAttribute(
      "href",
      "https://meet.nail.example/standup",
    );
    // The user's own answer is the pressed segment; Edit has the focus.
    const answers = within(popover).getByRole("group", { name: "Your answer" });
    expect(within(answers).getByRole("button", { name: "Accept" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(within(popover).getByRole("button", { name: "Edit" })).toHaveFocus();
    expect(within(popover).getByText("Enter")).toBeInTheDocument();
  });

  it("opens the full editor with Enter", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await openQuickLook(env, "ev-lunch");
    await user.keyboard("{Enter}");
    const pim = pimStoreFor(env.store).getState();
    expect(pim.quickLook).toBeNull();
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    expect(pim.eventEditor).toMatchObject({ mode: "edit", eventId: "ev-lunch" });
  });

  it("deletes with the Delete key, asking first for a recurring occurrence", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await openQuickLook(env, "ev-lunch");
    await user.keyboard("{Delete}");
    await waitFor(() => {
      expect(env.transport.data.features.pim.events.some((e) => e.id === "ev-lunch")).toBe(false);
    });
    expect(screen.queryByRole("dialog")).toBeNull();

    await showQuickLook(env, "ev-standup", 2);
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(env.store.getState().windows).toContain(SCOPE_DIALOG);
    expect(pimStoreFor(env.store).getState().pendingChange?.kind).toBe("delete");
  });

  it("answers an unanswered invitation and names what it overlaps", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    addInvitation(env);
    const { popover } = await openQuickLook(env, "ev-invite");
    expect(popover.querySelector('[data-part="pim.popover.body"]')).toHaveAttribute(
      "data-status",
      "needs-action",
    );
    // The core's overlaps (the invitation card's rule), asked once the answer is known.
    expect(await within(popover).findByText(/^Overlaps 2 events: /)).toHaveTextContent(
      /Sprint planning/,
    );
    const answers = within(popover).getByRole("group", { name: "Your answer" });
    for (const name of ["Accept", "Maybe", "Decline"]) {
      expect(within(answers).getByRole("button", { name })).toHaveAttribute(
        "aria-pressed",
        "false",
      );
    }
    await user.click(within(answers).getByRole("button", { name: "Accept" }));
    expect(
      await within(popover).findByText("Accepted. Your answer is on its way."),
    ).toBeInTheDocument();
    expect(requestsOf(env, "RespondToInvitation").at(-1)).toMatchObject({
      target: { type: "event", event_id: "ev-invite", recurrence_id: null },
      response: "accept",
      send: true,
    });
  });

  it("offers Undo while the answer waits in the outbox, and takes the mail back (R8)", async () => {
    const { env, user } = await acceptWithToasts();
    const toast = await screen.findByText("Your answer goes out in a moment.");
    const data = env.transport.data;
    const queued = must(data.outbox.find((e) => e.state === "scheduled"));
    expect(data.drafts.get(queued.draft_id)?.automatic).toEqual({ kind: "itip" });
    await user.click(within(must(toast.closest("li"))).getByRole("button", { name: "Undo" }));
    expect(
      await screen.findByText("Your answer was not sent. Your calendar keeps it."),
    ).toBeInTheDocument();
    expect(data.outbox.some((e) => e.id === queued.id)).toBe(false);
    expect(data.drafts.has(queued.draft_id)).toBe(false);
    // The calendar keeps the answer.
    const invite = must(data.features.pim.events.find((e) => e.id === "ev-invite"));
    expect(invite.fields.attendees.find((a) => a.email === "ada@work.example")?.status).toBe(
      "accepted",
    );
  });

  it("gives the reply toast's Undo the focus, one key away, and the focus back after it (R8 review)", async () => {
    const { user } = await acceptWithToasts();
    const toast = must(
      (await screen.findByText("Your answer goes out in a moment.")).closest("li"),
    );
    const undo = within(toast).getByRole("button", { name: "Undo" });
    await waitFor(() => {
      expect(undo).toHaveFocus();
    });
    await user.keyboard("{Enter}");
    expect(
      await screen.findByText("Your answer was not sent. Your calendar keeps it."),
    ).toBeInTheDocument();
    await waitFor(() => {
      expect(document.activeElement).not.toBe(document.body);
    });
  });

  it("drops the Undo once the answer left the outbox (R8)", async () => {
    const { env } = await acceptWithToasts(0);
    await waitFor(() => {
      expect(screen.queryByText("Your answer goes out in a moment.")).toBeNull();
    });
    expect(
      pimStoreFor(env.store)
        .getState()
        .toasts.some((t) => t.kind === "reply"),
    ).toBe(false);
  });

  it("answers one occurrence of a recurring invitation, or the whole series (R8)", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    addInvitation(env);
    const events = env.transport.data.features.pim.events;
    const standup = must(events.find((e) => e.id === "ev-standup"));
    must(events.find((e) => e.id === "ev-invite")).recurrence = structuredClone(standup.recurrence);
    const { popover, instance } = await openQuickLook(env, "ev-invite", 1);
    const scope = within(popover).getByRole("group", { name: "Answer for" });
    // The whole series unless the user picks this occurrence (the invitation card's default).
    expect(within(scope).getByRole("radio", { name: "All occurrences" })).toBeChecked();
    await user.click(within(scope).getByRole("radio", { name: "One occurrence" }));
    const answers = within(popover).getByRole("group", { name: "Your answer" });
    await user.click(within(answers).getByRole("button", { name: "Decline" }));
    await within(popover).findByText(/Declined/);
    expect(requestsOf(env, "RespondToInvitation").at(-1)).toMatchObject({
      target: { type: "event", event_id: "ev-invite", recurrence_id: instance.recurrence_id },
      response: "decline",
    });
    // Only that occurrence changed: the series still waits for an answer.
    const series = must(events.find((e) => e.id === "ev-invite"));
    expect(series.fields.attendees.find((a) => a.email === "ada@work.example")?.status).toBe(
      "needs-action",
    );
    expect(series.overrides).toHaveLength(1);
  });

  it("answers a single event without asking for the scope", async () => {
    const env = createEnv();
    addInvitation(env);
    const { popover } = await openQuickLook(env, "ev-invite");
    expect(within(popover).queryByRole("group", { name: "Answer for" })).toBeNull();
  });

  it("lets an organizer send the update, and has no axe violations", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    seedSchedulingDemo(env.transport.data);
    const { popover } = await openQuickLook(env, "ev-review");
    expect(within(popover).queryByRole("group", { name: "Your answer" })).toBeNull();
    expect(within(popover).getByText("Ada Lovelace (organizer, you)")).toBeInTheDocument();
    expect(await violations(popover)).toEqual([]);
    await user.click(within(popover).getByRole("button", { name: "Send update" }));
    expect(await within(popover).findByText("The update is on its way.")).toBeInTheDocument();
    expect(requestsOf(env, "SendInvitations").at(-1)).toMatchObject({
      event_id: "ev-review",
      attendees: [],
    });
  });

  it("shows an override's own fields for its occurrence", () => {
    const rid = { type: "date", date: "2026-11-27" } as const;
    const event = {
      fields: { title: "Series" },
      overrides: [{ recurrence_id: rid, fields: { title: "Moved" } }],
    } as unknown as CalendarEvent;
    const instance = { recurrence_id: rid } as unknown as EventInstance;
    expect(occurrenceFields(event, instance).title).toBe("Moved");
    expect(occurrenceFields(event, { ...instance, recurrence_id: null }).title).toBe("Series");
  });
});

describe("focus after Delete", () => {
  /** Cards `ids` and the grid's active slot in the document. */
  function view(ids: string[]): HTMLElement[] {
    const slot = document.createElement("button");
    slot.dataset.part = "calendar.slot";
    slot.tabIndex = 0;
    const cards = ids.map((id) => {
      const card = document.createElement("button");
      card.dataset.event = id;
      return card;
    });
    document.body.append(...cards, slot);
    return [...cards, slot];
  }

  it("moves to the next card once the deleted card is gone, else the previous, else the slot", () => {
    const [a, b, c, slot] = view(["a", "b", "c"]).map((e) => must(e));
    b?.focus();
    const refocus = focusAfterDelete("b");
    b?.remove();
    refocus();
    expect(c).toHaveFocus();
    const last = focusAfterDelete("c");
    c?.remove();
    last();
    expect(a).toHaveFocus();
    const only = focusAfterDelete("a");
    a?.remove();
    only();
    expect(slot).toHaveFocus();
    slot?.remove();
  });

  it("leaves a focus that found its own place alone", () => {
    const [a, b, slot] = view(["a", "b"]).map((e) => must(e));
    const outside = document.createElement("input");
    document.body.append(outside);
    const refocus = focusAfterDelete("a");
    outside.focus();
    refocus();
    expect(outside).toHaveFocus();
    for (const el of [a, b, slot, outside]) el?.remove();
  });
});
