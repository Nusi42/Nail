/**
 * The compact event editor and the reminder toast (R7 S3 / C3, ADR 0246)
 * against the mock core: the large title, the one-line time row with the
 * day as text and the length, the option chips (repeat, reminder,
 * calendar, busy, call link), "More options", the readable date picker,
 * and the toast's relative time, calendar and split snooze button.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../../test/harness";
import { violations } from "../../test/axe";
import { clickAndClose, inTimeZone } from "../../test/env";
import { BERLIN } from "../../test/mockIds";
import { must } from "../../test/must";
import { DateTimeInput } from "./DateTimeInput";
import { EventEditorDialog } from "./EventEditor";
import { fireReminder } from "./mock";
import { PimToasts, reminderWhen } from "./Reminders";
import { pimStoreFor } from "./store";
import { CalendarDate, localZone, makePimTime, todayIn } from "./time";
import { dateFormatter } from "../../i18n/format";
import { chooseFromMenu } from "./lookTesting";

inTimeZone(BERLIN);

const START = "Start";

/** Opens the editor on a new event tomorrow 15:00–16:00 (Berlin). */
function openNew(env: Env, close = vi.fn()) {
  const day = todayIn(localZone()).add({ days: 1 });
  pimStoreFor(env.store)
    .getState()
    .editEvent({
      mode: "new",
      start: makePimTime(day, { hour: 15, minute: 0 }, BERLIN),
      end: makePimTime(day, { hour: 16, minute: 0 }, BERLIN),
      calendarId: "cal-personal",
      fields: null,
    });
  const view = renderWithProviders(
    <EventEditorDialog name="pim-event-editor" close={close} />,
    env,
  );
  return { day, close, ...view };
}

describe("the compact event editor", () => {
  it("reads the time as one line and saves the chips' choices", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const { day, close, container } = openNew(env);
    const title = await screen.findByLabelText("Title");
    expect(title).toHaveAttribute("data-part", "pim.compact.title.input");
    expect(title).toHaveAttribute("placeholder", "Add a title");
    await user.type(title, "Dentist");

    const when = screen.getByRole("group", { name: "When" });
    // The day as text on one button, the end on the same day shows no day of its own.
    const startDay = within(when).getByRole("button", { name: /^Start, date: / });
    expect(startDay.textContent).toMatch(/^[A-Z][a-z]{2}, [A-Z][a-z]{2} \d{1,2}, \d{4}$/);
    expect(within(when).queryByRole("button", { name: /^End, date: / })).toBeNull();
    expect(within(when).getByText("Length: 1 hour")).toBeInTheDocument();

    const options = screen.getByRole("group", { name: "Options" });
    await user.selectOptions(within(options).getByLabelText("Reminder"), "30");
    await user.selectOptions(within(options).getByLabelText("Show me as"), "free");
    await user.selectOptions(within(options).getByLabelText("Repeats"), "weekly");
    // The repetition's details show only while it repeats.
    expect(screen.getByText(/^Every week on/)).toBeInTheDocument();
    await user.click(within(options).getByRole("button", { name: "Call link" }));
    await user.type(screen.getByLabelText("Link"), "https://meet.example.com/dentist");
    await user.type(screen.getByLabelText("Location"), "Main street 1");
    // Status and visibility wait behind "More options".
    const more = must(container.querySelector('[data-part="pim.compact.more"]'));
    expect(more).not.toHaveAttribute("open");
    expect(await violations(container)).toEqual([]);
    await clickAndClose(user, "Save", close);

    const created = must(
      env.transport.data.features.pim.events.find((e) => e.fields.title === "Dentist"),
    );
    expect(created.fields).toMatchObject({
      transparency: "free",
      location: "Main street 1",
      conferences: [{ uri: "https://meet.example.com/dentist" }],
      alarms: [{ action: "display", trigger: { type: "relative", offset_secs: -1800 } }],
      start: { type: "zoned", local: `${day.toString()}T15:00:00` },
    });
    expect(created.recurrence?.rules[0]?.freq).toBe("weekly");
  });

  it("shows the end's day once the event ends on another day, and keeps the length when the start moves", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    openNew(env);
    await screen.findByLabelText("Title");
    await user.click(screen.getByText("More options"));
    await user.click(screen.getByRole("button", { name: /^End date, date: / }));
    const grid = screen.getByRole("grid");
    await waitFor(() => expect(document.activeElement).toHaveAttribute("data-day"));
    await user.keyboard("{ArrowRight}{Enter}");
    expect(grid).not.toBeInTheDocument();
    const when = screen.getByRole("group", { name: "When" });
    expect(within(when).getByRole("button", { name: /^End, date: / })).toBeInTheDocument();
    expect(within(when).getByText("Length: 1 day 1 hour")).toBeInTheDocument();
  });
});

