/**
 * The PIM editors and dialogs against the mock core: the event editor
 * (new events, recurrence, attendees, alarms, zones, occurrence scopes,
 * delete), the date / time picker in the UI locale, the scope dialog,
 * contacts (list, card, editor, groups, merge), tasks (list, editor,
 * subtasks), reminder toasts, import and sources.
 */
import { CalendarDate, resetLocalTimeZone, setLocalTimeZone } from "@internationalized/date";
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { GroupsDialog, MergeDialog } from "./ContactDialogs";
import { ContactEditorDialog } from "./ContactEditor";
import { DateTimeInput } from "./DateTimeInput";
import { EventEditorDialog } from "./EventEditor";
import { ImportDialog } from "./ImportDialog";
import { handlePimEvent } from ".";
import { fireReminder } from "./mock";
import { CONTACT_DIALOG, TASK_DIALOG } from "./names";
import { PimToasts } from "./Reminders";
import { ScopeDialog } from "./ScopeDialog";
import { SourcesDialog } from "./Sources";
import { pimStoreFor } from "./store";
import { TaskEditorDialog } from "./TaskEditor";
import { localZone, makePimTime, todayIn } from "./time";
import { stringMatching } from "../../test/matchers";

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error("missing test data");
  return value;
}

const BERLIN = "Europe/Berlin";
const START_LABEL = "Start";
beforeAll(() => {
  setLocalTimeZone(BERLIN);
});
afterAll(() => {
  resetLocalTimeZone();
});

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
  });
}

const pimOf = (env: Env) => env.transport.data.features.pim;

describe("date and time picker", () => {
  function Picker({ onChange }: { onChange: (day: string, time: string) => void }) {
    const [day, setDay] = useState(new CalendarDate(2026, 10, 1));
    const [time, setTime] = useState<{ hour: number; minute: number } | null>({
      hour: 14,
      minute: 30,
    });
    return (
      <DateTimeInput
        label={START_LABEL}
        field="start"
        day={day}
        time={time}
        tz={BERLIN}
        onChange={(d, t) => {
          setDay(d);
          setTime(t);
          onChange(d.toString(), t ? `${String(t.hour)}:${String(t.minute)}` : "");
        }}
      />
    );
  }

  it("follows the UI locale, not the browser's, and picks days from a month grid", async () => {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(["en-US"]);
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const changes: string[] = [];
    renderWithProviders(<Picker onChange={(d, t) => changes.push(`${d} ${t}`)} />, env, {
      locale: "de",
    });
    const segments = screen.getAllByRole("spinbutton");
    // German order (day, month, year) and a 24-hour clock.
    expect(segments.map((s) => s.getAttribute("data-type"))).toEqual([
      "day",
      "month",
      "year",
      "hour",
      "minute",
    ]);
    expect(segments[3]).toHaveTextContent("14");
    await user.click(must(segments[0]));
    await user.keyboard("{ArrowUp}");
    expect(changes.at(-1)).toBe("2026-10-02 14:30");
    await user.click(screen.getByRole("button", { name: /Choose the date for Start/ }));
    const popup = screen.getByRole("dialog");
    const grid = within(popup).getByRole("grid");
    // Weeks start on Monday in German.
    expect(within(grid).getAllByRole("columnheader")[0]).toHaveAttribute("abbr", "Montag");
    await waitFor(() => expect(document.activeElement).toHaveAttribute("data-day", "2026-10-02"));
    await user.keyboard("{ArrowDown}{Enter}");
    expect(changes.at(-1)).toBe("2026-10-09 14:30");
    expect(screen.queryByRole("dialog")).toBeNull();
    vi.restoreAllMocks();
  });

  it("uses a 12-hour clock in English", () => {
    const env = createEnv();
    renderWithProviders(<Picker onChange={() => undefined} />, env, { locale: "en" });
    const types = screen.getAllByRole("spinbutton").map((s) => s.getAttribute("data-type"));
    expect(types).toEqual(["month", "day", "year", "hour", "minute", "dayPeriod"]);
  });
});

