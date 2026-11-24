/**
 * The R7 time grid (ADR 0246) against the mock core: drawing a new event
 * with the pointer and the keyboard (inline title, "More options"), the
 * 15-minute click snap, the now line under the events, working hours, days
 * off and the week number, short cards, card icons, unanswered and
 * declined invitations, tasks due in the all-day row and a task dropped on
 * the grid.
 */
import { act, createEvent, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Attendee, CalendarEvent } from "../../api/generated";
import { userAddresses } from "../../api/mockAccounts";
import { createEnv, renderNode, settleQueries, widget, type Env } from "../../test/harness";
import { inTimeZone } from "../../test/env";
import { stringMatching } from "../../test/matchers";
import { must } from "../../test/must";
import { BERLIN } from "../../test/mockIds";
import { EVENT_DIALOG } from "./names";
import { TASK_DRAG_TYPE, isoWeekNumber } from "./prefs";
import { pimStoreFor } from "./store";
import { localZone, makePimTime, parseDay, todayIn } from "./time";

inTimeZone(BERLIN);

/** 24 hours over 2400 px (100 px per hour) and `days` columns of 100 px. */
function sizeColumns(days: number) {
  const columns = must(document.querySelector<HTMLElement>('[data-part="calendar.columns"]'));
  columns.getBoundingClientRect = () => ({
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: days * 100,
    bottom: 2400,
    width: days * 100,
    height: 2400,
    toJSON: () => "",
  });
}

const slots = () => must(document.querySelector<HTMLElement>('[data-part="calendar.slots"]'));

/** A pointer press on the slots at `y` (100 px per hour), moved to `to` and released there. */
function drawOnSlots(y: number, to = y) {
  fireEvent.pointerDown(slots(), { button: 0, clientX: 50, clientY: y, pointerId: 1 });
  if (to !== y) fireEvent.pointerMove(slots(), { clientX: 50, clientY: to, pointerId: 1 });
}

function releaseSlots(y: number) {
  fireEvent.pointerUp(slots(), { clientX: 50, clientY: y, pointerId: 1 });
}

/** The inline title of a drawn event and its title field. */
async function draftTitle() {
  const draft = await screen.findByRole("dialog", { name: "New event" });
  return { draft, title: within(draft).getByRole("textbox", { name: "Title" }) };
}

function eventsTitled(env: Env, title: string): CalendarEvent[] {
  return env.transport.data.features.pim.events.filter((e) => e.fields.title === title);
}

async function renderDay(env: Env, options: Record<string, unknown> = {}) {
  renderNode(widget("calendar", { options: { view: "day", ...options } }), env);
  await screen.findByRole("button", { name: /Lunch with Grace/ });
  await settleQueries(env);
  sizeColumns(1);
}

/** An event today from `hour` for an hour with the user as an attendee who answered `status`. */
function invitation(env: Env, id: string, hour: number, status: Attendee["status"]): CalendarEvent {
  const tz = localZone();
  const today = todayIn(tz);
  const start = makePimTime(today, { hour, minute: 0 }, tz);
  const end = makePimTime(today, { hour: hour + 1, minute: 0 }, tz);
  const me = must(userAddresses(env.transport.data)[0]);
  const base = must(env.transport.data.features.pim.events.find((e) => e.id === "ev-lunch"));
  return {
    ...base,
    id,
    calendar_id: "cal-work",
    uid: `${id}@nail.example`,
    fields: {
      ...base.fields,
      title: id,
      start,
      end,
      location: null,
      organizer: { email: "grace@navy.example", name: "Grace Hopper" },
      attendees: [
        {
          email: me,
          name: null,
          role: "required",
          status,
          kind: "individual",
          rsvp: true,
          delegated_to: [],
          delegated_from: [],
        },
      ],
    },
  };
}

