/**
 * R7 views and shell (ADR 0246) against the mock core: quick add from text
 * (S2), the calendar search (S9), the time zone chip (C5), narrow windows
 * (M7: drawer, floating New event, fewer days), the agenda (S7), the month
 * bars and lines (S8), the year heat (C2), the free-time view (C1), empty
 * states (C4), the mini-month (S5), the "Due this week" panel (S12) and the
 * calendar preferences in Settings (S6).
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Task } from "../../api/generated";
import { createEnv, renderNode, settleQueries, widget, type Env } from "../../test/harness";
import { inTimeZone } from "../../test/env";
import { BERLIN } from "../../test/mockIds";
import { must } from "../../test/must";
import { renderSettings, saveSettingsDone } from "../settings/testing";
import { EVENT_DIALOG } from "./names";
import { pimStoreFor } from "./store";
import { eventTitled } from "./testing";
import { dateString, localZone, todayIn } from "./time";
import { stubResize } from "./viewsTesting";

inTimeZone(BERLIN);

const pimOf = (env: Env) => env.transport.data.features.pim;
const today = () => todayIn(localZone());

/** Renders the calendar widget the default layout shows (`calendar-main`). */
function renderCalendar(env: Env, options: Record<string, unknown> = {}) {
  return renderNode(
    widget("calendar", { id: "calendar-main", options: { view: "week", ...options } }),
    env,
  );
}

/** Sets the window width the layout's breakpoints see. */
function windowWidth(width: number) {
  act(() => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
    window.dispatchEvent(new Event("resize"));
  });
}

afterEach(() => {
  windowWidth(1024);
});

/** The calendar with the quick-add field, once its queries settled. */
async function quickAddField(env: Env): Promise<HTMLElement> {
  renderCalendar(env);
  await settleQueries(env);
  return screen.getByRole("textbox", { name: "Quick add an event" });
}

describe("quick add (S2)", () => {
  it("previews the parsed parts and adds the event with Enter", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const field = await quickAddField(env);
    await user.type(field, "Lunch with Bob tomorrow 12:30 at Corner café");
    const preview = await screen.findByRole("list", { name: "What Nail understood" });
    await waitFor(() => {
      expect(within(preview).getByText("Corner café", { exact: false })).toBeInTheDocument();
    });
    const chips = [...preview.querySelectorAll('[data-part="calendar.quick-add.chip"]')];
    expect(chips.map((c) => c.getAttribute("data-kind"))).toEqual([
      "title",
      "date",
      "time",
      "location",
      "calendar",
    ]);
    expect(chips[0]).toHaveTextContent("Lunch with Bob");
    // Resolved values, not the words as typed: the day after today, the end, the calendar.
    expect(preview).not.toHaveTextContent("tomorrow");
    expect(chips[2]).toHaveTextContent(/12:30.*1:30/);
    await user.keyboard("{Enter}");
    const created = await eventTitled(env, "Lunch with Bob");
    expect(created.fields.location).toBe("Corner café");
    const tomorrow = dateString(today().add({ days: 1 }));
    expect(created.fields.start).toMatchObject({ type: "zoned", local: `${tomorrow}T12:30:00` });
    // The default calendar (this device's) and the day shown.
    expect(created.calendar_id).toBe("cal-personal");
    expect(pimStoreFor(env.store).getState().anchor).toBe(tomorrow);
    expect(field).toHaveValue("");
    expect(await screen.findByText(/^Added “Lunch with Bob” on/)).toBeInTheDocument();
  });

  it("opens the editor prefilled with Tab and empties the field with Esc", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const field = await quickAddField(env);
    await user.type(field, "Dentist tomorrow 3pm");
    await user.keyboard("{Escape}");
    expect(field).toHaveValue("");
    await user.type(field, "Dentist tomorrow 3pm");
    await user.keyboard("{Tab}");
    await waitFor(() => {
      expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    });
    const editor = pimStoreFor(env.store).getState().eventEditor;
    expect(editor).toMatchObject({
      mode: "new",
      fields: { title: "Dentist" },
      start: { type: "zoned", local: `${dateString(today().add({ days: 1 }))}T15:00:00` },
    });
    // Nothing was saved.
    expect(pimOf(env).events.some((e) => e.fields.title === "Dentist")).toBe(false);
  });
});

