/**
 * R6 (ADR 0239): nothing in a settings section applies until Save. The
 * page shows the save bar while a widget has unsaved changes and asks with
 * the shared prompt before another section shows (also through the
 * `settings.section` action).
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActionContext } from "../../actions/registry";
import { promptWindowFor, MAIN_GUARD } from "../../layout/closeGuard";
import { withCloseGuard } from "../../test/closeGuard";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { UI_STATE_SETTINGS_SECTION } from "../../state/uiState";
import {
  createEnv,
  renderWithProviders,
  settleQueries,
  stubElementSizesEachTest,
  widget,
  type Env,
} from "../../test/harness";
import { useSettingsDraft, type SettingsDraft } from "../../widgets/dialog";
import { actions } from ".";

stubElementSizesEachTest();

/** A widget's draft, changed by the test. */
let draft: SettingsDraft;

function Probe() {
  useSettingsDraft("probe", draft);
  return null;
}

async function setup(): Promise<Env> {
  const env = createEnv();
  withCloseGuard(env);
  renderWithProviders(
    <>
      <NodeView node={widget("settings-page")} path="test" />
      <Probe />
      <WindowLayer layout={env.transport.data.layout} />
    </>,
    env,
  );
  await settleQueries(env);
  return env;
}

const shownSection = () =>
  document.querySelector('[data-part="settings-page.content"]')?.getAttribute("data-section");

const nav = () => screen.getByRole("navigation", { name: "Settings sections" });

/** A navigation button of a section other than the shown one. */
function otherSection(): HTMLElement {
  const shown = shownSection();
  const other = within(nav())
    .getAllByRole("button")
    .find((b) => b.getAttribute("data-section") !== shown);
  if (other === undefined) throw new Error("a second section");
  return other;
}

beforeEach(() => {
  draft = { dirty: true, save: vi.fn(() => true), discard: vi.fn() };
});

describe("settings save bar (R6)", () => {
  it("shows the bar and asks before another section shows", async () => {
    const user = userEvent.setup();
    const env = await setup();
    const first = shownSection();
    const bar = await screen.findByRole("group", { name: /^Unsaved changes to “/ });
    expect(within(bar).getByRole("button", { name: "Save" })).toBeEnabled();
    const other = otherSection();
    await user.click(other);
    const question = await screen.findByRole("dialog", { name: "Unsaved changes" });
    expect(env.store.getState().windows).toEqual([promptWindowFor(MAIN_GUARD)]);
    expect(shownSection()).toBe(first);
    // Cancel: the section stays, with its changes.
    await user.click(within(question).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(shownSection()).toBe(first);
    expect(draft.discard).not.toHaveBeenCalled();
    // Don't save: the changes are dropped and the other section shows.
    await user.click(other);
    await user.click(
      within(await screen.findByRole("dialog", { name: "Unsaved changes" })).getByRole("button", {
        name: "Don't save",
      }),
    );
    expect(draft.discard).toHaveBeenCalledOnce();
    await waitFor(() => {
      expect(shownSection()).toBe(other.getAttribute("data-section"));
    });
    await settleQueries(env);
  });

  it("saves from the prompt, and a deep link asks too", async () => {
    const user = userEvent.setup();
    const env = await setup();
    const ctx: ActionContext = {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      layout: env.transport.data.layout,
      t: (id) => id,
      locale: "en",
    };
    const target = env.transport.data.layout.settings.at(-1)?.sections.at(-1)?.id ?? "";
    await act(() => actions["settings.section"]?.run(ctx, [target]));
    const question = await screen.findByRole("dialog", { name: "Unsaved changes" });
    await user.click(within(question).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(shownSection()).toBe(target);
    });
    expect(draft.save).toHaveBeenCalledOnce();
    const state = await env.client.call({ type: "GetUiState" });
    expect(state.type === "UiState" && state.state[UI_STATE_SETTINGS_SECTION]).toBe(target);
  });

  it("moves on at once without unsaved changes", async () => {
    draft = { ...draft, dirty: false };
    const user = userEvent.setup();
    const env = await setup();
    expect(screen.queryByRole("group", { name: /^Unsaved changes to “/ })).toBeNull();
    const other = otherSection();
    await user.click(other);
    await waitFor(() => {
      expect(shownSection()).toBe(other.getAttribute("data-section"));
    });
    expect(env.store.getState().windows).toEqual([]);
    await settleQueries(env);
  });
});
