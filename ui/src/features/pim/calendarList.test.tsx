/**
 * The M5b calendar list (WP164) against the mock core: groups by source
 * with the subscriptions last, the colour square as the show / hide switch,
 * the per-calendar "…" menu (settings, show / hide, colour, remove) and the
 * per-group "+" menu (new calendar / task list / subscription) with their
 * dialogs instead of inline forms, the `actions` / `show-sources` options,
 * and the settings page's "Calendar & contacts" section.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Calendar, PimSource } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { settings, widgets } from ".";
import {
  CalendarSettingsDialog,
  NewCalendarDialog,
  RemoveCalendarDialog,
  calendarFormIssues,
  unusedColor,
} from "./CalendarDialogs";
import { calendarGroups, isTaskList, removalOf } from "./calendarGroups";
import {
  CALENDAR_SETTINGS_DIALOG,
  IMPORT_DIALOG,
  NEW_CALENDAR_DIALOG,
  REMOVE_CALENDAR_DIALOG,
  SOURCES_DIALOG,
} from "./names";
import { pimStoreFor } from "./store";

const pimOf = (env: Env) => env.transport.data.features.pim;
const windows = (env: Env) => env.store.getState().windows;

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
  });
}

function calendar(id: string, source: string, order: number, extra: Partial<Calendar> = {}) {
  return {
    id,
    source_id: source,
    account_id: null,
    settings: {
      name: id,
      description: null,
      color: null,
      visible: true,
      reminders: true,
      default_alarms: [],
      order,
    },
    components: ["event"],
    read_only: false,
    timezone: null,
    ...extra,
  } satisfies Calendar;
}

function source(id: string, kind: PimSource["kind"]): PimSource {
  return {
    id,
    kind,
    name: id,
    account_id: null,
    enabled: true,
    config: null,
    state: "idle",
    last_sync_at: null,
    error: null,
    message: null,
  };
}

describe("calendar groups", () => {
  const sources = [
    source("dav", "caldav"),
    source("local", "local"),
    source("feed-a", "ics"),
    source("feed-b", "ics"),
    source("ews", "exchange"),
  ];

  it("puts this computer first, each account next and every subscription last", () => {
    const groups = calendarGroups(
      [
        calendar("work", "dav", 0),
        calendar("holidays", "feed-a", 1),
        calendar("personal", "local", 2),
        calendar("school", "feed-b", 3),
        calendar("mailbox", "ews", 4),
      ],
      sources,
    );
    expect(groups.map((g) => g.key)).toEqual([
      "source:local",
      "source:dav",
      "source:ews",
      "subscriptions",
    ]);
    expect(groups.at(-1)?.calendars.map((c) => c.id)).toEqual(["holidays", "school"]);
    // New calendars only where the core can make them (this computer, CalDAV).
    expect(groups.map((g) => g.createIn)).toEqual(["local", "dav", null, null]);
  });

  it("keeps an empty local group for new calendars and one group without sources", () => {
    const groups = calendarGroups([calendar("work", "dav", 0)], sources);
    expect(groups.map((g) => g.key)).toEqual(["source:local", "source:dav"]);
    expect(groups[0]?.calendars).toEqual([]);
    const flat = calendarGroups(
      [calendar("b", "dav", 2), calendar("a", "feed-a", 1)],
      sources,
      false,
    );
    expect(flat).toHaveLength(1);
    expect(flat[0]).toMatchObject({ kind: "all", createIn: "local" });
    expect(flat[0]?.calendars.map((c) => c.id)).toEqual(["a", "b"]);
  });

  it("removes by deleting, unsubscribing or not at all", () => {
    expect(removalOf(calendar("p", "local", 0), sources)).toBe("delete");
    expect(removalOf(calendar("w", "dav", 0), sources)).toBe("delete");
    expect(removalOf(calendar("w", "dav", 0, { read_only: true }), sources)).toBeNull();
    expect(removalOf(calendar("h", "feed-a", 0, { read_only: true }), sources)).toBe("unsubscribe");
    expect(removalOf(calendar("m", "ews", 0), sources)).toBeNull();
    expect(isTaskList(calendar("t", "local", 0, { components: ["task"] }))).toBe(true);
    expect(isTaskList(calendar("t", "local", 0, { components: ["event", "task"] }))).toBe(false);
  });

  it("checks names and colours and picks an unused colour", () => {
    expect(calendarFormIssues(" ", "")).toEqual({ id: "pim-issue-missing-name" });
    expect(calendarFormIssues("Garden", "#12345")).toEqual({ id: "pim-calendar-color-invalid" });
    expect(calendarFormIssues("Garden", "red;x")).toEqual({ id: "pim-calendar-color-invalid" });
    expect(calendarFormIssues("Garden", "")).toBeNull();
    expect(calendarFormIssues("Garden", "#3b82f6")).toBeNull();
    const used = calendar("x", "local", 0);
    used.settings.color = "#2563EB";
    expect(unusedColor([used])).toBe("#0891b2");
  });
});

describe("calendar list widget", () => {
  it("groups calendars, shows colour squares and hides a calendar with its square", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar-list"), env);
    const work = await screen.findByRole("checkbox", { name: /^Work/ });
    const groups = [...document.querySelectorAll('[data-part="calendar-list.source"]')];
    expect(
      groups.map(
        (g) => /^(On this computer|Work calendars|Subscriptions)/.exec(g.textContent)?.[1],
      ),
    ).toEqual(["On this computer", "Work calendars", "Subscriptions"]);
    // No inline form and no text buttons: menus only.
    expect(document.querySelector('[data-part="calendar-list.new"]')).toBeNull();
    expect(screen.queryByRole("button", { name: "Create" })).toBeNull();
    expect(screen.getByRole("button", { name: "Add to On this computer" })).toHaveAttribute(
      "aria-haspopup",
      "menu",
    );
    expect(work).toHaveAttribute("data-part", "calendar-list.toggle");
    expect(
      screen.getByText("Tasks", { selector: '[data-part="calendar-list.kind"]' }),
    ).toBeVisible();
    await user.click(work);
    await waitFor(() => {
      expect(pimOf(env).calendars.find((c) => c.id === "cal-work")?.settings.visible).toBe(false);
    });
    await waitFor(() =>
      expect(work.closest('[data-part="calendar-list.item"]')).toHaveAttribute(
        "data-state",
        "hidden",
      ),
    );
  });

  it("opens the … menu from the keyboard, recolours and shows a hidden calendar", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    for (const c of pimOf(env).calendars) if (c.id === "cal-personal") c.settings.visible = false;
    renderNode(widget("calendar-list"), env);
    const button = await screen.findByRole("button", { name: "Actions for Personal" });
    button.focus();
    await user.keyboard("{ArrowDown}");
    const menu = screen.getByRole("menu", { name: "Actions for Personal" });
    expect(within(menu).getByRole("menuitem", { name: "Settings…" })).toHaveFocus();
    expect(within(menu).getByRole("menuitem", { name: "Remove…" })).toBeInTheDocument();
    // Arrow keys move, Escape closes and returns to the button.
    await user.keyboard("{ArrowDown}");
    expect(within(menu).getByRole("menuitem", { name: "Show in the calendar" })).toHaveFocus();
    await user.keyboard("{End}");
    expect(within(menu).getByRole("menuitem", { name: "Remove…" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(button).toHaveFocus();
    expect(button).toHaveAttribute("aria-expanded", "false");

    await user.click(button);
    const colours = within(screen.getByRole("menu")).getAllByRole("menuitemradio");
    expect(colours.map((c) => c.textContent)).toEqual([
      "Blue",
      "Cyan",
      "Green",
      "Amber",
      "Orange",
      "Red",
      "Pink",
      "Purple",
      "Slate",
    ]);
    await user.click(screen.getByRole("menuitemradio", { name: "Purple" }));
    await waitFor(() => {
      expect(pimOf(env).calendars.find((c) => c.id === "cal-personal")?.settings.color).toBe(
        "#9333ea",
      );
    });
    await user.click(screen.getByRole("button", { name: "Actions for Personal" }));
    expect(screen.getByRole("menuitemradio", { name: "Purple" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await user.click(screen.getByRole("menuitem", { name: "Show in the calendar" }));
    await waitFor(() => {
      expect(pimOf(env).calendars.find((c) => c.id === "cal-personal")?.settings.visible).toBe(
        true,
      );
    });
  });

  it("closes a menu on a click elsewhere and offers unsubscribing for a subscription", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar-list"), env);
    await user.click(await screen.findByRole("button", { name: "Actions for Public holidays" }));
    expect(screen.getByRole("menuitem", { name: "Unsubscribe…" })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: "Remove…" })).toBeNull();
    await user.click(document.body);
    expect(screen.queryByRole("menu")).toBeNull();
    // The subscriptions' "+" menu: no new calendars there.
    await user.click(screen.getByRole("button", { name: "Add to Subscriptions" }));
    expect(screen.queryByRole("menuitem", { name: "New calendar" })).toBeNull();
    await user.click(screen.getByRole("menuitem", { name: "New subscription…" }));
    expect(windows(env)).toContain(SOURCES_DIALOG);
    await user.click(screen.getByRole("button", { name: "Add to Work calendars" }));
    await user.click(screen.getByRole("menuitem", { name: "Import…" }));
    expect(windows(env)).toContain(IMPORT_DIALOG);
  });

  it("creates a task list in the group's source through the + menu and its dialog", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar-list"), env);
    await user.click(await screen.findByRole("button", { name: "Add to Work calendars" }));
    await user.click(screen.getByRole("menuitem", { name: "New task list" }));
    expect(windows(env)).toContain(NEW_CALENDAR_DIALOG);
    expect(pimStoreFor(env.store).getState().newCalendar).toEqual({
      sourceId: "work-calendars",
      component: "task",
    });
    const close = vi.fn();
    renderWithProviders(<NewCalendarDialog name={NEW_CALENDAR_DIALOG} close={close} />, env);
    expect(await screen.findByText("A new task list in Work calendars")).toBeInTheDocument();
    // A name is required; a broken colour is refused before asking the core.
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("A name is required.");
    await user.type(screen.getByLabelText("Name"), "Errands");
    await user.type(screen.getByLabelText("Other colour (for example #3b82f6)"), "nope!");
    await user.click(screen.getByRole("button", { name: "Create" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/Enter a colour/);
    await user.click(screen.getByRole("radio", { name: "Green" }));
    expect(screen.getByLabelText("Other colour (for example #3b82f6)")).toHaveValue("");
    await user.click(screen.getByRole("button", { name: "Create" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(pimOf(env).calendars.find((c) => c.settings.name === "Errands")).toMatchObject({
      source_id: "work-calendars",
      components: ["task"],
      settings: { color: "#16a34a", visible: true, reminders: true },
    });
  });

  it("edits a calendar's settings in the settings dialog", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar-list"), env);
    await user.click(await screen.findByRole("button", { name: "Actions for Work" }));
    await user.click(screen.getByRole("menuitem", { name: "Settings…" }));
    expect(windows(env)).toContain(CALENDAR_SETTINGS_DIALOG);
    const close = vi.fn();
    renderWithProviders(
      <CalendarSettingsDialog name={CALENDAR_SETTINGS_DIALOG} close={close} />,
      env,
    );
    const form = await screen.findByRole("form", { name: "Settings of Work" });
    const name = within(form).getByLabelText("Name");
    expect(name).toHaveValue("Work");
    // #ef4444 is not in the palette: it shows as the other colour.
    expect(within(form).getByLabelText("Other colour (for example #3b82f6)")).toHaveValue(
      "#ef4444",
    );
    await user.clear(name);
    await user.type(name, "Office");
    await user.click(within(form).getByRole("checkbox", { name: "Show reminders" }));
    await user.click(within(form).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(pimOf(env).calendars.find((c) => c.id === "cal-work")?.settings).toMatchObject({
      name: "Office",
      color: "#ef4444",
      reminders: false,
    });
  });

  it("removes a local calendar and unsubscribes from a subscription after asking", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("calendar-list"), env);
    await user.click(await screen.findByRole("button", { name: "Actions for Personal" }));
    await user.click(screen.getByRole("menuitem", { name: "Remove…" }));
    expect(windows(env)).toContain(REMOVE_CALENDAR_DIALOG);
    const close = vi.fn();
    const { unmount } = renderWithProviders(
      <RemoveCalendarDialog name={REMOVE_CALENDAR_DIALOG} close={close} />,
      env,
    );
    expect(
      await screen.findByText(/Remove the calendar “Personal” with all its events\?/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Remove" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(pimOf(env).calendars.some((c) => c.id === "cal-personal")).toBe(false);
    unmount();

    act(() => {
      pimStoreFor(env.store).getState().setRemoveCalendar("cal-holidays");
    });
    renderWithProviders(<RemoveCalendarDialog name={REMOVE_CALENDAR_DIALOG} close={close} />, env);
    expect(await screen.findByText(/Unsubscribe from “Public holidays”\?/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Unsubscribe" }));
    await waitFor(() => {
      expect(pimOf(env).sources.some((s) => s.id === "holidays")).toBe(false);
    });
  });

  it("says when the calendar to remove is gone", async () => {
    const env = createEnv();
    pimStoreFor(env.store).getState().setRemoveCalendar("cal-nope");
    renderWithProviders(
      <RemoveCalendarDialog name={REMOVE_CALENDAR_DIALOG} close={() => undefined} />,
      env,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("This calendar no longer exists.");
    expect(screen.queryByRole("button", { name: "Remove" })).toBeNull();
  });

  it("shows settings and remove as buttons and one list without source headers", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(
      widget("calendar-list", { options: { actions: "buttons", "show-sources": false } }),
      env,
    );
    await settle(env);
    expect(screen.queryByRole("button", { name: /^Actions for/ })).toBeNull();
    const groups = document.querySelectorAll('[data-part="calendar-list.source"]');
    expect(groups).toHaveLength(1);
    expect(screen.getByRole("group", { name: "Calendars" })).toBe(groups[0]);
    // A subscription's remove button unsubscribes (it asks first).
    expect(screen.getByRole("button", { name: "Remove Personal" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Remove Public holidays" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Settings of Tasks" }));
    expect(pimStoreFor(env.store).getState().calendarSettingsId).toBe("list-tasks");
    expect(windows(env)).toContain(CALENDAR_SETTINGS_DIALOG);
  });
});

describe("calendar & contacts settings section", () => {
  it("registers the section widget with labels that all exist", async () => {
    expect(widgets["pim-settings"]).toBeDefined();
    expect(Object.keys(settings).sort()).toEqual(["calendar-list", "pim-settings"]);
    const env = createEnv();
    renderNode(widget("pim-settings"), env);
    await settle(env);
    expect(screen.getByRole("heading", { name: "Calendars and task lists" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Subscriptions and servers" })).toBeInTheDocument();
    // The sources panel without its dialog's Close button; settings as buttons.
    expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
    expect(screen.getByRole("button", { name: "Settings of Personal" })).toBeInTheDocument();
    await userEvent.setup().click(screen.getByRole("button", { name: "Import…" }));
    expect(windows(env)).toContain(IMPORT_DIALOG);
  });
});
