/**
 * Plug-in dialogs and the send veto as windows of the window manager (M5c,
 * ADR 0121): every dialog of the app is a window, none keeps a fixed modal
 * frame of its own.
 *
 * - A plug-in dialog (`nail.ui.showDialog`) opens the window
 *   `plugin-dialog-<size>:<key>` (non-modal, `modal: false`) or
 *   `plugin-modal-<size>:<key>` (a modal confirmation — what it blocks is
 *   the window manager's rule: the window it was opened from, else the
 *   main window); `<size>` is the dialog's `small` / `medium` / `large`,
 *   `<key>` its dialog id made an instance key.
 * - The send veto ("Send anyway?") is the modal confirmation
 *   `plugin-send-veto` (it blocks the composer it came from).
 *
 * Titles come from the dialog itself (`windowTitles`), kept here by window
 * id when the window opens.
 */
import type { PluginDialog, PluginDialogSize } from "../../api/generated";
import { windowId, windowKind } from "../../state/windows";
import type { WindowRegistration } from "../../state/windows";

/** The send veto's window. */
export const SEND_VETO_WINDOW = "plugin-send-veto";

const SIZES: Readonly<Record<PluginDialogSize, { width: number; height: number }>> = {
  small: { width: 420, height: 300 },
  medium: { width: 560, height: 440 },
  large: { width: 760, height: 600 },
};

/** The window kind of a plug-in dialog. */
export function pluginDialogKind(dialog: Pick<PluginDialog, "spec">): string {
  const size = dialog.spec.size ?? "medium";
  return `${dialog.spec.modal === false ? "plugin-dialog" : "plugin-modal"}-${size}`;
}

/** The window kinds of plug-in dialogs. */
export const PLUGIN_DIALOG_KINDS: readonly string[] = (["dialog", "modal"] as const).flatMap(
  (mode) => (Object.keys(SIZES) as PluginDialogSize[]).map((size) => `plugin-${mode}-${size}`),
);

/** Whether `window` is a plug-in dialog's window. */
export function isPluginDialogWindow(window: string): boolean {
  return PLUGIN_DIALOG_KINDS.includes(windowKind(window));
}

/** The instance key of a dialog id (`[A-Za-z0-9_-]{1,64}`). */
export function dialogKey(dialogId: string): string {
  const key = dialogId.replace(/[^A-Za-z0-9_-]/g, "-").slice(0, 64);
  return key === "" ? "dialog" : key;
}

/** The window id of a plug-in dialog. */
export function pluginDialogWindow(dialog: Pick<PluginDialog, "spec" | "dialog_id">): string {
  return windowId(pluginDialogKind(dialog), dialogKey(dialog.dialog_id));
}

/** The window model: sizes per kind; the modal kinds are confirmations. */
export const pluginWindows: Readonly<Record<string, WindowRegistration>> = {
  ...Object.fromEntries(
    (Object.entries(SIZES) as [PluginDialogSize, { width: number; height: number }][]).flatMap(
      ([size, box]) => {
        const min = { width: Math.min(360, box.width), height: Math.min(240, box.height) };
        return [
          [
            `plugin-dialog-${size}`,
            { size: box, min_size: min, instances: "multiple", pop_out: false },
          ],
          [
            `plugin-modal-${size}`,
            {
              confirmation: true,
              resizable: true,
              size: box,
              min_size: min,
              instances: "multiple",
            },
          ],
        ];
      },
    ),
  ),
  [SEND_VETO_WINDOW]: {
    confirmation: true,
    resizable: true,
    size: { width: 520, height: 380 },
    min_size: { width: 400, height: 280 },
  },
};

/** Window titles the dialogs gave (by window id). */
const titles = new Map<string, string>();

/** Records the title of a plug-in window (before it opens). */
export function setPluginWindowTitle(window: string, title: string | null): void {
  if (title === null) titles.delete(window);
  else titles.set(window, title);
}

/** The title of a plug-in window, `null` = the kind's own text. */
export function pluginWindowTitle(_state: unknown, window: string): string | null {
  return titles.get(window) ?? null;
}