describe("calendar search (S9)", () => {
  it("groups results by upcoming and past and shows the chosen event's day", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderCalendar(env);
    await settleQueries(env);
    // `/` (search.focus) focuses the field.
    act(() => {
      env.store.getState().requestFocus("search");
    });
    const field = screen.getByRole("combobox", { name: "Search events" });
    expect(field).toHaveFocus();
    await user.type(field, "review");
    const results = await screen.findByRole("listbox", { name: "Events found" });
    const upcoming = await within(results).findByRole("group", { name: "Upcoming" });
    expect(within(upcoming).getByRole("option", { name: /Design review/ })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await user.keyboard("{Enter}");
    const tomorrow = dateString(today().add({ days: 1 }));
    await waitFor(() => {
      expect(pimStoreFor(env.store).getState().quickLook?.instance.event_id).toBe("ev-review");
    });
    expect(pimStoreFor(env.store).getState()).toMatchObject({ anchor: tomorrow, view: "day" });
  });

  it("says when nothing matches", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderCalendar(env);
    await settleQueries(env);
    await user.type(screen.getByRole("combobox", { name: "Search events" }), "zzz-nothing");
    expect(await screen.findByText("No events match “zzz-nothing”.")).toBeInTheDocument();
  });
});

describe("time zone chip (C5)", () => {
  it("adds a second zone to the preferences from the toolbar", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderCalendar(env);
    await settleQueries(env);
    await user.click(screen.getByRole("button", { name: /^Time zone: Europe\/Berlin/ }));
    const popover = await screen.findByRole("dialog", { name: "Time zones" });
    await user.selectOptions(
      within(popover).getByRole("combobox", { name: "Second time zone" }),
      "America/New_York",
    );
    await waitFor(() => {
      expect(pimOf(env).preferences.second_time_zone).toBe("America/New_York");
    });
    await waitFor(() => {
      expect(
        screen.getByRole("button", { name: /^Time zones: Europe\/Berlin and America\/New_York/ }),
      ).toBeInTheDocument();
    });
    // The time grid shows the second gutter.
    expect(document.querySelector('[data-part="calendar.body"]')).toHaveAttribute(
      "data-zones",
      "2",
    );
  });
});

