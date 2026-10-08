/**
 * The calendar widgets against the mock core: views, navigation, the view
 * switcher, keyboard navigation and creation, moving events with the
 * keyboard and the pointer (single and recurring), the mini-month and the
 * agenda, in the UI locale.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {
  createEnv,
  renderNode,
  renderWithProviders,
  widget,
  type Env,
  settleQueries,
} from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { EVENT_DIALOG, SCOPE_DIALOG } from "./names";
import { PIM_SKIN_CSS } from "./skin";
import { pimStoreFor } from "./store";
import { dateString, localZone, todayIn } from "./time";
import { must } from "../../test/must";
import { stringMatching } from "../../test/matchers";
import { inTimeZone } from "../../test/env";
import { BERLIN } from "../../test/mockIds";

inTimeZone(BERLIN);

function geometryText(): string {
  const style = document.querySelector<HTMLStyleElement>("style[data-nail-theme]");
  return [...(style?.sheet?.cssRules ?? [])].map((r) => r.cssText).join("\n");
}

function eventOf(env: Env, id: string) {
  return env.transport.data.features.pim.events.find((e) => e.id === id);
}

describe("calendar widget", () => {
  it("shows the week with today's events placed side by side", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const planning = screen.getByRole("button", { name: /Sprint planning/ });
    expect(lunch).toHaveAttribute("data-part", "calendar.event");
    expect(planning.closest('[data-part="calendar.column"]')).toBe(
      lunch.closest('[data-part="calendar.column"]'),
    );
    // Overlapping events share the column: half the width each.
    expect(geometryText()).toMatch(/width: 50%/);
    expect(document.querySelectorAll('[data-part="calendar.day-head"]')).toHaveLength(7);
    const today = dateString(todayIn(localZone()));
    expect(
      document.querySelector(`[data-part="calendar.day-head"][data-day="${today}"]`),
    ).toHaveAttribute("aria-current", "date");
    // The recurring stand-up shows on weekdays; all-day events in the all-day row.
    expect(screen.getAllByRole("button", { name: /Stand-up/ }).length).toBeGreaterThan(0);
    expect(document.querySelector('[data-part="calendar.slots"]')).toHaveAttribute("role", "grid");
  });

  it("starts weeks on Monday in a German UI and on Sunday in an English one", async () => {
    const env = createEnv();
    const { unmount } = renderNode(widget("calendar", { options: { view: "week" } }), env, {
      locale: "de",
    });
    await settleQueries(env);
    const first =
      document.querySelector('[data-part="calendar.day-head"]')?.getAttribute("data-day") ?? "";
    expect(new Date(`${first}T12:00:00Z`).getUTCDay()).toBe(1);
    unmount();
    renderNode(widget("calendar", { options: { view: "week" } }), env, { locale: "en" });
    await settleQueries(env);
    const sunday =
      document.querySelector('[data-part="calendar.day-head"]')?.getAttribute("data-day") ?? "";
    expect(new Date(`${sunday}T12:00:00Z`).getUTCDay()).toBe(0);
  });

  it("navigates, switches views and returns to today", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "day" } }), env);
    await screen.findByRole("button", { name: /Lunch with Grace/ });
    const title = () => document.querySelector('[data-part="calendar.title"]')?.textContent ?? "";
    const before = title();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(title()).not.toBe(before);
    expect(screen.queryByRole("button", { name: /Lunch with Grace/ })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Today" }));
    expect(title()).toBe(before);
    await user.click(screen.getByRole("button", { name: "Month" }));
    expect(document.querySelector('[data-part="calendar"]')).toHaveAttribute("data-view", "month");
    expect(screen.getByRole("button", { name: "Month" })).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Year" }));
    expect(document.querySelectorAll('[data-part="calendar.year.month"]')).toHaveLength(12);
    await user.click(screen.getByRole("button", { name: "Agenda" }));
    expect(await screen.findByRole("button", { name: /Design review/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Free time" }));
    expect(await screen.findAllByRole("button", { name: /^Free on/ })).not.toHaveLength(0);
  });

  it("creates events from the keyboard in the slot grid", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week", "scroll-to": 9 } }), env);
    await settleQueries(env);
    const active = document.querySelector<HTMLElement>('[data-part="calendar.slot"][tabindex="0"]');
    expect(active).not.toBeNull();
    active?.focus();
    await user.keyboard("{ArrowDown}{ArrowDown}");
    expect(document.activeElement?.getAttribute("data-slot")).toMatch(/-11$/);
    await user.keyboard("{Enter}");
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    const editor = pimStoreFor(env.store).getState().eventEditor;
    expect(editor).toMatchObject({
      mode: "new",
      start: { type: "zoned", local: stringMatching(/T11:00:00$/), tz: BERLIN },
    });
  });

  it("keeps one tab stop in the hours when the view gets fewer days", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    await settleQueries(env);
    const last = document.querySelector<HTMLElement>(
      '[data-part="calendar.slot"][data-slot="6-9"]',
    );
    act(() => last?.focus());
    expect(last).toHaveAttribute("tabindex", "0");
    await user.click(screen.getByRole("button", { name: "Day" }));
    expect(document.querySelectorAll('[data-part="calendar.slot"][tabindex="0"]')).toHaveLength(1);
  });

  it("moves and resizes single events with Alt + arrows", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const start = eventOf(env, "ev-lunch")?.fields.start;
    act(() => {
      lunch.focus();
    });
    fireEvent.keyDown(lunch, { key: "ArrowDown", altKey: true });
    await waitFor(() => {
      expect(eventOf(env, "ev-lunch")?.fields.start).toEqual({
        type: "floating",
        local: start?.type === "floating" ? start.local.replace("12:30", "12:45") : "",
      });
    });
    fireEvent.keyDown(await screen.findByRole("button", { name: /Lunch with Grace/ }), {
      key: "ArrowDown",
      altKey: true,
      shiftKey: true,
    });
    await waitFor(() => {
      expect(eventOf(env, "ev-lunch")?.fields.end).toMatchObject({
        local: stringMatching(/T14:00:00$/),
      });
    });
  });

  it("asks this / this and following / all before moving a recurring event", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "work-week" } }), env);
    await settleQueries(env);
    const standups = await screen.findAllByRole("button", { name: /Stand-up/ });
    expect(standups).toHaveLength(5);
    fireEvent.keyDown(must(standups[0]), { key: "ArrowRight", altKey: true });
    await waitFor(() => {
      expect(env.store.getState().windows).toContain(SCOPE_DIALOG);
    });
    expect(pimStoreFor(env.store).getState().pendingChange).toMatchObject({ kind: "move" });
    fireEvent.keyDown(must(standups[0]), { key: "Delete" });
    await waitFor(() => {
      expect(pimStoreFor(env.store).getState().pendingChange).toMatchObject({ kind: "delete" });
    });
  });

  it("drags an event to another time and resizes it with the pointer", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const columns = must(document.querySelector<HTMLElement>('[data-part="calendar.columns"]'));
    // 24 hours over 2400 px: 100 px per hour; 7 columns over 700 px.
    columns.getBoundingClientRect = () => ({
      x: 0,
      y: 0,
      top: 0,
      left: 0,
      right: 700,
      bottom: 2400,
      width: 700,
      height: 2400,
      toJSON: () => "",
    });
    fireEvent.pointerDown(lunch, { button: 0, clientX: 50, clientY: 1250, pointerId: 1 });
    fireEvent.pointerMove(lunch, { clientX: 50, clientY: 1350, pointerId: 1 });
    expect(document.querySelector('[data-part="calendar.ghost"]')).not.toBeNull();
    fireEvent.pointerUp(lunch, { clientX: 50, clientY: 1350, pointerId: 1 });
    await waitFor(() => {
      expect(eventOf(env, "ev-lunch")?.fields.start).toMatchObject({
        local: stringMatching(/T13:30:00$/),
      });
    });
    // The click that ends a drag does not open the editor.
    fireEvent.click(lunch);
    expect(env.store.getState().windows).not.toContain(EVENT_DIALOG);
    const moved = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const handle = must(moved.querySelector<HTMLElement>('[data-part="calendar.event.resize"]'));
    fireEvent.pointerDown(handle, { button: 0, clientX: 50, clientY: 1450, pointerId: 2 });
    fireEvent.pointerMove(moved, { clientX: 50, clientY: 1500, pointerId: 2 });
    fireEvent.pointerUp(moved, { clientX: 50, clientY: 1500, pointerId: 2 });
    await waitFor(() => {
      expect(eventOf(env, "ev-lunch")?.fields.end).toMatchObject({
        local: stringMatching(/T15:00:00$/),
      });
    });
  });

  it("opens the editor on an event and moves events between days in the month view", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "month", "max-events": 2 } }), env);
    const planning = await screen.findByRole("button", { name: /Sprint planning/ });
    await user.click(planning);
    expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    expect(pimStoreFor(env.store).getState().eventEditor).toMatchObject({
      mode: "edit",
      eventId: "ev-planning",
    });
    const before = eventOf(env, "ev-planning")?.fields.start;
    fireEvent.keyDown(planning, { key: "ArrowRight", altKey: true });
    await waitFor(() => {
      expect(eventOf(env, "ev-planning")?.fields.start).not.toEqual(before);
    });
    // Days with more events than fit offer "n more".
    expect(screen.getAllByRole("button", { name: /more\)/ }).length).toBeGreaterThan(0);
    // Arrow keys move between days; Enter shows the day.
    const cell = must(
      document.querySelector<HTMLElement>('[data-part="calendar.month.day"][tabindex="0"]'),
    );
    act(() => {
      cell.focus();
    });
    await user.keyboard("{ArrowRight}");
    const next = document.activeElement as HTMLElement;
    expect(next.getAttribute("data-day")).not.toBe(cell.getAttribute("data-day"));
    await user.keyboard("{Enter}");
    expect(pimStoreFor(env.store).getState()).toMatchObject({
      view: "day",
      anchor: next.getAttribute("data-day"),
    });
  });

  it("follows the mini-month and shows today's agenda", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(
      <>
        <NodeView node={widget("mini-month")} path="t/0" />
        <NodeView node={widget("calendar", { id: "cal", options: { view: "day" } })} path="t/1" />
        <NodeView node={widget("agenda")} path="t/2" />
      </>,
      env,
    );
    await settleQueries(env);
    const grid = screen.getByRole("grid", { name: /\d{4}/ });
    expect(
      within(grid)
        .getAllByRole("gridcell")
        .some((c) => c.getAttribute("data-state")?.includes("busy")),
    ).toBe(true);
    const agenda = must(document.querySelector<HTMLElement>('[data-part="agenda"]'));
    expect(within(agenda).getAllByRole("button", { name: /Lunch with Grace/ })).toHaveLength(1);
    const tomorrow = dateString(todayIn(localZone()).add({ days: 1 }));
    const cell = grid.querySelector<HTMLElement>(`[data-day="${tomorrow}"]`);
    if (cell) {
      await user.click(cell);
      expect(pimStoreFor(env.store).getState().anchor).toBe(tomorrow);
      const calendar = must(document.querySelector<HTMLElement>('[data-part="calendar"]'));
      expect(
        await within(calendar).findByRole("button", { name: /Design review/ }),
      ).toBeInTheDocument();
    }
    // Keyboard: Page Down shows the next month.
    const focusable = must(grid.querySelector<HTMLElement>('[tabindex="0"]'));
    act(() => {
      focusable.focus();
    });
    const month = document.querySelector('[data-part="mini-month.title"]')?.textContent;
    await user.keyboard("{PageDown}");
    expect(document.querySelector('[data-part="mini-month.title"]')?.textContent).not.toBe(month);
  });
});

describe("calendar toolbar and event cards (M5b)", () => {
  it("puts Today, previous / next, the range, the view switch and New event on one line", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(
      widget("calendar", {
        options: { view: "week", views: "day week month year agenda free-time" },
      }),
      env,
    );
    await settleQueries(env);
    const toolbar = must(document.querySelector<HTMLElement>('[data-part="calendar.toolbar"]'));
    const order = [...toolbar.children]
      .flatMap((el) =>
        el.getAttribute("data-part") === "calendar.nav" ? [el, ...el.children] : [el],
      )
      .map(
        (el) =>
          el.getAttribute("data-field") ?? el.getAttribute("data-part") ?? el.tagName.toLowerCase(),
      );
    expect(order).toEqual([
      "calendar.nav",
      "calendar-today",
      "calendar-previous",
      "calendar-next",
      "calendar.title",
      "calendar-view",
      "calendar-new-event",
    ]);
    // The range is one heading; its full text is the tooltip when it is cut.
    const title = within(toolbar).getByRole("heading", { level: 2 });
    expect(title).toHaveAttribute("title", title.textContent);
    // The segmented switch: toggle buttons, exactly one pressed.
    const views = within(toolbar).getByRole("group", { name: "View" });
    expect(views).toHaveAttribute("data-part", "segmented");
    expect(
      within(views)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual(["Day", "Week", "Month", "Year", "Agenda", "Free time"]);
    expect(within(views).getAllByRole("button", { pressed: true })).toHaveLength(1);
    await user.click(within(views).getByRole("button", { name: "Free time" }));
    expect(document.querySelector('[data-part="calendar"]')).toHaveAttribute(
      "data-view",
      "free-time",
    );
    expect(within(views).getByRole("button", { name: "Free time" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    // Icon buttons carry their names.
    expect(within(toolbar).getByRole("button", { name: "Previous" })).toHaveAttribute(
      "data-variant",
      "icon ghost",
    );
    expect(within(toolbar).getByRole("button", { name: "New event" })).toHaveAttribute(
      "data-variant",
      "primary",
    );
  });

  it("switches views from a drop-down with view-switch=menu", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week", "view-switch": "menu" } }), env);
    await settleQueries(env);
    expect(screen.queryByRole("group", { name: "View" })).toBeNull();
    await user.selectOptions(screen.getByLabelText("View"), "month");
    expect(document.querySelector('[data-part="calendar"]')).toHaveAttribute("data-view", "month");
    expect(pimStoreFor(env.store).getState().view).toBe("month");
  });

  it("shows events as cards: title, time, location, tinted in the calendar colour", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const parts = [...lunch.children].map((c) => c.getAttribute("data-part"));
    expect(parts.slice(0, 2)).toEqual(["event-card.title", "event-card.time"]);
    expect(lunch.querySelector('[data-part="event-card.title"]')).toHaveTextContent(
      "Lunch with Grace",
    );
    // Personal is #3b82f6: a bar in the colour and a light tint of it.
    const css = geometryText();
    expect(css).toMatch(/border-left-color: rgb\(59, 130, 246\)/);
    expect(css).toMatch(/color-mix\(in srgb, #3b82f6 16%, var\(--nail-color-bg\)\)/);
  });
});

describe("calendar views after the M5b review", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("shows free time per working day within the working hours, without weekends", async () => {
    const env = createEnv();
    renderNode(
      widget("calendar", {
        options: { view: "free-time", "work-start": 8, "work-end": 18, duration: 30 },
      }),
      env,
      { locale: "en" },
    );
    await settleQueries(env);
    const days = [...document.querySelectorAll('[data-part="calendar.free-time.day"]')];
    expect(days).toHaveLength(5);
    for (const day of days) {
      expect(day.getAttribute("aria-label")).not.toMatch(/Saturday|Sunday/);
    }
    // Every slot lies inside 8:00–18:00: never a midnight-to-midnight stretch.
    const slots = screen.getAllByRole("button", { name: /^Free on/ });
    expect(slots.length).toBeGreaterThanOrEqual(5);
    for (const slot of slots) {
      expect(slot.textContent).not.toMatch(/12:00\s?AM/);
    }
  });

  it("gives the year's months a visible name", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "year" } }), env, { locale: "en" });
    await settleQueries(env);
    const titles = [...document.querySelectorAll('[data-part="calendar.year.month.title"]')];
    expect(titles.map((t) => t.textContent)).toEqual([
      "January",
      "February",
      "March",
      "April",
      "May",
      "June",
      "July",
      "August",
      "September",
      "October",
      "November",
      "December",
    ]);
  });

  it("puts the month grid into a frame that gives the weeks the view's height", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "month" } }), env);
    await settleQueries(env);
    const grid = screen.getByRole("grid");
    expect(grid.parentElement).toHaveAttribute("data-part", "calendar.month.frame");
  });

  it("shows only the start time on month chips, after the title (the label keeps both)", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "month" } }), env);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    expect(lunch).toHaveAttribute("data-part", "calendar.chip");
    const parts = [...lunch.children].map((c) => c.getAttribute("data-part"));
    expect(parts.slice(0, 2)).toEqual(["event-card.title", "event-card.time"]);
    const time = lunch.querySelector('[data-part="event-card.time"]')?.textContent ?? "";
    expect(time).toMatch(/^\d{1,2}:\d{2}\s?(AM|PM)$/);
    expect(lunch.getAttribute("aria-label")).toMatch(/–|-/);
    expect(PIM_SKIN_CSS).toMatch(
      /\[data-part="calendar\.month\.item"\] > \[data-part="calendar\.chip"\] > \[data-part="event-card\.time"\] \{ order: 0;/,
    );
  });

  it("offers the view menu when the segmented switch does not fit, and returns", async () => {
    const callbacks = new Set<() => void>();
    vi.stubGlobal(
      "ResizeObserver",
      class {
        constructor(private readonly cb: () => void) {}
        observe() {
          callbacks.add(this.cb);
        }
        disconnect() {
          callbacks.delete(this.cb);
        }
        unobserve = vi.fn();
      },
    );
    const size = { bar: 700 };
    const client = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
    const scroll = Object.getOwnPropertyDescriptor(Element.prototype, "scrollWidth");
    Object.defineProperty(HTMLElement.prototype, "clientWidth", {
      configurable: true,
      get(this: HTMLElement) {
        const part = this.getAttribute("data-part");
        if (part === "calendar.toolbar") return size.bar;
        if (part === "segmented") return Math.min(380, Math.max(0, size.bar - 580));
        return 100;
      },
    });
    Object.defineProperty(Element.prototype, "scrollWidth", {
      configurable: true,
      get(this: Element) {
        const part = this.getAttribute("data-part");
        if (part === "segmented") return 380;
        if (part === "calendar.toolbar") return size.bar;
        return 100;
      },
    });
    try {
      const env = createEnv();
      renderNode(
        widget("calendar", { options: { view: "week", views: "day week month agenda" } }),
        env,
      );
      await settleQueries(env);
      // 700 px: the switch would need 380 but gets 120 → the menu.
      const menu = screen.getByRole("combobox", { name: "View" });
      expect(menu).toHaveValue("week");
      expect(screen.queryByRole("group", { name: "View" })).toBeNull();
      expect(document.querySelector('[data-part="calendar.toolbar"]')).toHaveAttribute(
        "data-switch",
        "menu",
      );
      // Still too narrow: stays.
      size.bar = 900;
      act(() => {
        callbacks.forEach((cb) => {
          cb();
        });
      });
      expect(screen.getByRole("combobox", { name: "View" })).toBeInTheDocument();
      // As wide as the segmented row needed: back.
      size.bar = 1000;
      act(() => {
        callbacks.forEach((cb) => {
          cb();
        });
      });
      const group = screen.getByRole("group", { name: "View" });
      expect(within(group).getAllByRole("button")).toHaveLength(4);
    } finally {
      if (client) Object.defineProperty(HTMLElement.prototype, "clientWidth", client);
      if (scroll) Object.defineProperty(Element.prototype, "scrollWidth", scroll);
    }
  });
});
