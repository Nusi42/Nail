/**
 * Icons for place and call (R8, ADR 0251): the event editor's place field
 * and the quick look's place show the place icon (`PLACE_ICON`, Lucide
 * `map-pin`), the quick look's Join the call icon. A theme may draw any
 * icon itself; here it draws the place icon, so the test does not depend
 * on which Lucide icons the app bundles.
 */
import { act, screen, within } from "@testing-library/react";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { EventPopoverHost } from "./EventPopover";
import { CALL_ICON, PLACE_ICON } from "./eventLook";
import { pimStoreFor } from "./store";
import { instancesOf, openEventEditor } from "./testing";

/** A demo with the place icon drawn by the theme. */
function envWithPlaceIcon(): Env {
  const env = createEnv();
  env.transport.data.theme.icons[PLACE_ICON] = { view_box: "0 0 24 24", path: "M12 2v20" };
  return env;
}

describe("icons for place and call (R8)", () => {
  it("are the Lucide map pin and video camera", () => {
    expect(PLACE_ICON).toBe("map-pin");
    expect(CALL_ICON).toBe("video");
  });

  it("marks the editor's place field", async () => {
    const env = envWithPlaceIcon();
    openEventEditor(env, "ev-lunch");
    const place = await screen.findByPlaceholderText(/place/i);
    const field = must(place.closest('[data-part="pim.compact.place"]'));
    expect(field.querySelector(`svg[data-icon="${PLACE_ICON}"]`)).toHaveAttribute(
      "data-part",
      "pim.compact.place.icon",
    );
  });

  it("marks the quick look's place and its Join link", async () => {
    const env = envWithPlaceIcon();
    renderWithProviders(<EventPopoverHost />, env);
    const standup = must((await instancesOf(env, "ev-standup"))[0]);
    act(() => {
      pimStoreFor(env.store)
        .getState()
        .setQuickLook({ instance: standup, anchor: { left: 0, top: 0, width: 10, height: 10 } });
    });
    await settleQueries(env);
    const popover = screen.getByRole("dialog", { name: "Stand-up" });
    const place = must(popover.querySelector('[data-part="pim.popover.place"]'));
    expect(place).toHaveTextContent("Room 4");
    expect(place.querySelector(`svg[data-icon="${PLACE_ICON}"]`)).not.toBeNull();
    const join = within(popover).getByRole("link", { name: "Join" });
    expect(join.querySelector(`svg[data-icon="${CALL_ICON}"]`)).not.toBeNull();
  });
});
