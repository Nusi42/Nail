/**
 * The attendee picker with the free/busy timeline, "find a time", the
 * event editor's meeting panel (organizer and attendee), the cancel and
 * settings dialogs, and the feature's actions — against the mock core.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Attendee, CalendarEvent, PimTime } from "../../api/generated";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { pimStoreFor } from "../pim/store";
import { localZone, makePimTime, todayIn } from "../pim/time";
import { actions, AttendeePicker, dialogs, MeetingPanel, schedulingKeys } from ".";
import { newAttendee } from "./AttendeePicker";
import { DEMO_INVITATIONS, seedSchedulingDemo } from "./mockSeed";
import { CANCEL_DIALOG, FIND_TIME_DIALOG, SETTINGS_DIALOG } from "./names";
import { schedulingStoreFor } from "./store";
import { actionContext, requestsOf } from "../../test/env";

function setup(): Env {
  const env = createEnv();
  seedSchedulingDemo(env.transport.data);
  return env;
}

function event(env: Env, id: string): CalendarEvent {
  const found = env.transport.data.features.pim.events.find((e) => e.id === id);
  if (!found) throw new Error(`no event ${id}`);
  return structuredClone(found);
}

const tz = localZone();
const tomorrow = todayIn(tz).add({ days: 1 });
const at = (hour: number, minute = 0): PimTime => makePimTime(tomorrow, { hour, minute }, tz);

function Picker({
  initial,
  onTime,
}: {
  initial: Attendee[];
  onTime: (start: PimTime, end: PimTime) => void;
}) {
  const [value, setValue] = useState(initial);
  const [time, setTime] = useState({ start: at(14, 30), end: at(15, 30) });
  return (
    <AttendeePicker
      value={value}
      onChange={setValue}
      start={time.start}
      end={time.end}
      organizer="ada@work.example"
      onTimeChange={(start, end) => {
        setTime({ start, end });
        onTime(start, end);
      }}
    />
  );
}

describe("attendee picker and free/busy timeline", () => {
  it("shows busy blocks per person, who is busy, and moves the meeting to a free slot", async () => {
    const user = userEvent.setup();
    const env = setup();
    const onTime = vi.fn();
    const { container } = renderWithProviders(
      <Picker
        initial={[
          newAttendee("grace@navy.example", "Grace Hopper"),
          newAttendee("katherine@nasa.example", "Katherine Johnson"),
          newAttendee("linus@kernel.example", "Linus"),
        ]}
        onTime={onTime}
      />,
      env,
    );
    const grace = await screen.findByRole("img", { name: /^Grace Hopper: .*busy/ });
    expect(grace).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /^ada@work.example: / })).toBeInTheDocument();
    expect(
      screen.getByRole("img", { name: "Linus: availability could not be checked" }),
    ).toBeInTheDocument();
    expect(container.querySelectorAll('[data-part="freebusy.busy"]').length).toBeGreaterThan(2);
    expect(container.querySelectorAll('[data-part="freebusy.meeting"]').length).toBe(4);
    // 14:30–15:30: Katherine (14–15) and Ada's design review (14–15:30) collide.
    expect(screen.getByText(/people are busy at the chosen time/)).toHaveTextContent(
      "Katherine Johnson",
    );
    const query = requestsOf(env, "QueryFreeBusy")[0];
    expect(query?.attendees).toEqual([
      "ada@work.example",
      "grace@navy.example",
      "katherine@nasa.example",
      "linus@kernel.example",
    ]);
    const pick = screen.getAllByRole("button", { name: /^Move the meeting to/ })[0];
    if (!pick) throw new Error("no suggestion");
    await user.click(pick);
    expect(onTime).toHaveBeenCalledTimes(1);
    const [start, end] = onTime.mock.calls[0] as [PimTime, PimTime];
    expect(start.type).toBe("zoned");
    expect(end.type).toBe("zoned");
    await waitFor(() =>
      expect(screen.getByText("Everyone is free at the chosen time.")).toBeInTheDocument(),
    );
  });

  it("steps through days and asks the core again", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(
      <Picker initial={[newAttendee("grace@navy.example", null)]} onTime={() => undefined} />,
      env,
    );
    await screen.findByRole("img", { name: /^grace@navy.example: / });
    await user.click(screen.getByRole("button", { name: "Next day" }));
    await waitFor(() => {
      expect(requestsOf(env, "QueryFreeBusy").length).toBe(2);
    });
    const [first, second] = requestsOf(env, "QueryFreeBusy");
    expect(Date.parse(second?.from ?? "") - Date.parse(first?.from ?? "")).toBeGreaterThan(
      20 * 3_600_000,
    );
    await user.click(screen.getByRole("button", { name: "Today" }));
    await waitFor(() => {
      expect(requestsOf(env, "QueryFreeBusy").length).toBe(3);
    });
  });

  it("adds people, refuses bad addresses and offers kinds", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(<Picker initial={[]} onTime={() => undefined} />, env);
    expect(screen.getByText("No one is invited yet.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Address"), "nonsense");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getByRole("alert")).toHaveTextContent("This address is not valid.");
    await user.clear(screen.getByLabelText("Address"));
    await user.type(screen.getByLabelText("Address"), "room-4@work.example");
    await user.selectOptions(screen.getByLabelText("Kind"), "room");
    await user.click(screen.getByRole("button", { name: "Add" }));
    expect(screen.getAllByText("room-4@work.example").length).toBeGreaterThan(0);
    expect(screen.getByLabelText("Role of room-4@work.example")).toHaveValue("non-participant");
    await user.click(screen.getByRole("button", { name: "Remove room-4@work.example" }));
    expect(screen.getByText("No one is invited yet.")).toBeInTheDocument();
  });

  it("find a time opens the event editor with the people and the slot", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(
      <NodeView node={widget("freebusy-timeline", { options: { duration: 60 } })} path="t/0" />,
      env,
    );
    expect(screen.getByRole("button", { name: "Create meeting" })).toBeDisabled();
    await user.type(screen.getByLabelText("Address"), "alan@bletchley.example{Enter}");
    // Tomorrow always has free working hours, whatever the time of day now.
    await user.click(screen.getByRole("button", { name: "Next day" }));
    const pick = (await screen.findAllByRole("button", { name: /^Move the meeting to/ }))[0];
    if (!pick) throw new Error("no suggestion");
    await user.click(pick);
    await user.click(screen.getByRole("button", { name: "Create meeting" }));
    expect(env.store.getState().windows).toContain("pim-event-editor");
    const target = pimStoreFor(env.store).getState().eventEditor;
    expect(target?.mode).toBe("new");
    if (target?.mode !== "new") throw new Error("not new");
    expect(target.fields?.attendees.map((a) => a.email)).toEqual(["alan@bletchley.example"]);
    expect(target.fields?.organizer?.email).toBeTruthy();
  });
});

describe("meeting panel", () => {
  it("organizer sends invitations to everyone or chosen people", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(<MeetingPanel event={event(env, "ev-review")} recurrenceId={null} />, env);
    const panel = await screen.findByRole("region", { name: /^Meeting: / });
    await user.type(within(panel).getByLabelText("Note in the mail (optional)"), "Agenda inside");
    await user.click(within(panel).getByRole("button", { name: "Send invitations" }));
    await within(panel).findByText("The invitations are on their way.");
    expect(requestsOf(env, "SendInvitations")[0]).toMatchObject({
      event_id: "ev-review",
      attendees: [],
      comment: "Agenda inside",
    });
    await user.click(within(panel).getByRole("radio", { name: "Only the people I choose" }));
    expect(within(panel).getByRole("button", { name: "Send invitations" })).toBeDisabled();
    await user.click(within(panel).getByRole("checkbox", { name: /Katherine Johnson/ }));
    await user.click(within(panel).getByRole("button", { name: "Send invitations" }));
    await waitFor(() => {
      expect(requestsOf(env, "SendInvitations")).toHaveLength(2);
    });
    expect(requestsOf(env, "SendInvitations")[1]?.attendees).toEqual(["katherine@nasa.example"]);
  });

  it("organizer cancels for some attendees through the dialog", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(<MeetingPanel event={event(env, "ev-review")} recurrenceId={null} />, env);
    await user.click(await screen.findByRole("button", { name: "Cancel meeting…" }));
    expect(env.store.getState().windows).toContain(CANCEL_DIALOG);
    expect(schedulingStoreFor(env.store).getState().cancelTarget?.eventId).toBe("ev-review");
    const Dialog = dialogs[CANCEL_DIALOG];
    if (!Dialog) throw new Error("no dialog");
    const close = vi.fn();
    renderWithProviders(<Dialog name={CANCEL_DIALOG} close={close} />, env);
    expect(screen.queryByRole("radio", { name: /Only the occurrence/ })).toBeNull();
    await user.click(screen.getByRole("radio", { name: "Remove some attendees" }));
    expect(screen.getByRole("button", { name: "Cancel and notify" })).toBeDisabled();
    await user.click(screen.getByRole("checkbox", { name: /Katherine Johnson/ }));
    await user.click(screen.getByRole("button", { name: "Cancel and notify" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requestsOf(env, "CancelMeeting")[0]).toMatchObject({
      scope: "attendees",
      attendees: ["katherine@nasa.example"],
    });
    expect(schedulingStoreFor(env.store).getState().cancelTarget).toBeNull();
  });

  it("the cancel dialog offers one occurrence of a series and the whole meeting", async () => {
    const user = userEvent.setup();
    const env = setup();
    const recurrenceId = at(11);
    act(() => {
      schedulingStoreFor(env.store).getState().setCancelTarget({
        eventId: "ev-review",
        title: "Design review",
        recurrenceId,
        recurring: true,
        attendees: [],
      });
    });
    const Dialog = dialogs[CANCEL_DIALOG];
    if (!Dialog) throw new Error("no dialog");
    renderWithProviders(<Dialog name={CANCEL_DIALOG} close={() => undefined} />, env);
    expect(screen.getByRole("radio", { name: /Only the occurrence on/ })).toBeChecked();
    await user.click(screen.getByRole("radio", { name: "The whole meeting" }));
    await user.click(screen.getByRole("button", { name: "Cancel and notify" }));
    await waitFor(() => {
      expect(requestsOf(env, "CancelMeeting")).toHaveLength(1);
    });
    expect(requestsOf(env, "CancelMeeting")[0]).toMatchObject({
      scope: "meeting",
      recurrence_id: null,
    });
  });

  it("attendee answers one occurrence or the series and proposes a new time", async () => {
    const user = userEvent.setup();
    const env = setup();
    const standup = event(env, "ev-standup");
    const occurrence = standup.fields.start;
    renderWithProviders(<MeetingPanel event={standup} recurrenceId={occurrence} />, env);
    const panel = await screen.findByRole("region", { name: /^Meeting: / });
    expect(within(panel).getByText("Organized by Grace Hopper")).toBeInTheDocument();
    expect(
      within(panel).getByRole("checkbox", { name: "Answer only for this occurrence" }),
    ).toBeChecked();
    await user.click(within(panel).getByRole("button", { name: "Decline" }));
    await within(panel).findByText("Declined. Your answer is on its way.");
    expect(requestsOf(env, "RespondToInvitation")[0]?.target).toEqual({
      type: "event",
      event_id: "ev-standup",
      recurrence_id: occurrence,
    });
    await user.click(
      within(panel).getByRole("checkbox", { name: "Answer only for this occurrence" }),
    );
    await user.click(within(panel).getByRole("button", { name: "Propose a new time" }));
    await user.click(within(panel).getByRole("button", { name: "Send proposal" }));
    await within(panel).findByText("Your proposal is on its way.");
    expect(requestsOf(env, "ProposeNewTime")[0]?.target).toEqual({
      type: "event",
      event_id: "ev-standup",
      recurrence_id: null,
    });
  });

  it("shows nothing for events without attendees or without the user", async () => {
    const env = setup();
    const lunch = event(env, "ev-lunch");
    const { container } = renderWithProviders(
      <MeetingPanel event={lunch} recurrenceId={null} />,
      env,
    );
    await waitFor(() => {
      expect(requestsOf(env, "ListIdentities").length).toBeGreaterThan(0);
    });
    expect(container.querySelector('[data-part="scheduling.meeting"]')).toBeNull();
  });
});

describe("scheduling settings and actions", () => {
  it("edits and saves the settings", async () => {
    const user = userEvent.setup();
    const env = setup();
    const Dialog = dialogs[SETTINGS_DIALOG];
    if (!Dialog) throw new Error("no dialog");
    const close = vi.fn();
    renderWithProviders(<Dialog name={SETTINGS_DIALOG} close={close} />, env);
    await user.click(await screen.findByRole("checkbox", { name: /Ask before sending an answer/ }));
    await user.click(screen.getByRole("checkbox", { name: /Remove cancelled events/ }));
    await waitFor(() =>
      expect(screen.getByLabelText("Calendar for new invitations")).toContainHTML("Personal"),
    );
    await user.selectOptions(screen.getByLabelText("Calendar for new invitations"), "cal-personal");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(env.transport.data.features.scheduling.settings).toMatchObject({
      confirm_replies: true,
      apply_cancellations: true,
      calendar_id: "cal-personal",
    });
  });

  it("answers the selected message's invitation from a key binding", async () => {
    const env = setup();
    const c = actionContext(env);
    expect(actions["scheduling.accept"]?.enabled?.(c, [])).toBe(false);
    env.store.getState().selectMessage(DEMO_INVITATIONS.newRequest);
    const card = await env.client.request(
      { type: "GetInvitation", message_id: DEMO_INVITATIONS.newRequest },
      "Invitation",
    );
    env.queryClient.setQueryData(
      schedulingKeys.invitation(DEMO_INVITATIONS.newRequest),
      card.invitation,
    );
    expect(actions["scheduling.accept"]?.enabled?.(c, [])).toBe(true);
    await actions["scheduling.decline"]?.run(c, []);
    expect(requestsOf(env, "RespondToInvitation")[0]).toMatchObject({
      response: "decline",
      send: true,
    });
  });

  it("opens its dialogs", async () => {
    const env = setup();
    await actions["scheduling.settings"]?.run(actionContext(env), []);
    await actions["scheduling.find-time"]?.run(actionContext(env), []);
    expect(env.store.getState().windows).toEqual([SETTINGS_DIALOG, FIND_TIME_DIALOG]);
  });
});
