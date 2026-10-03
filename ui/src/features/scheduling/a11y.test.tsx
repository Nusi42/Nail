/**
 * Accessibility of the scheduling UI (axe-core in jsdom, like
 * `src/a11y.test.tsx`): every demo invitation card (new, series, update,
 * cancellation, reply, counter-proposal, spoofed), the open propose /
 * forward / confirm forms, the attendee picker with the free/busy
 * timeline, the meeting panels, and every dialog. Contrast needs a real
 * layout engine (Playwright).
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import type { ComponentType } from "react";
import type { DialogProps } from "../../layout/dialogRegistry";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { dialogs, InvitationCard, MeetingPanel } from ".";
import { firstStart, newAttendee, AttendeePicker } from "./AttendeePicker";
import { DEMO_INVITATIONS, seedSchedulingDemo } from "./mockSeed";
import { CANCEL_DIALOG, DIALOGS } from "./names";
import { schedulingStoreFor } from "./store";
import { localZone, makePimTime, todayIn } from "../pim/time";

async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

function setup(): Env {
  const env = createEnv();
  seedSchedulingDemo(env.transport.data);
  return env;
}

describe("axe: invitation cards", () => {
  it.each(Object.entries(DEMO_INVITATIONS))(
    "card %s has no violations",
    async (_, id) => {
      const env = setup();
      const { container } = renderWithProviders(
        <InvitationCard messageId={id} attendeesOpen />,
        env,
      );
      await settle(env);
      expect(container.querySelector('[data-part="invitation"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    30_000,
  );

  it("the propose and forward forms and the confirmation have no violations", async () => {
    const user = userEvent.setup();
    const env = setup();
    env.transport.data.features.scheduling.settings.confirm_replies = true;
    const { container } = renderWithProviders(
      <>
        <InvitationCard messageId={DEMO_INVITATIONS.newRequest} />
        <InvitationCard messageId={DEMO_INVITATIONS.series} />
        <InvitationCard messageId={DEMO_INVITATIONS.update} />
      </>,
      env,
    );
    await settle(env);
    const [first, second, third] = screen.getAllByRole("region", { name: /^Invitation:/ });
    if (!first || !second || !third) throw new Error("cards missing");
    await user.click(within(first).getByRole("button", { name: "Propose a new time" }));
    await user.click(
      within(first).getByRole("button", { name: /Choose the date for Proposed start/ }),
    );
    await user.click(within(second).getByRole("radio", { name: "One occurrence" }));
    await user.click(within(second).getByRole("button", { name: "Forward invitation" }));
    await user.click(within(second).getByRole("button", { name: "Forward" }));
    await user.click(within(third).getByRole("button", { name: "Accept" }));
    expect(within(third).getByText(/Send “Accept”/)).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  }, 60_000);

  it("the invitation-card widget has no violations", async () => {
    const env = setup();
    env.store.getState().selectMessage(DEMO_INVITATIONS.counter);
    const { container } = renderWithProviders(
      <NodeView node={widget("invitation-card")} path="t/0" />,
      env,
    );
    await settle(env);
    expect(screen.getByRole("heading", { name: "Design review" })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("axe: attendee picker, meeting panel, dialogs", () => {
  it("the attendee picker with the timeline has no violations", async () => {
    const env = setup();
    const tz = localZone();
    const day = todayIn(tz).add({ days: 1 });
    const { container } = renderWithProviders(
      <AttendeePicker
        value={[
          newAttendee("grace@navy.example", "Grace Hopper"),
          newAttendee("room-4@work.example", "Room 4", "room"),
          newAttendee("linus@kernel.example", null),
        ]}
        onChange={() => undefined}
        start={makePimTime(day, { hour: 14, minute: 30 }, tz)}
        end={makePimTime(day, { hour: 15, minute: 30 }, tz)}
        onTimeChange={() => undefined}
        organizer="ada@work.example"
      />,
      env,
    );
    await settle(env);
    expect(screen.getByRole("img", { name: /^Grace Hopper:/ })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  }, 30_000);

  it("the freebusy-timeline widget has no violations", async () => {
    const user = userEvent.setup();
    const env = setup();
    const { container } = renderWithProviders(
      <NodeView node={widget("freebusy-timeline")} path="t/0" />,
      env,
    );
    await user.type(screen.getByLabelText("Address"), "alan@bletchley.example{Enter}");
    await settle(env);
    expect(await violations(container)).toEqual([]);
  }, 30_000);

  it("organizer and attendee panels have no violations", async () => {
    const env = setup();
    const events = env.transport.data.features.pim.events;
    const review = structuredClone(events.find((e) => e.id === "ev-review"));
    const standup = structuredClone(events.find((e) => e.id === "ev-standup"));
    if (!review || !standup) throw new Error("demo events missing");
    const { container } = renderWithProviders(
      <>
        <MeetingPanel event={review} recurrenceId={null} />
        <MeetingPanel event={standup} recurrenceId={standup.fields.start} />
      </>,
      env,
    );
    await settle(env);
    expect(screen.getAllByRole("region", { name: /^Meeting: / })).toHaveLength(2);
    expect(await violations(container)).toEqual([]);
  });

  it.each([...DIALOGS])(
    "dialog %s has no violations",
    async (name) => {
      const env = setup();
      if (name === CANCEL_DIALOG) {
        schedulingStoreFor(env.store)
          .getState()
          .setCancelTarget({
            eventId: "ev-review",
            title: "Design review",
            recurrenceId: firstStart(localZone(), 8, 18, 30),
            recurring: true,
            attendees: [newAttendee("katherine@nasa.example", "Katherine Johnson")],
          });
      }
      const Dialog = dialogs[name] as ComponentType<DialogProps>;
      const { container } = renderWithProviders(
        <div role="dialog" aria-label={name}>
          <Dialog name={name} close={() => undefined} />
        </div>,
        env,
      );
      await settle(env);
      expect(container.querySelector("[data-part]")).not.toBeNull();
      expect(await violations(container)).toEqual([]);
    },
    30_000,
  );
});