describe("event editor", () => {
  it("creates a weekly event with an attendee, a reminder and another zone", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const close = vi.fn();
    const day = todayIn(localZone()).add({ days: 2 });
    pimStoreFor(env.store)
      .getState()
      .editEvent({
        mode: "new",
        start: makePimTime(day, { hour: 9, minute: 0 }, BERLIN),
        end: makePimTime(day, { hour: 10, minute: 0 }, BERLIN),
        calendarId: null,
        fields: null,
      });
    renderWithProviders(<EventEditorDialog name="pim-event-editor" close={close} />, env);
    const title = await screen.findByLabelText("Title");
    await user.type(title, "Yoga");
    await user.selectOptions(screen.getByLabelText("Repeats"), "weekly");
    expect(screen.getByText(/^Every week on/)).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "After a number of times" }));
    await user.clear(screen.getByLabelText("Number of times"));
    await user.type(screen.getByLabelText("Number of times"), "4");
    expect(screen.getByText(/4 times\.$/)).toBeInTheDocument();
    await user.type(screen.getByLabelText("Address"), "grace@navy.example");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getAllByText("grace@navy.example").length).toBeGreaterThan(0);
    await user.click(screen.getByRole("button", { name: "Add a reminder" }));
    await user.selectOptions(screen.getByLabelText("Also show the times in"), "America/New_York");
    expect(screen.getAllByText(/^In America\/New York:/)).toHaveLength(2);
    await user.selectOptions(screen.getByLabelText("Time zone"), "Europe/London");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    const created = pimOf(env).events.find((e) => e.fields.title === "Yoga");
    expect(created).toMatchObject({
      calendar_id: "cal-personal",
      fields: {
        start: { type: "zoned", tz: "Europe/London" },
        attendees: [{ email: "grace@navy.example", role: "required" }],
      },
      recurrence: { rules: [{ freq: "weekly", end: { type: "count", count: 4 } }] },
    });
    // The calendar's default reminder (15 minutes) plus the added one.
    expect(created?.fields.alarms).toHaveLength(2);
  });

  it("refuses an end before the start before asking the core", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const day = todayIn(localZone());
    pimStoreFor(env.store)
      .getState()
      .editEvent({
        mode: "new",
        start: makePimTime(day, { hour: 10, minute: 0 }, BERLIN),
        end: makePimTime(day, { hour: 9, minute: 0 }, BERLIN),
        calendarId: "cal-personal",
        fields: null,
      });
    renderWithProviders(<EventEditorDialog name="pim-event-editor" close={() => undefined} />, env);
    await user.click(await screen.findByRole("button", { name: "Save" }));
    expect(screen.getByRole("alert")).toHaveTextContent("The end is before the start.");
  });

  it("edits one occurrence, this and following, or all of a series", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    await settle(env);
    const instances = (
      await env.client.request(
        {
          type: "ListEventInstances",
          from: "2020-01-01T00:00:00Z",
          to: "2040-01-01T00:00:00Z",
          calendar_ids: ["cal-work"],
          tz: BERLIN,
        },
        "EventInstances",
      )
    ).instances.filter((i) => i.event_id === "ev-standup");
    const occurrence = must(instances[3]);
    const open = () => {
      pimStoreFor(env.store).getState().editEvent({
        mode: "edit",
        eventId: "ev-standup",
        recurrenceId: occurrence.recurrence_id,
        start: occurrence.start,
        end: occurrence.end,
      });
    };

    open();
    const close = vi.fn();
    const { unmount } = renderWithProviders(
      <EventEditorDialog name="pim-event-editor" close={close} />,
      env,
    );
    expect(await screen.findByRole("radio", { name: "This occurrence" })).toBeChecked();
    // Changing how a series repeats is not possible for one occurrence.
    expect(screen.getByLabelText("Repeats")).toBeDisabled();
    await user.clear(screen.getByLabelText("Title"));
    await user.type(screen.getByLabelText("Title"), "Retro");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(pimOf(env).events.find((e) => e.id === "ev-standup")?.overrides).toHaveLength(1);
    unmount();

    open();
    const close2 = vi.fn();
    const second = renderWithProviders(
      <EventEditorDialog name="pim-event-editor" close={close2} />,
      env,
    );
    await user.click(await screen.findByRole("radio", { name: "This and following occurrences" }));
    await user.clear(screen.getByLabelText("Location"));
    await user.type(screen.getByLabelText("Location"), "Room 9");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close2).toHaveBeenCalled();
    });
    const split = pimOf(env).events.filter(
      (e) => e.id !== "ev-standup" && e.fields.location === "Room 9",
    );
    expect(split).toHaveLength(1);
    expect(
      pimOf(env).events.find((e) => e.id === "ev-standup")?.recurrence?.rules[0]?.end.type,
    ).toBe("until");
    second.unmount();

    pimStoreFor(env.store)
      .getState()
      .editEvent({
        mode: "edit",
        eventId: "ev-standup",
        recurrenceId: must(instances[0]).recurrence_id,
        start: must(instances[0]).start,
        end: must(instances[0]).end,
      });
    const close3 = vi.fn();
    renderWithProviders(<EventEditorDialog name="pim-event-editor" close={close3} />, env);
    await user.click(await screen.findByRole("radio", { name: "All occurrences" }));
    await user.click(screen.getByRole("button", { name: "Delete all occurrences" }));
    await waitFor(() => {
      expect(close3).toHaveBeenCalled();
    });
    expect(pimOf(env).events.find((e) => e.id === "ev-standup")).toBeUndefined();
  });

  it("shows read-only events without saving", async () => {
    const env = createEnv();
    pimStoreFor(env.store).getState().editEvent({
      mode: "edit",
      eventId: "ev-holiday",
      recurrenceId: null,
      start: null,
      end: null,
    });
    renderWithProviders(<EventEditorDialog name="pim-event-editor" close={() => undefined} />, env);
    expect(await screen.findByText("This item is read-only.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save" })).toBeNull();
  });

  it("applies the scope chosen for a dragged recurring occurrence", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const instances = (
      await env.client.request(
        {
          type: "ListEventInstances",
          from: "2020-01-01T00:00:00Z",
          to: "2040-01-01T00:00:00Z",
          calendar_ids: ["cal-work"],
          tz: BERLIN,
        },
        "EventInstances",
      )
    ).instances.filter((i) => i.event_id === "ev-standup");
    const instance = must(instances[2]);
    const close = vi.fn();
    pimStoreFor(env.store)
      .getState()
      .setPendingChange({
        kind: "move",
        instance,
        start: {
          ...instance.start,
          local:
            instance.start.type === "date" ? "" : instance.start.local.replace("09:30", "11:00"),
        } as typeof instance.start,
        end: {
          ...instance.end,
          local: instance.end.type === "date" ? "" : instance.end.local.replace("09:45", "11:15"),
        } as typeof instance.end,
      });
    renderWithProviders(<ScopeDialog name="pim-edit-scope" close={close} />, env);
    expect(screen.getByText(/^Move “Stand-up” to/)).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "This occurrence" })).toBeChecked();
    await user.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(
      pimOf(env).events.find((e) => e.id === "ev-standup")?.overrides[0]?.fields.start,
    ).toMatchObject({
      local: stringMatching(/T11:00:00$/),
    });
    expect(pimStoreFor(env.store).getState().pendingChange).toBeNull();
  });
});

