/**
 * Test support for settings widgets (R6, ADR 0239): nothing applies until
 * the settings page's Save. `renderSettings` shows a widget the way the
 * page does — with the save bar, the close guard and the window layer (the
 * questions open as confirmation windows) — and the helpers press the
 * bar's Save / Discard or answer a question.
 */
import { screen, waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import type { ReactNode } from "react";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { withCloseGuard } from "../../test/closeGuard";
import { renderWithProviders, widget, type Env } from "../../test/harness";
import { SettingsSaveBar } from "../../widgets/dialog";
import { useSettingsGuard } from "./draft";

/** The section name the test bar shows ("Unsaved changes to “Test”"). */
export const TEST_SECTION = "Test";

/** The page's part of a test: its close guard (leaving asks). */
function PageGuard() {
  useSettingsGuard(TEST_SECTION);
  return null;
}

/** Renders `content` (default: the widget `kind`) with the save bar, the page's guard and the windows. */
export function renderSettings(env: Env, content: string | ReactNode) {
  withCloseGuard(env);
  return renderWithProviders(
    <>
      {typeof content === "string" ? <NodeView node={widget(content)} path="t" /> : content}
      <PageGuard />
      <SettingsSaveBar name={TEST_SECTION} />
      <WindowLayer layout={env.transport.data.layout} />
    </>,
    env,
  );
}

/** The save bar, while some widget has unsaved changes. */
export function saveBar(): HTMLElement | null {
  return screen.queryByRole("group", { name: /^Unsaved changes to “/ });
}

/** Presses the bar's Save (the bar must be there). */
export async function saveSettings(user: UserEvent): Promise<void> {
  const bar = await screen.findByRole("group", { name: /^Unsaved changes to “/ });
  await user.click(within(bar).getByRole("button", { name: "Save" }));
}

/** Presses Save and waits until everything is saved (the bar went). */
export async function saveSettingsDone(user: UserEvent): Promise<void> {
  await saveSettings(user);
  await waitFor(() => {
    expect(saveBar()).toBeNull();
  });
}

/** Presses the bar's Discard. */
export async function discardSettings(user: UserEvent): Promise<void> {
  const bar = await screen.findByRole("group", { name: /^Unsaved changes to “/ });
  await user.click(within(bar).getByRole("button", { name: "Discard" }));
}

/** The open question (a confirmation window) and its buttons. */
export async function settingsQuestion(): Promise<HTMLElement> {
  const dialogs = await screen.findAllByRole("dialog");
  const question = dialogs.find((d) => d.querySelector('[data-part="dialog.confirm"]') !== null);
  if (question === undefined) throw new Error("no question is open");
  return question;
}

/** Answers the open question with its verb `action`. */
export async function confirmSettingsQuestion(
  user: UserEvent,
  action: string | RegExp,
): Promise<void> {
  const question = await settingsQuestion();
  await user.click(within(question).getByRole("button", { name: action }));
}
