/**
 * Accessibility of the PIM UI (axe-core in jsdom, like `src/a11y.test.tsx`):
 * every calendar view, the small widgets, the contact and task widgets,
 * every dialog with real content (an occurrence of a recurring event, a
 * contact with every field, the merge choices, a task with subtasks), the
 * open date picker and the reminder toasts. Contrast needs a real layout
 * engine (Playwright).
 */
import { resetLocalTimeZone, setLocalTimeZone } from "@internationalized/date";
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import type { Density } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { BUILTIN_THEMES } from "../appearance";
import { resolveMockTheme } from "../appearance/themes";
import { WindowView } from "../../layout/LayoutRenderer";
import { compileTheme } from "../../theme/compile";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { dialogs, loadPimEditors } from ".";
import { fireReminder } from "./mock";
import { DIALOGS } from "./names";
import { PimToasts } from "./Reminders";
import { pimStoreFor } from "./store";
import { VIEW_KINDS } from "./time";

/** The value, or a test failure when it is missing. */
function must<T>(value: T | null | undefined): T {
  if (value === null || value === undefined) throw new Error("missing test data");
  return value;
}

/** Names of the test frames around the dialogs. */
const EVENT_LABEL = "event";
const MERGE_LABEL = "merge";

// Loading the lazy editor chunks can take long on a busy machine (the
// default hook timeout of 10 s once failed the whole file under load).
beforeAll(async () => {
  setLocalTimeZone("Europe/Berlin");
  await loadPimEditors();
}, 60_000);
afterAll(() => {
  resetLocalTimeZone();
});

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

describe("axe: calendar views", () => {
  it.each(VIEW_KINDS)(
    "view %s has no violations",
    async (view) => {
      const env = createEnv();
      const { container } = renderWithProviders(
        <NodeView
          node={widget("calendar", { options: { view, "secondary-zone": "America/New_York" } })}
          path="t/0"
        />,
        env,
      );
      await settle(env);
      expect(container.querySelector('[data-part="calendar"]')).not.toBeNull();
      expect(await violations(container)).toEqual([]);
      // The year view's twelve grids take over 20 s under full-suite load.
    },
    60_000,
  );
});