describe("time grid: drawing a new event", () => {
  it("draws a range in 15-minute steps and saves it with the inline title", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    await renderDay(env);
    // Pressed at 10:05, dragged to 11:31: 10:00 – 11:30.
    drawOnSlots(1008, 1152);
    const drawn = must(document.querySelector('[data-part="calendar.draft"]'));
    expect(drawn.textContent).toMatch(/1 hour,? 30 minutes/);
    releaseSlots(1152);
    const { draft, title } = await draftTitle();
    expect(title).toHaveFocus();
    expect(within(draft).getByRole("button", { name: "Save" })).toBeDisabled();
    await user.type(title, "Prep for the review{Enter}");
    await waitFor(() => {
      expect(eventsTitled(env, "Prep for the review")).toHaveLength(1);
    });
    const saved = must(eventsTitled(env, "Prep for the review")[0]);
    expect(saved.calendar_id).toBe("cal-personal");
    expect(saved.fields.start).toMatchObject({ local: stringMatching(/T10:00:00$/), tz: BERLIN });
    expect(saved.fields.end).toMatchObject({ local: stringMatching(/T11:30:00$/) });
    await waitFor(() => {
      expect(screen.queryByRole("dialog", { name: "New event" })).toBeNull();
    });
  });

  it("snaps a click to the 15-minute slot and opens the editor from More options", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    await renderDay(env);
    // 10:24 → the 10:15 slot, one hour long.
    drawOnSlots(1040);
    releaseSlots(1040);
    const { draft, title } = await draftTitle();
    await user.type(title, "Call Bob");
    await user.click(within(draft).getByRole("button", { name: "More options" }));
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    expect(pimStoreFor(env.store).getState().eventEditor).toMatchObject({
      mode: "new",
      calendarId: "cal-personal",
      start: { local: stringMatching(/T10:15:00$/) },
      end: { local: stringMatching(/T11:15:00$/) },
      fields: { title: "Call Bob" },
    });
    expect(screen.queryByRole("dialog", { name: "New event" })).toBeNull();
  });

  it("drops the draft on Escape", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    await renderDay(env);
    drawOnSlots(1500);
    releaseSlots(1500);
    await draftTitle();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog", { name: "New event" })).toBeNull();
    expect(document.querySelector('[data-part="calendar.draft"]')).toBeNull();
  });

  /** Puts the focus on the grid's active slot (its roving tab stop). */
  const focusActiveSlot = () => {
    act(() => {
      document.querySelector<HTMLElement>('[data-part="calendar.slot"][tabindex="0"]')?.focus();
    });
  };

  it("extends the range with Shift + arrows and saves it with Enter", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    await renderDay(env, { "scroll-to": 9 });
    focusActiveSlot();
    await user.keyboard("{Shift>}{ArrowDown}{ArrowDown}{/Shift}");
    expect(document.querySelector('[data-part="calendar.slot-range"]')).not.toBeNull();
    expect(
      document.querySelectorAll('[data-part="calendar.slot"][aria-selected="true"]'),
    ).toHaveLength(3);
    await user.keyboard("{Enter}");
    await user.type((await draftTitle()).title, "Workshop{Enter}");
    await waitFor(() => {
      expect(eventsTitled(env, "Workshop")).toHaveLength(1);
    });
    const saved = must(eventsTitled(env, "Workshop")[0]);
    expect(saved.fields.start).toMatchObject({ local: stringMatching(/T09:00:00$/) });
    expect(saved.fields.end).toMatchObject({ local: stringMatching(/T12:00:00$/) });
  });

  it("drops the keyboard range's highlight when its draft is cancelled", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    await renderDay(env, { "scroll-to": 9 });
    focusActiveSlot();
    await user.keyboard("{Shift>}{ArrowDown}{ArrowDown}{/Shift}{Enter}");
    await draftTitle();
    await user.keyboard("{Escape}");
    expect(document.querySelector('[data-part="calendar.draft"]')).toBeNull();
    expect(document.querySelector('[data-part="calendar.slot-range"]')).toBeNull();
  });

  it("blocks time for a task dropped on the grid", async () => {
    const env = createEnv();
    await renderDay(env);
    const canvas = must(document.querySelector<HTMLElement>('[data-part="calendar.canvas"]'));
    const payload = JSON.stringify({ taskId: "tk-groceries", title: "Buy groceries" });
    const dataTransfer = {
      types: [TASK_DRAG_TYPE],
      getData: (type: string) => (type === TASK_DRAG_TYPE ? payload : ""),
      dropEffect: "none",
    };
    const over = createEvent.dragOver(canvas, { dataTransfer });
    fireEvent(canvas, over);
    expect(over.defaultPrevented).toBe(true);
    const drop = createEvent.drop(canvas, { dataTransfer });
    Object.defineProperty(drop, "clientX", { value: 50 });
    Object.defineProperty(drop, "clientY", { value: 1410 });
    fireEvent(canvas, drop);
    await waitFor(() => {
      expect(eventsTitled(env, "Buy groceries")).toHaveLength(1);
    });
    const saved = must(eventsTitled(env, "Buy groceries")[0]);
    expect(saved.fields.start).toMatchObject({ local: stringMatching(/T14:00:00$/) });
    expect(saved.fields.end).toMatchObject({ local: stringMatching(/T15:00:00$/) });
    // The task stays.
    expect(env.transport.data.features.pim.tasks.some((t) => t.id === "tk-groceries")).toBe(true);
  });
});