describe("the readable date field", () => {
  it("shows the day as text in the UI language and picks it from the month grid", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    const onChange = vi.fn();
    renderWithProviders(
      <DateTimeInput
        label={START}
        field="start"
        display="text"
        day={new CalendarDate(2026, 11, 27)}
        time={{ hour: 15, minute: 0 }}
        tz={BERLIN}
        onChange={onChange}
      />,
      env,
    );
    const day = screen.getByRole("button", { name: "Start, date: Fri, Nov 27, 2026" });
    expect(day).toHaveTextContent("Fri, Nov 27, 2026");
    expect(screen.queryByRole("button", { name: /Choose the date/ })).toBeNull();
    await user.click(day);
    await waitFor(() => expect(document.activeElement).toHaveAttribute("data-day", "2026-11-27"));
    await user.keyboard("{ArrowRight}{Enter}");
    expect(onChange).toHaveBeenCalledWith(new CalendarDate(2026, 11, 28), { hour: 15, minute: 0 });
  });
});

describe("the reminder toast", () => {
  it("says how soon, in which calendar, and snoozes through one split button", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    renderWithProviders(<PimToasts />, env);
    const reminder = must(fireReminder(env.transport, "rem-review"));
    act(() => {
      pimStoreFor(env.store).getState().pushToast({ kind: "reminder", id: "r1", reminder });
    });
    await settleQueries(env);
    const toast = screen.getByRole("group", { name: "Reminder: Design review" });
    expect(toast.querySelector('[data-part="pim.reminder.when"]')?.textContent).toMatch(
      /^(in |tomorrow)/,
    );
    expect(within(toast).getByText(/ · Work$/)).toBeInTheDocument();
    expect(toast.querySelector('[data-part="pim.reminder.edge"]')).toHaveAttribute("data-geom");
    // Dismiss first and primary; then the split snooze; then Open.
    const buttons = within(toast)
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label") ?? b.textContent);
    expect(buttons).toEqual(["Dismiss", "Snooze 10 minutes", "More snooze times", "Open"]);
    expect(within(toast).getByRole("button", { name: "Dismiss" })).toHaveAttribute(
      "data-variant",
      "primary",
    );
    await user.click(within(toast).getByRole("button", { name: "More snooze times" }));
    expect(screen.getAllByRole("menuitem").map((m) => m.textContent)).toEqual([
      "5 minutes",
      "10 minutes",
      "15 minutes",
      "30 minutes",
      "1 hour",
      "Tomorrow morning",
    ]);
    await user.keyboard("{Escape}");
    await chooseFromMenu(user, toast, "More snooze times", "1 hour");
    await waitFor(() => {
      expect(env.transport.data.features.pim.reminders[0]?.state).toBe("snoozed");
    });
  });

  it("reads the start relative to now", () => {
    const format = dateFormatter("en");
    const now = Date.parse("2026-11-27T14:30:00Z");
    const reminder = {
      starts_at: "2026-11-27T15:00:00Z",
    } as Parameters<typeof reminderWhen>[1];
    expect(reminderWhen(format, reminder, now)).toBe("in 30 minutes");
    expect(reminderWhen(format, { ...reminder, starts_at: null }, now)).toBeNull();
  });
});
