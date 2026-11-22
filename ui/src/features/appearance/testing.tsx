/**
 * Test helpers of the appearance feature (only tests import this file).
 */
import { fireEvent, screen, waitFor, within } from "@testing-library/react";
import { renderWithProviders, widget, type Env } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { SettingsSaveBar } from "../../widgets/dialog";

/**
 * Renders the Appearance section as the settings page shows it: the
 * widget and the section's "Unsaved changes" bar (R6, ADR 0239).
 */
export function renderAppearance(env: Env) {
  return renderWithProviders(
    <>
      <NodeView node={widget("appearance-settings")} path="test" />
      <SettingsSaveBar name="Appearance" />
    </>,
    env,
  );
}

/** The section's "Unsaved changes" bar (`null` while nothing is unsaved). */
export function saveBar(): HTMLElement | null {
  return screen.queryByRole("group", { name: "Unsaved changes to “Appearance”" });
}

/** Saves the section with the bar's Save and waits until the bar is gone. */
export async function saveSection(): Promise<void> {
  const bar = await screen.findByRole("group", { name: "Unsaved changes to “Appearance”" });
  fireEvent.click(within(bar).getByRole("button", { name: "Save" }));
  await waitFor(() => {
    expect(saveBar()).toBeNull();
  });
}