describe("time grid: reading aids", () => {
  it("draws the now line under the events, with a dot on today and the time in the gutter", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    await screen.findByRole("button", { name: /Lunch with Grace/ });
    const today = must(document.querySelector('[data-part="calendar.now"][data-state="today"]'));
    expect(document.querySelector('[data-part="calendar.now"][data-state="week"]')).not.toBeNull();
    expect(document.querySelector('[data-part="calendar.now-label"]')?.textContent).toMatch(/\d/);
    // The hour label the now label lies on (and at most one close by) gives way to it.
    const nearNow = document.querySelectorAll(
      '[data-part="calendar.hour"][data-state~="near-now"]',
    );
    expect(nearNow.length).toBeGreaterThanOrEqual(1);
    expect(nearNow.length).toBeLessThanOrEqual(2);
    // Under the event layer: the line comes before the columns.
    const columns = must(document.querySelector('[data-part="calendar.columns"]'));
    expect(today.compareDocumentPosition(columns) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // Today's column is not shaded any more: no slot carries a "today" state.
    expect(document.querySelector('[data-part="calendar.slot"][data-state~="today"]')).toBeNull();
  });

  it("hatches time outside the working hours, tints days off and shows the week number", async () => {
    const env = createEnv();
    env.transport.data.features.pim.preferences = {
      ...env.transport.data.features.pim.preferences,
      show_week_numbers: true,
    };
    renderNode(widget("calendar", { options: { view: "week" } }), env, { locale: "de" });
    await settleQueries(env);
    const first = parseDay(
      must(document.querySelector('[data-part="calendar.day-head"]')).getAttribute("data-day") ??
        "",
    );
    expect(document.querySelector('[data-part="calendar.week-number"]')?.textContent).toBe(
      `Wk ${String(isoWeekNumber(must(first)))}`,
    );
    // Monday – Friday: before 8:00 and after 18:00; Saturday and Sunday: the whole day.
    expect(
      document.querySelectorAll('[data-part="calendar.off-hours"]:not([data-state])'),
    ).toHaveLength(10);
    expect(
      document.querySelectorAll('[data-part="calendar.off-hours"][data-state="day-off"]'),
    ).toHaveLength(2);
    expect(
      document.querySelectorAll('[data-part="calendar.day-head"][data-state~="day-off"]'),
    ).toHaveLength(2);
  });

  it("shows short events on one line and marks what the core knows with icons", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "work-week" } }), env);
    await settleQueries(env);
    const standup = must((await screen.findAllByRole("button", { name: /Stand-up/ }))[0]);
    expect(standup).toHaveAttribute("data-size", "short");
    const icons = [...standup.querySelectorAll('[data-part="event-card.icon"]')].map((i) =>
      i.getAttribute("data-icon"),
    );
    expect(icons).toEqual(["repeat", "users", "bell"]);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    expect(lunch).not.toHaveAttribute("data-size");
    expect(lunch.querySelector('[data-part="event-card.icons"]')).toBeNull();
  });

  it("shows unanswered invitations as provisional and declined ones by the preference", async () => {
    const env = createEnv();
    env.transport.data.features.pim.events.push(
      invitation(env, "Atlas roadmap review", 15, "needs-action"),
      invitation(env, "Vendor pitch", 17, "declined"),
    );
    renderNode(widget("calendar", { options: { view: "day" } }), env);
    const pending = await screen.findByRole("button", { name: /Atlas roadmap review/ });
    expect(pending.getAttribute("data-state")).toContain("needs-action");
    expect(pending.getAttribute("aria-label")).toContain("not answered yet");
    expect(pending.querySelector('[data-icon="circle-question-mark"]')).not.toBeNull();
    const declined = screen.getByRole("button", { name: /Vendor pitch/ });
    expect(declined.getAttribute("data-state")).toContain("declined");
  });

  it("hides declined invitations with show_declined off", async () => {
    const env = createEnv();
    env.transport.data.features.pim.events.push(invitation(env, "Vendor pitch", 17, "declined"));
    env.transport.data.features.pim.preferences = {
      ...env.transport.data.features.pim.preferences,
      show_declined: false,
    };
    renderNode(widget("calendar", { options: { view: "day" } }), env);
    await screen.findByRole("button", { name: /Lunch with Grace/ });
    await settleQueries(env);
    expect(screen.queryByRole("button", { name: /Vendor pitch/ })).toBeNull();
  });

  it("lists tasks due in the all-day row and completes them with the checkbox", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "day" } }), env);
    const tasks = await screen.findByRole("list", { name: "Tasks due" });
    const check = within(tasks).getByRole("checkbox", {
      name: "Mark “Write the quarterly report” as done",
    });
    await user.click(check);
    await waitFor(() => {
      expect(
        env.transport.data.features.pim.tasks.find((t) => t.id === "tk-report")?.fields.status,
      ).toBe("completed");
    });
    await waitFor(() => {
      expect(screen.queryByRole("checkbox", { name: /quarterly report/ })).toBeNull();
    });
  });
});
