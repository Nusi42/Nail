/**
 * Exchange PIM UI against the mock core: the room finder of the event
 * editor (room lists, capacity, availability, booking a room as an
 * attendee, missing permission) and To Do lists in the task widget.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useEffect, useState } from "react";
import type { Attendee, Calendar, PimTime } from "../../api/generated";
import { renderWithProviders } from "../../test/harness";
import { RoomFinder, TaskListBadge, TaskListChooser } from ".";
import { EXCHANGE_ACCOUNT, EXCHANGE_CALENDARS } from "./mockSeed";
import { eventRange } from "./RoomFinder";
import { requests, setup } from "./testing";

const at = (local: string): PimTime => ({ type: "zoned", local, tz: "UTC" });

interface Spy {
  attendees: Attendee[];
  location: string;
}

function Editor({
  calendarId,
  spy,
  start = at("2026-10-01T09:00:00"),
  end = at("2026-10-01T09:30:00"),
}: {
  calendarId: string;
  spy: Spy;
  start?: PimTime;
  end?: PimTime;
}) {
  const [attendees, setAttendees] = useState<Attendee[]>([]);
  const [location, setLocation] = useState("");
  useEffect(() => {
    Object.assign(spy, { attendees, location });
  }, [spy, attendees, location]);
  return (
    <RoomFinder
      calendarId={calendarId}
      start={start}
      end={end}
      attendees={attendees}
      onAttendeesChange={setAttendees}
      location={location}
      onLocationChange={setLocation}
      tz="UTC"
    />
  );
}

describe("room finder", () => {
  it("is absent for calendars of other accounts", async () => {
    const env = setup();
    const spy: Spy = { attendees: [], location: "" };
    const { container } = renderWithProviders(<Editor calendarId="cal-personal" spy={spy} />, env);
    await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
    expect(container.querySelector('[data-part="exchange.rooms"]')).toBeNull();
  });

  it("finds rooms with their availability and books one", async () => {
    const env = setup();
    const user = userEvent.setup();
    const spy: Spy = { attendees: [], location: "" };
    renderWithProviders(<Editor calendarId={EXCHANGE_CALENDARS.calendar} spy={spy} />, env);
    await user.click(await screen.findByRole("button", { name: "Find a room…" }));
    const results = await screen.findByRole("list", { name: "Rooms for this time" });
    const rows = within(results)
      .getAllByRole("listitem")
      .filter((li) => li.matches('[data-part="exchange.room"]'));
    expect(
      rows.map((r) => [
        r.querySelector('[data-part="exchange.room.name"]')?.textContent,
        r.getAttribute("data-availability"),
      ]),
    ).toEqual([
      ["Turing room", "free"],
      ["Ada room", "busy"],
      ["Babbage hall", "busy"],
      ["Hopper lab", "unknown"],
    ]);
    const ada = rows[1] as HTMLElement;
    expect(ada).toHaveTextContent("8 seats");
    expect(ada).toHaveTextContent("Building 1, floor 2");
    expect(ada).toHaveTextContent("Wheelchair accessible");
    expect(within(ada).getByRole("list", { name: "Busy times" })).toHaveTextContent(
      "9:00 AM–9:30 AM",
    );
    expect(requests(env, "FindRooms")[0]).toMatchObject({
      account_id: EXCHANGE_ACCOUNT,
      from: "2026-10-01T09:00:00Z",
      to: "2026-10-01T09:30:00Z",
      room_list: null,
      min_capacity: null,
    });

    await user.click(screen.getByRole("button", { name: "Add room Turing room" }));
    expect(spy.attendees).toEqual([
      expect.objectContaining({ email: "room-turing@contoso.example", kind: "room" }),
    ]);
    expect(spy.location).toBe("Turing room");
    const booked = screen.getByRole("list", { name: "Booked rooms" });
    expect(booked).toHaveTextContent("Turing room");
    expect(booked).toHaveTextContent("Not answered yet");
    await user.click(within(booked).getByRole("button", { name: "Remove room Turing room" }));
    expect(spy.attendees).toEqual([]);
    expect(spy.location).toBe("");
  });

  it("filters by room list, seats and free rooms", async () => {
    const env = setup();
    const user = userEvent.setup();
    const spy: Spy = { attendees: [], location: "" };
    renderWithProviders(<Editor calendarId={EXCHANGE_CALENDARS.calendar} spy={spy} />, env);
    await user.click(await screen.findByRole("button", { name: "Find a room…" }));
    await screen.findByRole("list", { name: "Rooms for this time" });
    await user.selectOptions(screen.getByLabelText("Room list"), "Building 2");
    await user.type(screen.getByLabelText("At least this many seats"), "5");
    await waitFor(() =>
      expect(requests(env, "FindRooms").at(-1)).toMatchObject({
        room_list: "building2@contoso.example",
        min_capacity: 5,
      }),
    );
    await waitFor(() =>
      expect(screen.getByRole("list", { name: "Rooms for this time" })).toHaveTextContent(
        "Hopper lab",
      ),
    );
    await user.click(screen.getByRole("checkbox", { name: "Only rooms that are free" }));
    expect(await screen.findByText("No room matches.")).toBeInTheDocument();
  });

  it("shows the permission warning and refuses a time that ends before it starts", async () => {
    const env = setup();
    const user = userEvent.setup();
    env.transport.data.features.exchange.roomsForbidden.push(EXCHANGE_ACCOUNT);
    const spy: Spy = { attendees: [], location: "" };
    renderWithProviders(
      <Editor
        calendarId={EXCHANGE_CALENDARS.calendar}
        spy={spy}
        start={at("2026-10-01T10:00:00")}
        end={at("2026-10-01T09:00:00")}
      />,
      env,
    );
    await user.click(await screen.findByRole("button", { name: "Find a room…" }));
    expect(
      await screen.findByText(/Rooms need the "Place.Read.All" permission/),
    ).toBeInTheDocument();
    expect(
      screen.getByText("The event must end after it starts to find a room."),
    ).toBeInTheDocument();
    expect(requests(env, "FindRooms")).toHaveLength(0);
  });

  it("asks for whole days of all-day events", () => {
    expect(
      eventRange({ type: "date", date: "2026-10-01" }, { type: "date", date: "2026-10-01" }, "UTC"),
    ).toEqual({ from: "2026-10-01T00:00:00Z", to: "2026-10-02T00:00:00Z" });
    expect(
      eventRange(at("2026-10-01T09:00:00"), at("2026-10-01T10:00:00"), "Europe/Berlin"),
    ).toEqual({ from: "2026-10-01T09:00:00Z", to: "2026-10-01T10:00:00Z" });
  });
});

describe("To Do lists in the task widget", () => {
  function lists(env: ReturnType<typeof setup>): Calendar[] {
    return env.transport.data.features.pim.calendars.filter((c) => c.components.includes("task"));
  }

  it("groups the lists: To Do per account, then the rest", async () => {
    const env = setup();
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithProviders(<TaskListChooser lists={lists(env)} value="" onChange={onChange} />, env);
    const select = screen.getByLabelText("Task list");
    await waitFor(() =>
      expect(
        within(select).getByRole("group", { name: "Microsoft To Do — Contoso" }),
      ).toBeInTheDocument(),
    );
    const todo = within(select).getByRole("group", { name: "Microsoft To Do — Contoso" });
    expect(
      within(todo)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Tasks", "Groceries", "Flagged email"]);
    expect(within(select).getByRole("group", { name: "Other lists" })).toBeInTheDocument();
    expect(within(select).getAllByRole("option")[0]).toHaveTextContent("All lists");
    await user.selectOptions(select, "Groceries");
    expect(onChange).toHaveBeenCalledWith(EXCHANGE_CALENDARS.groceries);
  });

  it("does not group when there are no Exchange lists", () => {
    const env = setup();
    const local = lists(env).filter((c) => !c.exchange);
    renderWithProviders(
      <TaskListChooser lists={local} value="" onChange={vi.fn()} allowAll={false} />,
      env,
    );
    const select = screen.getByLabelText("Task list");
    expect(within(select).queryByRole("group")).toBeNull();
    expect(within(select).queryByText("All lists")).toBeNull();
  });

  it("badges To Do, flagged email and other mailboxes' lists", () => {
    const env = setup();
    const byId = (id: string) => {
      const found = env.transport.data.features.pim.calendars.find((c) => c.id === id);
      if (!found) throw new Error(id);
      return found;
    };
    const boss: Calendar = {
      ...byId(EXCHANGE_CALENDARS.tasks),
      exchange: { mailbox: "charles@contoso.example", role: null, todo: false },
    };
    const { container } = renderWithProviders(
      <>
        <TaskListBadge list={byId(EXCHANGE_CALENDARS.tasks)} />
        <TaskListBadge list={byId(EXCHANGE_CALENDARS.flagged)} />
        <TaskListBadge list={boss} />
        <TaskListBadge list={byId(EXCHANGE_CALENDARS.calendar)} />
        <TaskListBadge list={byId("list-tasks")} />
      </>,
      env,
    );
    expect(
      [...container.querySelectorAll('[data-part="exchange.task-list-badge"]')].map(
        (b) => b.textContent,
      ),
    ).toEqual(["To Do", "Flagged email", "Shared from charles@contoso.example"]);
  });
});
