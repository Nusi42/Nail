import { act, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { EventInstance } from "../../api/generated";
import { createEnv, renderWithProviders } from "../../test/harness";
import { EventPopoverHost } from "./EventPopover";
import { pimStoreFor } from "./store";

const instance: EventInstance = {
  event_id: "ev-kickoff",
  calendar_id: "cal-work",
  recurrence_id: null,
  start: { type: "zoned", local: "2026-11-27T15:00:00", tz: "UTC" },
  end: { type: "zoned", local: "2026-11-27T16:00:00", tz: "UTC" },
  start_utc: "2026-11-27T15:00:00Z",
  end_utc: "2026-11-27T16:00:00Z",
  all_day: false,
  title: "Project kickoff",
  location: null,
  status: "confirmed",
  transparency: "busy",
  color: null,
  recurring: false,
  overridden: false,
  has_attendees: true,
  has_alarms: false,
  read_only: false,
  my_status: null,
};

describe("the quick look host", () => {
  it("shows nothing until a view opens a quick look, then closes it on Escape", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderWithProviders(<EventPopoverHost />, env);
    expect(screen.queryByRole("dialog")).toBeNull();
    const pim = pimStoreFor(env.store);
    act(() => {
      pim
        .getState()
        .setQuickLook({ instance, anchor: { left: 10, top: 10, width: 80, height: 40 } });
    });
    const popover = screen.getByRole("dialog", { name: "Project kickoff" });
    expect(popover).toHaveAttribute("data-part", "pim.popover");
    await user.keyboard("{Escape}");
    expect(pim.getState().quickLook).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
