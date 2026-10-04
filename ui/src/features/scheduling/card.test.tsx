/**
 * The invitation card against the mock core: states, conflicts, spoofing
 * warnings, answering (comment, without sending, confirmation, one
 * occurrence), proposing a new time, forwarding, applying cancellations,
 * counter-proposals, opening the event, and the `invitation-card` widget.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Request } from "../../api/generated";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { pimStoreFor } from "../pim/store";
import { InvitationCard } from ".";
import { DEMO_INVITATIONS, seedSchedulingDemo } from "./mockSeed";

function setup(): Env {
  const env = createEnv();
  seedSchedulingDemo(env.transport.data);
  return env;
}

function last<T extends Request["type"]>(env: Env, type: T): Extract<Request, { type: T }> {
  const found = [...env.transport.requests].reverse().find((r) => r.type === type);
  if (!found) throw new Error(`no ${type} request`);
  return found as Extract<Request, { type: T }>;
}

async function renderCard(env: Env, id: string) {
  renderWithProviders(<InvitationCard messageId={id} />, env);
  return screen.findByRole("region", { name: /^Invitation:/ });
}

describe("invitation card", () => {
  it("shows a new invitation with its details, conflicts and answers", async () => {
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    expect(within(card).getByRole("heading", { name: "Architecture sync" })).toBeInTheDocument();
    expect(within(card).getByText("Not answered yet")).toBeInTheDocument();
    expect(within(card).getByText("Room 4", { selector: "dd" })).toBeInTheDocument();
    expect(
      within(card).getByText(/Grace Hopper <grace@navy.example>/, { selector: "dd" }),
    ).toBeInTheDocument();
    expect(within(card).getByText(/overlaps one event/)).toBeInTheDocument();
    expect(within(card).getByText("Design review")).toBeInTheDocument();
    expect(within(card).getByText(/4 people: 2 accepted, 1 maybe/)).toBeInTheDocument();
    for (const name of ["Accept", "Maybe", "Decline", "Propose a new time", "Forward invitation"]) {
      expect(within(card).getByRole("button", { name })).toBeInTheDocument();
    }
  });

  it("warns first about a forged sender", async () => {
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.spoofed);
    expect(within(card).getByRole("alert")).toHaveTextContent(
      "This invitation was not sent by the organizer",
    );
  });

  it("accepts with a note and reports that the answer was sent", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    await user.type(within(card).getByLabelText("Note for the organizer (optional)"), "On my way");
    await user.click(within(card).getByRole("button", { name: "Accept" }));
    expect(
      await within(card).findByText("Accepted. Your answer is on its way."),
    ).toBeInTheDocument();
    expect(last(env, "RespondToInvitation")).toMatchObject({
      target: { type: "message", message_id: DEMO_INVITATIONS.newRequest },
      response: "accept",
      comment: "On my way",
      send: true,
    });
    await waitFor(() =>
      expect(within(card).getByRole("button", { name: "Accept" })).toHaveAttribute(
        "aria-pressed",
        "true",
      ),
    );
    expect(within(card).getByRole("button", { name: "Open in calendar" })).toBeInTheDocument();
  });

  it("declines without sending a response", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    await user.click(
      within(card).getByRole("checkbox", { name: "Send my answer to the organizer" }),
    );
    expect(within(card).getByLabelText("Note for the organizer (optional)")).toBeDisabled();
    await user.click(within(card).getByRole("button", { name: "Decline" }));
    expect(await within(card).findByText("Declined. No answer was sent.")).toBeInTheDocument();
    expect(last(env, "RespondToInvitation")).toMatchObject({ response: "decline", send: false });
  });

  it("asks before sending when the settings say so", async () => {
    const user = userEvent.setup();
    const env = setup();
    env.transport.data.features.scheduling.settings.confirm_replies = true;
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    await waitFor(() => {
      expect(env.transport.requests.some((r) => r.type === "GetSchedulingSettings")).toBe(true);
    });
    await user.click(within(card).getByRole("button", { name: "Maybe" }));
    expect(await within(card).findByText("Send “Maybe” to Grace Hopper?")).toBeInTheDocument();
    expect(env.transport.requests.some((r) => r.type === "RespondToInvitation")).toBe(false);
    await user.click(within(card).getByRole("button", { name: "Only update my calendar" }));
    await within(card).findByText("Marked as maybe. No answer was sent.");
    expect(last(env, "RespondToInvitation")).toMatchObject({ response: "tentative", send: false });
  });

  it("answers one occurrence of a series", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.series);
    expect(within(card).getByText(/Every week/)).toBeInTheDocument();
    await user.click(within(card).getByRole("radio", { name: "One occurrence" }));
    expect(within(card).getByRole("group", { name: "Day of the occurrence" })).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Accept" }));
    await within(card).findByText("Accepted. Your answer is on its way.");
    const request = last(env, "RespondToInvitation");
    expect(request.target.type).toBe("event");
    expect(request.target.type === "event" && request.target.recurrence_id).toBeTruthy();
  });

  it("proposes a new time with the PIM pickers", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    await user.click(within(card).getByRole("button", { name: "Propose a new time" }));
    const form = within(card).getByRole("form", { name: "Propose a new time" });
    expect(within(form).getByRole("group", { name: "Proposed start" })).toBeInTheDocument();
    await user.type(within(form).getByLabelText("Note (optional)"), "Morning?");
    await user.click(within(form).getByRole("button", { name: "Send proposal" }));
    expect(await within(card).findByText("Your proposal is on its way.")).toBeInTheDocument();
    expect(last(env, "ProposeNewTime")).toMatchObject({
      target: { type: "message", message_id: DEMO_INVITATIONS.newRequest },
      comment: "Morning?",
    });
  });

  it("forwards to valid addresses only", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    await user.click(within(card).getByRole("button", { name: "Forward invitation" }));
    const form = within(card).getByRole("form", { name: "Forward invitation" });
    await user.type(
      within(form).getByLabelText("To (addresses, separated by commas)"),
      "not an address",
    );
    await user.click(within(form).getByRole("button", { name: "Forward" }));
    expect(within(form).getByRole("alert")).toHaveTextContent("Enter at least one valid address.");
    await user.clear(within(form).getByLabelText("To (addresses, separated by commas)"));
    await user.type(
      within(form).getByLabelText("To (addresses, separated by commas)"),
      "edsger@tue.example, liskov@mit.example",
    );
    await user.click(within(form).getByRole("button", { name: "Forward" }));
    await within(card).findByText("The invitation was forwarded.");
    expect(last(env, "ForwardInvitation").to.map((a) => a.email)).toEqual([
      "edsger@tue.example",
      "liskov@mit.example",
    ]);
  });

  it("shows a cancellation and removes the event from the calendar", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.cancel);
    expect(within(card).getByText("Cancelled", { selector: "p" })).toBeInTheDocument();
    expect(within(card).getByText("The organizer cancelled this event")).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Accept" })).toBeNull();
    await user.click(within(card).getByRole("button", { name: "Remove from calendar" }));
    await within(card).findByText("Removed from your calendar.");
    expect(last(env, "ApplyInvitation").message_id).toBe(DEMO_INVITATIONS.cancel);
  });

  it("shows an update and applies it", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.update);
    expect(within(card).getByText("Updated invitation")).toBeInTheDocument();
    expect(within(card).getByText("Changes your calendar")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "Update my calendar" }));
    await within(card).findByText("Your calendar is updated.");
  });

  it("organizer: records a reply and handles a counter-proposal", async () => {
    const user = userEvent.setup();
    const env = setup();
    renderWithProviders(
      <>
        <InvitationCard messageId={DEMO_INVITATIONS.reply} />
        <InvitationCard messageId={DEMO_INVITATIONS.counter} />
      </>,
      env,
    );
    const reply = await screen.findByText("Katherine Johnson accepted.");
    expect(reply).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Record the answer" }));
    await screen.findByText("The answer is recorded.");

    expect(screen.getByText("Katherine Johnson proposes another time:")).toBeInTheDocument();
    expect(screen.getByText("I am at a conference that afternoon.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Accept the new time" }));
    await screen.findByText("The meeting moved; everyone gets the update.");
    expect(last(env, "RespondToCounter")).toMatchObject({ accept: true });
  });

  it("opens the event in the PIM editor", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.update);
    await user.click(within(card).getByRole("button", { name: "Open in calendar" }));
    expect(env.store.getState().windows).toContain("pim-event-editor");
    expect(pimStoreFor(env.store).getState().eventEditor).toMatchObject({
      mode: "edit",
      eventId: "ev-standup",
    });
  });

  it("shows the core's text when answering fails", async () => {
    const user = userEvent.setup();
    const env = setup();
    const card = await renderCard(env, DEMO_INVITATIONS.newRequest);
    env.transport.data.features.scheduling.unsupported = true;
    await user.click(within(card).getByRole("button", { name: "Accept" }));
    expect(await within(card).findByRole("alert")).toBeInTheDocument();
  });

  it("the invitation-card widget follows the selected message", async () => {
    const env = setup();
    env.store.getState().selectMessage(DEMO_INVITATIONS.cancel);
    const { container, unmount } = renderWithProviders(
      <NodeView
        node={widget("invitation-card", { options: { attendees: "expanded" } })}
        path="t/0"
      />,
      env,
    );
    expect(await screen.findByRole("heading", { name: "Book club" })).toBeInTheDocument();
    expect(container.querySelector('[data-part="invitation.attendees"]')).toHaveAttribute("open");
    unmount();

    const plain = env.transport.data.messages.find((m) => !m.id.includes("#invite"))?.id ?? "";
    env.store.getState().selectMessage(plain);
    const second = renderWithProviders(
      <NodeView node={widget("invitation-card")} path="t/0" />,
      env,
    );
    await waitFor(() => {
      expect(env.transport.requests.filter((r) => r.type === "GetInvitation").length).toBe(2);
    });
    expect(second.container.querySelector('[data-part="invitation"]')).toBeNull();
  });
});