describe("axe: PIM widgets", () => {
  const kinds = [
    "mini-month",
    "agenda",
    "task-list",
    "contact-list",
    "contact-card",
    "calendar-list",
    "pim-settings",
  ];
  it.each(kinds)("%s has no violations", async (kind) => {
    const env = createEnv();
    pimStoreFor(env.store).getState().showContact("ct-ada");
    const { container } = renderWithProviders(
      <NodeView node={widget(kind, { options: { days: 3 } })} path="t/0" />,
      env,
    );
    await settle(env);
    expect(container.querySelector("[data-part]")).not.toBeNull();
    expect(await violations(container)).toEqual([]);
  });

  it("a contact with linked contacts and a sync conflict has no violations", async () => {
    const env = createEnv();
    const pim = env.transport.data.features.pim;
    const grace = must(pim.contacts.find((c) => c.id === "ct-grace"));
    grace.linked_ids = ["ct-grace-work"];
    grace.sync = "conflict";
    pimStoreFor(env.store).getState().showContact("ct-grace");
    const { container } = renderWithProviders(
      <NodeView node={widget("contact-card")} path="t/0" />,
      env,
    );
    await settle(env);
    expect(screen.getByRole("button", { name: "Unlink Grace B. Hopper" })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("axe: PIM dialogs", () => {
  async function prepare(env: Env, name: string): Promise<void> {
    const pim = pimStoreFor(env.store).getState();
    const instances = (
      await env.client.request(
        {
          type: "ListEventInstances",
          from: "2020-01-01T00:00:00Z",
          to: "2040-01-01T00:00:00Z",
          calendar_ids: ["cal-work"],
          tz: "Europe/Berlin",
        },
        "EventInstances",
      )
    ).instances.filter((i) => i.event_id === "ev-standup");
    const occurrence = must(instances[1]);
    switch (name) {
      case "pim-event-editor":
        pim.editEvent({
          mode: "edit",
          eventId: "ev-standup",
          recurrenceId: occurrence.recurrence_id,
          start: occurrence.start,
          end: occurrence.end,
        });
        break;
      case "pim-edit-scope":
        pim.setPendingChange({
          kind: "move",
          instance: occurrence,
          start: occurrence.start,
          end: occurrence.end,
        });
        break;
      case "pim-contact-editor":
        pim.editContact({ mode: "edit", contactId: "ct-ada" });
        break;
      case "pim-merge-contacts":
        pim.setMergeIds(["ct-grace", "ct-grace-work"]);
        break;
      case "pim-task-editor":
        pim.editTask({ mode: "edit", taskId: "tk-report" });
        break;
      case "pim-calendar-settings":
        pim.editCalendar("cal-work");
        break;
      case "pim-new-calendar":
        pim.setNewCalendar({ sourceId: "work-calendars", component: "task" });
        break;
      case "pim-remove-calendar":
        pim.setRemoveCalendar("cal-holidays");
        break;
    }
  }

  it.each([...DIALOGS])("dialog %s has no violations", async (name) => {
    const env = createEnv();
    await prepare(env, name);
    const Dialog = must(dialogs[name]);
    const { container } = renderWithProviders(
      <div role="dialog" aria-label={name}>
        <Dialog name={name} close={() => undefined} />
      </div>,
      env,
    );
    await settle(env);
    expect(container.querySelector("[data-part]")).not.toBeNull();
    expect(await violations(container)).toEqual([]);
  });

  it("the event editor's open date picker and filled lists have no violations", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await prepare(env, "pim-event-editor");
    const Dialog = must(dialogs["pim-event-editor"]);
    const { container } = renderWithProviders(
      <div role="dialog" aria-label={EVENT_LABEL}>
        <Dialog name="pim-event-editor" close={() => undefined} />
      </div>,
      env,
    );
    await settle(env);
    await user.click(screen.getByRole("radio", { name: "All occurrences" }));
    await user.click(screen.getByRole("button", { name: "Add a call link" }));
    await user.selectOptions(screen.getByLabelText("Also show the times in"), "Asia/Tokyo");
    await user.click(screen.getByRole("checkbox", { name: "The end is in another time zone" }));
    await user.click(screen.getByRole("button", { name: /Choose the date for Start/ }));
    expect(screen.getAllByRole("grid").length).toBeGreaterThan(0);
    expect(await violations(container)).toEqual([]);
    // The fullest dialog scan: ~14 s alone, over 20 s under full-suite load.
  }, 60_000);

  it("the merge dialog's list of duplicates has no violations", async () => {
    const env = createEnv();
    const Dialog = must(dialogs["pim-merge-contacts"]);
    const { container } = renderWithProviders(
      <div role="dialog" aria-label={MERGE_LABEL}>
        <Dialog name="pim-merge-contacts" close={() => undefined} />
      </div>,
      env,
    );
    await settle(env);
    expect(await screen.findByRole("button", { name: "Merge these" })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("reminder toasts have no violations", async () => {
    const env = createEnv();
    const { container } = renderWithProviders(<PimToasts />, env);
    const reminder = fireReminder(env.transport, "rem-review");
    act(() => {
      if (reminder)
        pimStoreFor(env.store).getState().pushToast({ kind: "reminder", id: "r1", reminder });
      pimStoreFor(env.store)
        .getState()
        .pushToast({ kind: "problem", id: "p1", message: { id: "pim-op-failed" } });
    });
    await settle(env);
    expect(screen.getByRole("button", { name: "Snooze" })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("axe: calendar, contacts and tasks in every theme (M5b)", () => {
  const DENSITIES: readonly Density[] = ["comfortable", "compact"];
  const CASES = BUILTIN_THEMES.flatMap(([id]) => DENSITIES.map((d) => [id, d] as const));
  // Each theme once for the (slow) axe scans, the densities alternating;
  // every theme and density compiles below.
  const SCANS = BUILTIN_THEMES.map(([id], i) => [id, DENSITIES[i % 2] ?? "comfortable"] as const);

  it("covers the five built-in themes", () => {
    expect(BUILTIN_THEMES.map(([id]) => id)).toEqual([
      "frost",
      "sage",
      "graphite",
      "nocturne",
      "high-contrast",
    ]);
  });

  it.each(CASES)("%s / %s compiles with the PIM skin and no dropped rules", (id, density) => {
    const theme = resolveMockTheme(defaultTheme(), id, { density, overrides: {} });
    const compiled = compileTheme(theme);
    expect(compiled.warnings).toEqual([]);
    expect(compiled.css).toContain('[data-part="calendar.toolbar"]');
  });

  it.each(SCANS)(
    "%s / %s: the PIM windows, an open calendar menu and the settings section",
    async (id, density) => {
      const theme = resolveMockTheme(defaultTheme(), id, { density, overrides: {} });
      for (const name of ["calendar", "contacts", "tasks"]) {
        const env = createEnv();
        pimStoreFor(env.store).getState().showContact("ct-ada");
        const { container, unmount } = renderWithProviders(
          <WindowView layout={env.transport.data.layout} name={name} />,
          env,
          { theme },
        );
        await settle(env);
        expect(container.querySelector("[data-part]"), name).not.toBeNull();
        expect(await violations(container), name).toEqual([]);
        unmount();
      }
      const user = userEvent.setup();
      const env = createEnv();
      const { container } = renderWithProviders(
        <>
          <NodeView node={widget("calendar-list")} path="t/0" />
          <NodeView node={widget("pim-settings")} path="t/1" />
        </>,
        env,
        { theme },
      );
      await settle(env);
      await user.click(must(screen.getAllByRole("button", { name: "Actions for Work" })[0]));
      expect(screen.getByRole("menu", { name: "Actions for Work" })).toBeInTheDocument();
      expect(await violations(container)).toEqual([]);
      await user.keyboard("{Escape}");
      await user.click(must(screen.getAllByRole("button", { name: "Add to On this computer" })[0]));
      expect(screen.getByRole("menuitem", { name: "New calendar" })).toBeInTheDocument();
      expect(await violations(container)).toEqual([]);
    },
    180_000,
  );
});
