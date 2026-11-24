/**
 * "New event from text" (R8, ADR 0251): the command palette lists the PIM
 * feature's `calendar.new-event-from-text`, which opens the quick-add field
 * in a window of its own; Enter there creates the event through the same
 * reading (`ParseEventText`) and chips as the calendar's quick add, the
 * window stays for the next one, and Tab hands the text to the editor and
 * closes it.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useActions } from "../../actions/ActionsProvider";
import { WindowLayer } from "../../layout/WindowLayer";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { PALETTE_OPEN_ACTION, loadPalette } from "../palette";
import { appRegistry } from "../registry";
import { EVENT_DIALOG, EVENT_FROM_TEXT_DIALOG, NEW_EVENT_FROM_TEXT_ACTION } from "./names";
import { eventTitled } from "./testing";
import { dateString, localZone, todayIn } from "./time";

beforeAll(() => loadPalette());

function OpenPalette() {
  const actions = useActions();
  return (
    <button type="button" onClick={() => void actions.dispatch(PALETTE_OPEN_ACTION)}>
      open palette
    </button>
  );
}

function render(env: Env) {
  const layout = env.transport.data.layout;
  return renderWithProviders(
    <>
      <OpenPalette />
      <WindowLayer layout={layout} />
    </>,
    env,
    { layout },
  );
}

/** Opens "New event from text" from the palette; answers its field. */
async function openFromPalette(user: ReturnType<typeof userEvent.setup>) {
  await user.click(screen.getByRole("button", { name: "open palette" }));
  const search = await screen.findByRole("combobox", { name: "Search actions" });
  await user.type(search, "event from text");
  const list = await screen.findByRole("listbox", { name: "Actions" });
  const first = must(within(list).getAllByRole("option")[0]);
  expect(first).toHaveAttribute("data-action", NEW_EVENT_FROM_TEXT_ACTION);
  expect(first).toHaveTextContent("New event from text");
  await user.keyboard("{Enter}");
  const window = await screen.findByRole("dialog", { name: "New event from text" });
  const field = within(window).getByRole("textbox", { name: "Quick add an event" });
  await waitFor(() => {
    expect(field).toHaveFocus();
  });
  return { window, field };
}

describe("New event from text (R8)", () => {
  it("is described for the palette by the PIM feature", () => {
    expect(appRegistry.actionInfo[NEW_EVENT_FROM_TEXT_ACTION]).toMatchObject({
      title: "pim-act-from-text-title",
      icon: "calendar-plus",
    });
  });

  it("creates the event from a sentence with the quick-add chips and stays for the next one", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    render(env);
    const { window, field } = await openFromPalette(user);
    await user.type(field, "Lunch with Bob tomorrow 12:30");
    const preview = await within(window).findByRole("list", { name: "What Nail understood" });
    await waitFor(() => {
      expect(
        [...preview.querySelectorAll('[data-part="calendar.quick-add.chip"]')].map((c) =>
          c.getAttribute("data-kind"),
        ),
      ).toEqual(["title", "date", "time", "calendar"]);
    });
    await user.keyboard("{Enter}");
    const created = await eventTitled(env, "Lunch with Bob");
    const tomorrow = dateString(todayIn(localZone()).add({ days: 1 }));
    expect(created.fields.start).toMatchObject({ local: `${tomorrow}T12:30:00` });
    expect(await within(window).findByText(/^Added “Lunch with Bob” on/)).toBeInTheDocument();
    expect(env.store.getState().windows).toContain(EVENT_FROM_TEXT_DIALOG);
    expect(field).toHaveValue("");
  });

  it("hands the text to the editor with Tab and closes", async () => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    render(env);
    const { field } = await openFromPalette(user);
    await user.type(field, "Dentist tomorrow 3pm");
    await user.keyboard("{Tab}");
    await waitFor(() => {
      expect(env.store.getState().windows).toContain(EVENT_DIALOG);
    });
    expect(env.store.getState().windows).not.toContain(EVENT_FROM_TEXT_DIALOG);
  });
});
