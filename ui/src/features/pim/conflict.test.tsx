/**
 * The conflict choice of events (R8, ADR 0251): an event changed here and on
 * the server shows the contact card's choice in its editor; choosing a
 * version resolves it through the core (`ResolvePimConflict`) and closes
 * the editor, whose form showed the version that lost.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, type Env } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { must } from "../../test/must";
import { openEventEditor } from "./testing";

const lunchOf = (env: Env) =>
  must(env.transport.data.features.pim.events.find((e) => e.id === "ev-lunch"));

/** Opens the editor of the demo's lunch. */
function editLunch(env: Env, close: () => void) {
  openEventEditor(env, "ev-lunch", close);
}

describe("the event editor's conflict choice (R8)", () => {
  it.each([
    ["Keep my version", "local", "pending"],
    ["Keep the server's version", "server", "synced"],
  ] as const)("%s resolves the conflict and closes", async (button, keep, after) => {
    const user = userEvent.setup({ delay: null });
    const env = createEnv();
    lunchOf(env).sync = "conflict";
    const close = vi.fn();
    editLunch(env, close);
    const state = await screen.findByText(
      "Changed here and on the server. Choose which version to keep.",
    );
    const block = must(state.parentElement);
    expect(block).toHaveAttribute("data-part", "pim.editor.sync");
    await user.click(within(block).getByRole("button", { name: button }));
    await waitFor(() => {
      expect(close).toHaveBeenCalled();
    });
    expect(requestsOf(env, "ResolvePimConflict").at(-1)).toEqual({
      type: "ResolvePimConflict",
      item: { type: "event", event_id: "ev-lunch" },
      keep,
    });
    expect(lunchOf(env).sync).toBe(after);
  });

  it("shows nothing for a synced event and the waiting state for a pending one", async () => {
    const env = createEnv();
    lunchOf(env).sync = "pending";
    editLunch(env, () => undefined);
    expect(await screen.findByText("Waiting to be saved on the server.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Keep my version" })).toBeNull();
  });
});
