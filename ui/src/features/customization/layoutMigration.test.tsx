/**
 * The one-time offer of the M5b default layout to a layout of the user's
 * that predates the navigation rail (WP165, ADR 0114).
 */
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ResolvedLayout } from "../../api/generated";
import { normalizeLayout, normalizeNode } from "../../api/normalize";
import { defaultLayout } from "../../api/mockData";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../../test/harness";
import {
  DEFAULT_LAYOUT_TEXT,
  LayoutMigration,
  UI_STATE_LAYOUT_OFFER,
  needsLayoutMigration,
} from "./LayoutMigration";
import { MergeDialog } from "./MergeDialog";
import { DIALOGS } from "./names";
import { customizationStore } from "./state";

/** The default layout with a main window of the M5a kind (no view host). */
function oldLayout(): ResolvedLayout {
  const base = defaultLayout();
  const main = normalizeNode({
    type: "split",
    id: "panes",
    orientation: "horizontal",
    sizes: ["240px", "1fr"],
    children: [
      { type: "widget", kind: "folder-tree", id: "folders" },
      { type: "widget", kind: "message-list", id: "list" },
    ],
  });
  return normalizeLayout({
    ...base,
    windows: { ...base.windows, main: [{ min_width: null, max_width: null, root: main }] },
  });
}

const offer = () => screen.queryByRole("region", { name: "A new layout with a navigation rail" });

async function show(env: Env, layout: ResolvedLayout) {
  renderWithProviders(<LayoutMigration />, env, { layout });
  await settleQueries(env);
}

afterEach(() => {
  customizationStore.getState().reset();
});

describe("layout migration", () => {
  it("only a main window without a view host needs it", () => {
    expect(needsLayoutMigration(defaultLayout())).toBe(false);
    expect(needsLayoutMigration(oldLayout())).toBe(true);
    // A layout without views has nothing to offer.
    expect(needsLayoutMigration({ ...oldLayout(), views: {} })).toBe(false);
    // The offered text is the shipped default with the rail.
    expect(DEFAULT_LAYOUT_TEXT).toContain('layout "default"');
    expect(DEFAULT_LAYOUT_TEXT).toContain("view-host");
  });

  it("is not shown with the default layout", async () => {
    const env = createEnv();
    await show(env, defaultLayout());
    expect(offer()).toBeNull();
  });

  it("offers the new default once; Not now is remembered", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await show(env, oldLayout());
    const region = await screen.findByRole("region", {
      name: "A new layout with a navigation rail",
    });
    expect(region).toHaveTextContent("Your own layout keeps working");
    await user.click(screen.getByRole("button", { name: "Not now" }));
    await waitFor(() => {
      expect(offer()).toBeNull();
    });
    await settleQueries(env);
    const state = await env.client.request({ type: "GetUiState" }, "UiState");
    expect(state.state[UI_STATE_LAYOUT_OFFER]).toBe("dismissed");
  });

  it("stays away once answered", async () => {
    const env = createEnv();
    await env.client.request(
      { type: "SetUiState", key: UI_STATE_LAYOUT_OFFER, value: "reviewed" },
      "UiState",
    );
    await show(env, oldLayout());
    expect(offer()).toBeNull();
  });

  it("Review opens the merge dialog on the default layout, compared at once", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const request = vi.spyOn(env.client, "request");
    await show(env, oldLayout());
    await user.click(await screen.findByRole("button", { name: "Review the new layout" }));
    expect(env.store.getState().windows).toContain(DIALOGS.merge);
    expect(customizationStore.getState().mergePrefill).toEqual({
      file: "layout",
      incoming: DEFAULT_LAYOUT_TEXT,
    });
    await waitFor(() => {
      expect(offer()).toBeNull();
    });

    // The dialog takes the prefill and compares it without a click.
    renderWithProviders(<MergeDialog name={DIALOGS.merge} close={vi.fn()} />, env, {
      layout: oldLayout(),
    });
    await waitFor(() => {
      expect(request).toHaveBeenCalledWith(
        { type: "PreviewConfigMerge", file: "layout", incoming: DEFAULT_LAYOUT_TEXT },
        "ConfigMergePreview",
      );
    });
    expect(customizationStore.getState().mergePrefill).toBeNull();
    await act(async () => {
      await settleQueries(env);
    });
    const state = await env.client.request({ type: "GetUiState" }, "UiState");
    expect(state.state[UI_STATE_LAYOUT_OFFER]).toBe("reviewed");
  });
});
