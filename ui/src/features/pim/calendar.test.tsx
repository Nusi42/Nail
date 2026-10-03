/**
 * The calendar widgets against the mock core: views, navigation, the view
 * switcher, keyboard navigation and creation, moving events with the
 * keyboard and the pointer (single and recurring), the mini-month and the
 * agenda, in the UI locale.
 */
import { resetLocalTimeZone, setLocalTimeZone } from "@internationalized/date";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { EVENT_DIALOG, SCOPE_DIALOG } from "./names";
import { pimStoreFor } from "./store";
import { dateString, localZone, todayIn } from "./time";

const BERLIN = "Europe/Berlin";
beforeAll(() => setLocalTimeZone(BERLIN));
afterAll(() => resetLocalTimeZone());

async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
}

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
    await settle(env);
    const first =
      document.querySelector('[data-part="calendar.day-head"]')?.getAttribute("data-day") ?? "";
    expect(new Date(`${first}T12:00:00Z`).getUTCDay()).toBe(1);
    unmount();
    renderNode(widget("calendar", { options: { view: "week" } }), env, { locale: "en" });
    await settle(env);
    const sunday =
      document.querySelector('[data-part="calendar.day-head"]')?.getAttribute("data-day") ?? "";
    expect(new Date(`${sunday}T12:00:00Z`).getUTCDay()).toBe(0);
  });

  it("navigates, switches views and returns to today", async () => {
    const user = userEvent.setup();
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
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week", "scroll-to": 9 } }), env);
    await settle(env);
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
      start: { type: "zoned", local: expect.stringMatching(/T11:00:00$/), tz: BERLIN },
    });
  });

  it("keeps one tab stop in the hours when the view gets fewer days", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    await settle(env);
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
    act(() => lunch.focus());
    fireEvent.keyDown(lunch, { key: "ArrowDown", altKey: true });
    await waitFor(() =>
      expect(eventOf(env, "ev-lunch")?.fields.start).toEqual({
        type: "floating",
        local: start?.type === "floating" ? start.local.replace("12:30", "12:45") : "",
      }),
    );
    fireEvent.keyDown(await screen.findByRole("button", { name: /Lunch with Grace/ }), {
      key: "ArrowDown",
      altKey: true,
      shiftKey: true,
    });
    await waitFor(() =>
      expect(eventOf(env, "ev-lunch")?.fields.end).toMatchObject({
        local: expect.stringMatching(/T14:00:00$/),
      }),
    );
  });

  it("asks this / this and following / all before moving a recurring event", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "work-week" } }), env);
    await settle(env);
    const standups = await screen.findAllByRole("button", { name: /Stand-up/ });
    expect(standups).toHaveLength(5);
    fireEvent.keyDown(standups[0] as HTMLElement, { key: "ArrowRight", altKey: true });
    await waitFor(() => expect(env.store.getState().windows).toContain(SCOPE_DIALOG));
    expect(pimStoreFor(env.store).getState().pendingChange).toMatchObject({ kind: "move" });
    fireEvent.keyDown(standups[0] as HTMLElement, { key: "Delete" });
    await waitFor(() =>
      expect(pimStoreFor(env.store).getState().pendingChange).toMatchObject({ kind: "delete" }),
    );
  });

  it("drags an event to another time and resizes it with the pointer", async () => {
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week" } }), env);
    const lunch = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const columns = document.querySelector<HTMLElement>(
      '[data-part="calendar.columns"]',
    ) as HTMLElement;
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
    await waitFor(() =>
      expect(eventOf(env, "ev-lunch")?.fields.start).toMatchObject({
        local: expect.stringMatching(/T13:30:00$/),
      }),
    );
    // The click that ends a drag does not open the editor.
    fireEvent.click(lunch);
    expect(env.store.getState().windows).not.toContain(EVENT_DIALOG);
    const moved = await screen.findByRole("button", { name: /Lunch with Grace/ });
    const handle = moved.querySelector('[data-part="calendar.event.resize"]') as HTMLElement;
    fireEvent.pointerDown(handle, { button: 0, clientX: 50, clientY: 1450, pointerId: 2 });
    fireEvent.pointerMove(moved, { clientX: 50, clientY: 1500, pointerId: 2 });
    fireEvent.pointerUp(moved, { clientX: 50, clientY: 1500, pointerId: 2 });
    await waitFor(() =>
      expect(eventOf(env, "ev-lunch")?.fields.end).toMatchObject({
        local: expect.stringMatching(/T15:00:00$/),
      }),
    );
  });

  it("opens the editor on an event and moves events between days in the month view", async () => {
    const user = userEvent.setup();
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
    await waitFor(() => expect(eventOf(env, "ev-planning")?.fields.start).not.toEqual(before));
    // Days with more events than fit offer "n more".
    expect(screen.getAllByRole("button", { name: /more\)/ }).length).toBeGreaterThan(0);
    // Arrow keys move between days; Enter shows the day.
    const cell = document.querySelector<HTMLElement>(
      '[data-part="calendar.month.day"][tabindex="0"]',
    ) as HTMLElement;
    act(() => cell.focus());
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
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(
      <>
        <NodeView node={widget("mini-month")} path="t/0" />
        <NodeView node={widget("calendar", { id: "cal", options: { view: "day" } })} path="t/1" />
        <NodeView node={widget("agenda")} path="t/2" />
      </>,
      env,
    );
    await settle(env);
    const grid = screen.getByRole("grid", { name: /\d{4}/ });
    expect(
      within(grid)
        .getAllByRole("gridcell")
        .some((c) => c.getAttribute("data-state")?.includes("busy")),
    ).toBe(true);
    const agenda = document.querySelector('[data-part="agenda"]') as HTMLElement;
    expect(within(agenda).getAllByRole("button", { name: /Lunch with Grace/ })).toHaveLength(1);
    const tomorrow = dateString(todayIn(localZone()).add({ days: 1 }));
    const cell = grid.querySelector<HTMLElement>(`[data-day="${tomorrow}"]`);
    if (cell) {
      await user.click(cell);
      expect(pimStoreFor(env.store).getState().anchor).toBe(tomorrow);
      const calendar = document.querySelector('[data-part="calendar"]') as HTMLElement;
      expect(
        await within(calendar).findByRole("button", { name: /Design review/ }),
      ).toBeInTheDocument();
    }
    // Keyboard: Page Down shows the next month.
    const focusable = grid.querySelector<HTMLElement>('[tabindex="0"]') as HTMLElement;
    act(() => focusable.focus());
    const month = document.querySelector('[data-part="mini-month.title"]')?.textContent;
    await user.keyboard("{PageDown}");
    expect(document.querySelector('[data-part="mini-month.title"]')?.textContent).not.toBe(month);
  });
});

describe("calendar toolbar and event cards (M5b)", () => {
  it("puts Today, previous / next, the range, the view switch and New event on one line", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(
      widget("calendar", {
        options: { view: "week", views: "day week month year agenda free-time" },
      }),
      env,
    );
    await settle(env);
    const toolbar = document.querySelector('[data-part="calendar.toolbar"]') as HTMLElement;
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
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar", { options: { view: "week", "view-switch": "menu" } }), env);
    await settle(env);
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