describe("contacts", () => {
  it("lists, searches, selects and shows contacts with linked ones", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(
      <>
        <NodeView node={widget("contact-list")} path="t/0" />
        <NodeView node={widget("contact-card")} path="t/1" />
      </>,
      env,
    );
    const list = await screen.findByRole("listbox", { name: "Contacts" });
    expect(within(list).getAllByRole("option")).toHaveLength(6);
    expect(screen.getByText("Choose a contact to see it here.")).toBeInTheDocument();
    await user.click(within(list).getByRole("option", { name: /Ada Lovelace/ }));
    expect(await screen.findByRole("heading", { name: "Ada Lovelace" })).toBeInTheDocument();
    expect(await screen.findByRole("img", { name: "Photo of Ada Lovelace" })).toHaveAttribute(
      "src",
      expect.stringMatching(/^data:image\/svg\+xml;base64,/),
    );
    expect(screen.getByText("12 St James's Square")).toBeInTheDocument();
    expect(screen.getByText("X-FAVOURITE-ENGINE")).toBeInTheDocument();
    // Keyboard: Down shows the next contact, Shift+Down extends the selection.
    // Sorted by last name: Ada Lovelace follows Linus.
    await user.keyboard("{ArrowUp}");
    expect(pimStoreFor(env.store).getState().contactId).not.toBe("ct-ada");
    await user.keyboard("{Shift>}{ArrowUp}{/Shift}");
    expect(pimStoreFor(env.store).getState().selectedContacts).toHaveLength(2);
    await user.type(screen.getByLabelText("Search contacts"), "navy");
    await waitFor(() => {
      expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(2);
    });
    await user.selectOptions(screen.getByLabelText("Group"), "grp-team");
    await waitFor(() => {
      expect(within(screen.getByRole("listbox")).getAllByRole("option")).toHaveLength(1);
    });
  });

  it("creates and edits a contact in the editor", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const close = vi.fn();
    pimStoreFor(env.store)
      .getState()
      .editContact({ mode: "new", addressBookId: null, fields: null });
    const { unmount } = renderWithProviders(
      <ContactEditorDialog name={CONTACT_DIALOG} close={close} />,
      env,
    );
    await user.type(await screen.findByLabelText("First name"), "Margaret");
    await user.type(screen.getByLabelText("Last name"), "Hamilton");
    await user.click(screen.getByRole("button", { name: "Add (Email)" }));
    await user.type(screen.getByLabelText("Email 1"), "margaret@mit.example");
    await user.click(screen.getByRole("button", { name: "Add an address" }));
    await user.type(screen.getByLabelText("City"), "Boston");
    await user.click(screen.getByRole("checkbox", { name: "Birthday is known" }));
    await user.click(screen.getByRole("checkbox", { name: "The year is not known" }));
    await user.click(screen.getByRole("button", { name: "Add a field" }));
    await user.type(screen.getByLabelText("Field 1: name"), "X-SHIP");
    await user.type(screen.getByLabelText("Field 1: value"), "Apollo");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    const created = pimOf(env).contacts.find((c) => c.fields.display_name === "Margaret Hamilton");
    expect(created).toMatchObject({
      address_book_id: "ab-personal",
      fields: {
        emails: [{ value: "margaret@mit.example", kinds: ["home"], preferred: true }],
        addresses: [{ locality: "Boston" }],
        birthday: { year: null },
        custom: [{ name: "X-SHIP", value: "Apollo" }],
      },
    });
    expect(pimStoreFor(env.store).getState().contactId).toBe(created?.id);
    unmount();

    pimStoreFor(env.store).getState().editContact({ mode: "edit", contactId: "ct-linus" });
    const close2 = vi.fn();
    renderWithProviders(<ContactEditorDialog name={CONTACT_DIALOG} close={close2} />, env);
    await user.selectOptions(
      await screen.findByLabelText(/Address book \(choose another/),
      "ab-personal",
    );
    await user.clear(screen.getByLabelText("Shown as"));
    await user.type(screen.getByLabelText("Shown as"), "Linus T.");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close2).toHaveBeenCalled();
    });
    expect(pimOf(env).contacts.find((c) => c.id === "ct-linus")).toMatchObject({
      address_book_id: "ab-personal",
      fields: { display_name: "Linus T." },
    });
  });

  it("refuses a contact without any name", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    pimStoreFor(env.store)
      .getState()
      .editContact({ mode: "new", addressBookId: "ab-personal", fields: null });
    renderWithProviders(<ContactEditorDialog name={CONTACT_DIALOG} close={() => undefined} />, env);
    await user.click(await screen.findByRole("button", { name: "Save" }));
    expect(screen.getByRole("alert")).toHaveTextContent("A name is required.");
  });

  it("merges duplicates: folds the same book, links the others", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const close = vi.fn();
    renderWithProviders(<MergeDialog name="pim-merge-contacts" close={close} />, env);
    const groups = await screen.findByRole("list", { name: "Possible duplicates" });
    expect(await within(groups).findByText("Grace Hopper, Grace B. Hopper")).toBeInTheDocument();
    expect(within(groups).getByText("Same email address, name")).toBeInTheDocument();
    await user.click(within(groups).getByRole("button", { name: "Merge these" }));
    expect(
      await screen.findByText(/One contact in another address book stays there, linked to it\./),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("radio", { name: "Grace B. Hopper" }));
    await user.click(screen.getByRole("button", { name: "Merge 2 contacts" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    const grace = pimOf(env).contacts.find((c) => c.id === "ct-grace");
    expect(grace).toMatchObject({
      linked_ids: ["ct-grace-work"],
      fields: { display_name: "Grace B. Hopper" },
    });
    expect(grace?.fields.phones.map((p) => p.value)).toEqual(["+1 555 0100"]);
    expect(pimStoreFor(env.store).getState().contactId).toBe("ct-grace");
  });

  it("edits groups and their members", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(<GroupsDialog name="pim-contact-groups" close={() => undefined} />, env);
    await user.click(await screen.findByRole("button", { name: "Family (2 members)" }));
    await user.click(await screen.findByRole("checkbox", { name: "Ada Lovelace" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Group saved.")).toBeInTheDocument();
    expect(pimOf(env).groups.find((g) => g.id === "grp-family")?.member_ids).toEqual([
      "ct-grace",
      "ct-ada",
    ]);
    await user.type(must(screen.getAllByLabelText("Group name").at(-1)), "Book club");
    await user.click(screen.getByRole("button", { name: "Create group" }));
    await waitFor(() => {
      expect(pimOf(env).groups.some((g) => g.name === "Book club")).toBe(true);
    });
  });
});

describe("tasks", () => {
  it("shows tasks with subtasks, completes, collapses and adds tasks", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderNode(widget("task-list"), env);
    const tree = await screen.findByRole("tree", { name: "Tasks" });
    const report = within(tree).getByRole("treeitem", { name: /^Write the quarterly report/ });
    expect(report).toHaveAttribute("aria-expanded", "true");
    expect(within(tree).getByRole("treeitem", { name: /^Draft the text/ })).toHaveAttribute(
      "aria-level",
      "2",
    );
    // Completed subtasks are hidden unless "Show completed".
    expect(within(tree).queryByRole("treeitem", { name: /^Collect the numbers/ })).toBeNull();
    await user.click(screen.getByRole("checkbox", { name: "Show completed" }));
    expect(
      await within(screen.getByRole("tree")).findByRole("treeitem", {
        name: /^Collect the numbers, done/,
      }),
    ).toBeInTheDocument();
    // Keyboard: Left collapses, Space completes.
    act(() => {
      report.focus();
    });
    await user.keyboard("{ArrowLeft}");
    expect(screen.getByRole("treeitem", { name: /^Write the quarterly report/ })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    await user.click(screen.getByRole("checkbox", { name: "Mark “Buy groceries” as done" }));
    await waitFor(() => {
      expect(pimOf(env).tasks.find((t) => t.id === "tk-groceries")?.fields.status).toBe(
        "completed",
      );
    });
    await user.type(screen.getByLabelText(/^Add a task to/), "Call the bank");
    await user.click(screen.getByRole("button", { name: "Add" }));
    await waitFor(() => {
      expect(pimOf(env).tasks.some((t) => t.fields.title === "Call the bank")).toBe(true);
    });
    // Enter opens the editor.
    const item = await screen.findByRole("treeitem", { name: /^Call the bank/ });
    act(() => {
      item.focus();
    });
    await user.keyboard("{Enter}");
    expect(env.store.getState().windows).toContain(TASK_DIALOG);
  });

  it("edits a task with due date, priority, recurrence and subtasks", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const close = vi.fn();
    pimStoreFor(env.store).getState().editTask({ mode: "edit", taskId: "tk-groceries" });
    renderWithProviders(<TaskEditorDialog name={TASK_DIALOG} close={close} />, env);
    await user.selectOptions(await screen.findByLabelText("Priority"), "high");
    await user.selectOptions(screen.getByLabelText("Repeats"), "weekly");
    await user.type(screen.getByLabelText("New subtask"), "Milk{Enter}");
    await waitFor(() => {
      expect(
        pimOf(env).tasks.some(
          (t) => t.fields.title === "Milk" && t.fields.parent_id === "tk-groceries",
        ),
      ).toBe(true);
    });
    expect(await screen.findByRole("checkbox", { name: "Milk" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(pimOf(env).tasks.find((t) => t.id === "tk-groceries")).toMatchObject({
      fields: { priority: 1, due: { type: "date" } },
      recurrence: { rules: [{ freq: "weekly" }] },
    });
  });

  it("creates a subtask-less task and refuses a recurrence without a date", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const close = vi.fn();
    pimStoreFor(env.store)
      .getState()
      .editTask({ mode: "new", listId: "list-tasks", parentId: "tk-report", fields: null });
    renderWithProviders(<TaskEditorDialog name={TASK_DIALOG} close={close} />, env);
    expect(await screen.findByText("Set a due date to make the task repeat.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Title"), "Check the figures");
    expect(screen.getByLabelText("Subtask of")).toHaveValue("tk-report");
    await user.click(screen.getByRole("checkbox", { name: "Due date set" }));
    await user.click(screen.getByRole("checkbox", { name: "At a time of day" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(pimOf(env).tasks.find((t) => t.fields.title === "Check the figures")).toMatchObject({
      fields: { parent_id: "tk-report", due: { type: "zoned", tz: BERLIN } },
    });
  });
});

describe("reminders", () => {
  it("shows a fired reminder as a toast to snooze or dismiss", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(<PimToasts />, env);
    // The app shell passes every core event to the feature's handlers.
    const off = env.transport.onEvent((event) => {
      handlePimEvent(event, { client: env.client, queryClient: env.queryClient, store: env.store });
    });
    act(() => {
      fireReminder(env.transport, "rem-review");
    });
    const toast = await screen.findByRole("group", { name: "Reminder: Design review" });
    expect(within(toast).getByRole("alert")).toHaveTextContent("Design review");
    await user.selectOptions(within(toast).getByLabelText("Remind me again in"), "30");
    await user.click(within(toast).getByRole("button", { name: "Snooze" }));
    await waitFor(() => {
      expect(pimOf(env).reminders[0]?.state).toBe("snoozed");
    });
    expect(screen.queryByRole("group", { name: "Reminder: Design review" })).toBeNull();
    act(() => {
      fireReminder(env.transport, "rem-review");
    });
    await user.click(await screen.findByRole("button", { name: "Dismiss" }));
    await waitFor(() => {
      expect(pimOf(env).reminders[0]?.state).toBe("dismissed");
    });
    off();
  });

  it("shows reminders that fired before the UI started", async () => {
    const env = createEnv();
    must(pimOf(env).reminders[0]).state = "fired";
    renderWithProviders(<PimToasts />, env);
    expect(
      await screen.findByRole("group", { name: "Reminder: Design review" }),
    ).toBeInTheDocument();
  });
});

describe("import and sources", () => {
  it("previews and imports an uploaded vCard file", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(<ImportDialog name="pim-import" close={() => undefined} />, env);
    const file = new File(
      ["BEGIN:VCARD\nVERSION:3.0\nFN:Zed Shaw\nEMAIL:zed@example.org\nEND:VCARD\n"],
      "zed.vcf",
      { type: "text/vcard" },
    );
    await user.upload(screen.getByLabelText("File"), file);
    expect(
      await screen.findByText("zed.vcf: events: 0, tasks: 0, contacts: 1, groups: 0."),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Import contacts into")).toHaveValue("ab-personal");
    await user.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Imported: 1 new, 0 updated, 0 skipped.",
    );
    expect(pimOf(env).contacts.some((c) => c.fields.display_name === "Zed Shaw")).toBe(true);
  });

  it("previews and imports a file Nail was opened with", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const send = env.transport.send.bind(env.transport);
    const sent: unknown[] = [];
    env.transport.send = async (request) => {
      sent.push(request);
      if (request.type === "PreviewPimImport" && request.source.type === "file") {
        return {
          type: "PimImportPreview",
          preview: {
            format: "icalendar",
            name: null,
            events: 2,
            tasks: 0,
            contacts: 0,
            groups: 0,
            sample: ["Kick-off", "Review"],
            issues: [],
          },
        };
      }
      if (request.type === "ImportPim" && request.source.type === "file") {
        return {
          type: "PimImported",
          result: { created: 2, updated: 0, skipped: 0, issues: [], op_ids: [] },
        };
      }
      return send(request);
    };
    pimStoreFor(env.store).getState().setImportPath("C:\\Downloads\\team.ics");
    renderWithProviders(<ImportDialog name="pim-import" close={() => undefined} />, env);
    expect(
      await screen.findByText("team.ics: events: 2, tasks: 0, contacts: 0, groups: 0."),
    ).toBeInTheDocument();
    // Taken once: reopening the dialog does not import the file again.
    expect(pimStoreFor(env.store).getState().importPath).toBeNull();
    await user.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Imported: 2 new, 0 updated, 0 skipped.",
    );
    expect(sent).toContainEqual(
      expect.objectContaining({
        type: "ImportPim",
        source: { type: "file", path: "C:\\Downloads\\team.ics" },
      }),
    );
  });

  it("fills in a webcal link Nail was opened with", async () => {
    const env = createEnv();
    pimStoreFor(env.store).getState().setSubscribeUrl("webcal://example.org/holidays.ics");
    renderWithProviders(<SourcesDialog name="pim-sources" close={() => undefined} />, env);
    expect(await screen.findByLabelText("Address of the calendar feed")).toHaveValue(
      "webcal://example.org/holidays.ics",
    );
    await waitFor(() => {
      expect(pimStoreFor(env.store).getState().subscribeUrl).toBeNull();
    });
  });

  it("adds a calendar subscription after testing it, syncs and removes sources", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(<SourcesDialog name="pim-sources" close={() => undefined} />, env);
    const list = await screen.findByRole("list", { name: "Sources" });
    expect(await within(list).findByText("On this computer")).toBeInTheDocument();
    expect(within(list).getAllByText("Up to date")).toHaveLength(4);
    await user.type(screen.getByLabelText("Name"), "School holidays");
    await user.type(
      screen.getByLabelText("Address of the calendar feed"),
      "webcals://school.example/h.ics",
    );
    await user.click(screen.getByRole("button", { name: "Test" }));
    expect(
      await screen.findByText("Connected. Found one calendar or address book."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(await screen.findByText("Added. Nail is syncing it now.")).toBeInTheDocument();
    expect(env.transport.data.accountsConfig.pim_sources.map((s) => s.id)).toContain(
      "school-holidays",
    );
    await user.click(await screen.findByRole("button", { name: "Remove School holidays" }));
    await waitFor(() => {
      expect(pimOf(env).sources.some((s) => s.id === "school-holidays")).toBe(false);
    });
    await user.selectOptions(screen.getByLabelText("Kind"), "caldav");
    await user.selectOptions(screen.getByLabelText("Use the login of"), "");
    expect(screen.getByLabelText("Password")).toHaveAttribute("type", "password");
    fireEvent.click(screen.getByRole("button", { name: "Sync everything" }));
    await waitFor(() => {
      expect(pimOf(env).sources.some((s) => s.state === "syncing")).toBe(true);
    });
  });
});