describe("narrow windows (M7)", () => {
  it("offers the sidebar as a drawer and a floating New event when the layout has no sidebar", async () => {
    const user = userEvent.setup({ delay: null });
    windowWidth(420);
    const env = createEnv();
    renderCalendar(env);
    await settleQueries(env);
    // One New event: the floating one.
    const fab = screen.getByRole("button", { name: "New event" });
    expect(fab).toHaveAttribute("data-part", "calendar.fab");
    const toggle = screen.getByRole("button", { name: "Show calendars" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    await user.click(toggle);
    const drawer = screen.getByRole("complementary", { name: "Calendars" });
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(within(drawer).getByRole("button", { name: "Close" })).toHaveFocus();
    expect(drawer.querySelector('[data-part="mini-month"]')).not.toBeNull();
    expect(within(drawer).getByRole("region", { name: "Due this week" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("complementary", { name: "Calendars" })).toBeNull();
    // Choosing a day in the drawer's mini-month shows it and closes the drawer.
    await user.click(toggle);
    const grid = within(screen.getByRole("complementary", { name: "Calendars" })).getByRole("grid");
    const other = must(grid.querySelectorAll<HTMLElement>('[role="gridcell"]')[10]);
    await user.click(other);
    expect(screen.queryByRole("complementary", { name: "Calendars" })).toBeNull();
    await user.click(fab);
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
  });

  it("keeps the toolbar's New event and no drawer in a wide window", async () => {
    const env = createEnv();
    renderCalendar(env);
    await settleQueries(env);
    expect(screen.getByRole("button", { name: "New event" })).toHaveAttribute(
      "data-field",
      "calendar-new-event",
    );
    expect(screen.queryByRole("button", { name: "Show calendars" })).toBeNull();
  });

  it("shows three days, then one, when the week's columns would get too narrow", async () => {
    const size = { calendar: 420 };
    const resize = stubResize((el) =>
      el.getAttribute("data-part") === "calendar" ? size.calendar : 0,
    );
    try {
      const env = createEnv();
      renderCalendar(env);
      await settleQueries(env);
      const calendar = must(document.querySelector('[data-part="calendar"]'));
      expect(calendar).toHaveAttribute("data-fallback", "week");
      expect(calendar).toHaveAttribute("data-view", "multi-day");
      expect(document.querySelectorAll('[data-part="calendar.day-head"]')).toHaveLength(3);
      // The switch still says Week; "next" pages by the days shown.
      expect(screen.getByRole("button", { name: "Week" })).toHaveAttribute("aria-pressed", "true");
      expect(pimStoreFor(env.store).getState().shown).toEqual({ view: "multi-day", days: 3 });
      size.calendar = 200;
      resize.notify();
      expect(document.querySelectorAll('[data-part="calendar.day-head"]')).toHaveLength(1);
      size.calendar = 1200;
      resize.notify();
      expect(document.querySelectorAll('[data-part="calendar.day-head"]')).toHaveLength(7);
      expect(calendar).not.toHaveAttribute("data-fallback");
    } finally {
      resize.restore();
    }
  });
});

describe("agenda (S7) and empty states (C4)", () => {
  it("lists events with a time column, Today / Tomorrow and a now marker; a click opens the quick look", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderCalendar(env, { view: "agenda" });
    await settleQueries(env);
    const heads = [...document.querySelectorAll('[data-part="calendar.agenda.day-head"]')].map(
      (h) => h.textContent,
    );
    expect(must(heads[0])).toMatch(/^Today, /);
    expect(must(heads[1])).toMatch(/^Tomorrow, /);
    expect(document.querySelector('[data-part="calendar.agenda.now"]')).toHaveTextContent(
      /^Now · /,
    );
    const review = screen.getByRole("button", { name: /^Design review, / });
    expect(review.querySelector('[data-part="calendar.agenda.time"]')).toHaveTextContent(/2:00/);
    expect(review.querySelector('[data-part="calendar.agenda.dot"]')).not.toBeNull();
    // The multi-day trip shows as all day.
    expect(
      screen.getAllByRole("button", { name: /^Trip to Lisbon, All day/ }).length,
    ).toBeGreaterThan(0);
    await user.click(review);
    expect(pimStoreFor(env.store).getState().quickLook?.instance.event_id).toBe("ev-review");
    await user.dblClick(review);
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
  });

  it("offers Back to today, New event and the nearest event in an empty range", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    // The holidays calendar only: its one holiday lies before the range.
    pimStoreFor(env.store)
      .getState()
      .setAnchor(dateString(today().add({ days: 60 })));
    renderCalendar(env, { view: "agenda", calendars: "cal-holidays" });
    await settleQueries(env);
    const empty = await screen.findByRole("status");
    expect(empty).toHaveTextContent("Nothing planned for these days.");
    expect(
      await within(empty).findByRole("button", { name: /^Last: Public holiday/ }),
    ).toBeInTheDocument();
    await user.click(within(empty).getByRole("button", { name: "Back to today" }));
    expect(pimStoreFor(env.store).getState().anchor).toBeNull();
    // Away again: the nearest event shows its day.
    act(() => {
      pimStoreFor(env.store)
        .getState()
        .setAnchor(dateString(today().add({ days: 60 })));
    });
    await user.click(await screen.findByRole("button", { name: /^Last: Public holiday/ }));
    expect(pimStoreFor(env.store).getState()).toMatchObject({
      anchor: dateString(today().add({ days: 10 })),
      view: "day",
    });
  });
});

describe("month (S8), year (C2) and free time (C1)", () => {
  it("draws a multi-day event as one bar per week row and today as a pill only", async () => {
    const env = createEnv();
    renderCalendar(env, { view: "month" });
    await settleQueries(env);
    const bars = [...document.querySelectorAll('[data-part="calendar.month.bar"]')].filter((b) =>
      (b.getAttribute("aria-label") ?? "").startsWith("Trip to Lisbon"),
    );
    const rows = new Set(
      [0, 1, 2].map((d) =>
        document
          .querySelector(`[data-day="${dateString(today().add({ days: 3 + d }))}"]`)
          ?.closest("tr"),
      ),
    );
    expect(bars).toHaveLength([...rows].filter(Boolean).length);
    const cell = must(document.querySelector(`td[data-day="${dateString(today())}"]`));
    expect(cell).toHaveAttribute("aria-current", "date");
    expect(cell.getAttribute("data-state") ?? "").not.toContain("today");
  });

  it("shades the year's days by booked hours, rings today and numbers the weeks when asked", async () => {
    const env = createEnv();
    pimOf(env).preferences.show_week_numbers = true;
    renderCalendar(env, { view: "year" });
    await settleQueries(env);
    const day = must(
      document.querySelector(
        `[data-part="calendar.year.grid.day"][data-day="${dateString(today())}"]`,
      ),
    );
    expect(day.getAttribute("data-state")).toContain("today");
    // Lunch and planning today: at least an hour booked.
    expect(Number(day.getAttribute("data-heat"))).toBeGreaterThan(0);
    expect(
      document.querySelectorAll('[data-part="calendar.year.grid.week-number"]').length,
    ).toBeGreaterThan(50);
  });

  it("gives the free-time view an hour axis, labelled busy blocks and the free total", async () => {
    const env = createEnv();
    renderCalendar(env, { view: "free-time" });
    await settleQueries(env);
    expect(document.querySelectorAll('[data-part="calendar.free-time.tick"]')).toHaveLength(11);
    const totals = [...document.querySelectorAll('[data-part="calendar.free-time.total"]')];
    expect(totals.length).toBeGreaterThan(0);
    // The total words its length like the rest of the calendar ("1 hour, 30 minutes free").
    for (const total of totals) {
      expect(total.textContent).toMatch(/^(\d+ (hours?|minutes?),? ?)+ free$|^No free time$/);
    }
    const blocks = [...document.querySelectorAll('[data-part="calendar.free-time.block"]')];
    expect(blocks.some((b) => b.textContent === "Stand-up")).toBe(true);
  });
});

describe("mini-month (S5)", () => {
  it("bands the range on screen, dots days with events and numbers weeks", async () => {
    const env = createEnv();
    pimOf(env).preferences.show_week_numbers = true;
    pimStoreFor(env.store).getState().setShown({ view: "day", days: 1 });
    renderNode(widget("mini-month"), env);
    await settleQueries(env);
    const cell = must(document.querySelector(`[data-day="${dateString(today())}"]`));
    expect(cell).toHaveAttribute("data-band", "single");
    expect(cell.querySelectorAll('[data-part="mini-month.grid.dot"]').length).toBeGreaterThan(0);
    expect(cell).toHaveAccessibleName(/, has events$/);
    expect(screen.getAllByRole("rowheader").length).toBeGreaterThanOrEqual(4);
    expect(screen.getAllByRole("rowheader")[0]).toHaveAccessibleName(/^Week \d+$/);
  });
});

describe("Due this week (S12) and the task list's empty state", () => {
  function dueToday(env: Env): Task {
    const task = must(pimOf(env).tasks.find((t) => t.id === "tk-groceries"));
    const due = {
      ...task,
      fields: { ...task.fields, due: { type: "date" as const, date: dateString(today()) } },
    };
    pimOf(env).tasks = [due];
    return due;
  }

  it("lists the open tasks due this week, completes one and adds one due today", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    dueToday(env);
    renderNode(widget("calendar-list"), env);
    await settleQueries(env);
    const panel = screen.getByRole("region", { name: "Due this week" });
    const item = await within(panel).findByText("Buy groceries");
    expect(item.closest("li")).toHaveAttribute("draggable", "true");
    await user.type(
      within(panel).getByRole("textbox", { name: /^Add a task due today to / }),
      "Call the bank",
    );
    await user.click(within(panel).getByRole("button", { name: "Add" }));
    await waitFor(() => {
      expect(pimOf(env).tasks.find((t) => t.fields.title === "Call the bank")?.fields.due).toEqual({
        type: "date",
        date: dateString(today()),
      });
    });
    // Done: it leaves the panel (only open tasks are due).
    await user.click(within(panel).getByRole("checkbox", { name: /^Mark “Buy groceries”/ }));
    await waitFor(() => {
      expect(within(panel).queryByText("Buy groceries")).toBeNull();
    });
  });

  it("puts the task on a drag for the time grid", async () => {
    const env = createEnv();
    const task = dueToday(env);
    renderNode(widget("calendar-list"), env);
    await settleQueries(env);
    const item = must((await screen.findByText("Buy groceries")).closest("li"));
    const data = new Map<string, string>();
    const dataTransfer = {
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? "",
      effectAllowed: "none",
    };
    fireEvent.dragStart(item, { dataTransfer });
    expect(JSON.parse(data.get("application/x-nail-task") ?? "{}")).toEqual({
      taskId: task.id,
      title: "Buy groceries",
    });
  });

  it("drags an untitled task with its own (empty) title, not the placeholder", async () => {
    const env = createEnv();
    const task = dueToday(env);
    pimOf(env).tasks = [{ ...task, fields: { ...task.fields, title: "" } }];
    renderNode(widget("calendar-list"), env);
    await settleQueries(env);
    const item = must((await screen.findByRole("checkbox", { name: /^Mark / })).closest("li"));
    const data = new Map<string, string>();
    fireEvent.dragStart(item, {
      dataTransfer: {
        setData: (type: string, value: string) => data.set(type, value),
        getData: (type: string) => data.get(type) ?? "",
        effectAllowed: "none",
      },
    });
    expect(JSON.parse(data.get("application/x-nail-task") ?? "{}")).toMatchObject({ title: "" });
  });

  it("plans a task at the slot chosen on the grid with the keyboard (no drag needed)", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    dueToday(env);
    renderNode(widget("calendar-list"), env);
    await settleQueries(env);
    const tomorrow = dateString(today().add({ days: 1 }));
    act(() => {
      pimStoreFor(env.store)
        .getState()
        .setPlanSlot({ day: tomorrow, minute: 14 * 60 });
    });
    const panel = screen.getByRole("region", { name: "Due this week" });
    const plan = await within(panel).findByRole("button", {
      name: "Plan “Buy groceries” in the calendar",
    });
    plan.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(pimOf(env).events.some((e) => e.fields.title === "Buy groceries")).toBe(true);
    });
    const created = must(pimOf(env).events.find((e) => e.fields.title === "Buy groceries"));
    expect(created.fields.start).toMatchObject({ local: `${tomorrow}T14:00:00` });
    // The task stays open; the calendar shows the day.
    expect(pimOf(env).tasks[0]?.fields.status).not.toBe("completed");
    expect(pimStoreFor(env.store).getState().anchor).toBe(tomorrow);
  });

  it("shows an empty Tasks view with a reason", async () => {
    const env = createEnv();
    pimOf(env).tasks = [];
    renderNode(widget("task-list"), env);
    await settleQueries(env);
    expect(screen.getByRole("status")).toHaveTextContent(/^No open tasks\./);
  });
});

/** Settings → Calendar & contacts with the page's save bar, the working day's start set to `start`. */
async function prefsWithStart(start: string) {
  const user = userEvent.setup({ delay: null });
  const env = createEnv();
  renderSettings(env, "pim-settings");
  await settleQueries(env);
  const card = screen.getByRole("region", { name: "Calendar preferences" });
  await user.selectOptions(
    within(card).getByRole("combobox", { name: "Working day starts" }),
    start,
  );
  return { user, env, card };
}

describe("calendar preferences in Settings (S6)", () => {
  it("saves working hours, work days, week start and week numbers with the page's Save", async () => {
    const { user, env, card } = await prefsWithStart("540");
    await user.selectOptions(
      within(card).getByRole("combobox", { name: "Working day ends" }),
      "1020",
    );
    await user.click(within(card).getByRole("checkbox", { name: "Saturday" }));
    await user.click(within(card).getByRole("checkbox", { name: "Friday" }));
    await user.selectOptions(within(card).getByRole("combobox", { name: "Week starts on" }), "mon");
    await user.click(within(card).getByRole("checkbox", { name: "Show week numbers" }));
    // Nothing is written before Save.
    expect(pimOf(env).preferences.show_week_numbers).toBe(false);
    await saveSettingsDone(user);
    expect(pimOf(env).preferences).toMatchObject({
      work_hours: { start_minute: 540, end_minute: 1020 },
      work_days: ["mon", "tue", "wed", "thu", "sat"],
      week_start: "mon",
      show_week_numbers: true,
    });
    expect(within(card).getByRole("status")).toHaveTextContent("Calendar preferences saved.");
  });

  it("does not save a working day that ends before it starts", async () => {
    const { card } = await prefsWithStart("1200");
    expect(within(card).getByRole("alert")).toHaveTextContent(
      "The working day must end after it starts.",
    );
    const bar = screen.getByRole("group", { name: /^Unsaved changes to “/ });
    expect(within(bar).getByRole("button", { name: "Save" })).toBeDisabled();
  });
});
