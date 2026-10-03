/**
 * Feature slot of M5c WP173 (contract ADR 0116): pop-out windows in the
 * desktop app — "Open in new window" for windows whose spec allows it
 * (`WindowSpec.pop_out`), a second webview on the same core connection that
 * shows one window (route `#/window/<id>`, `PopOutHost.tsx`, rendered by
 * `App.tsx` instead of the main window), "Return to main window", geometry
 * remembered per pop-out (the core keeps it per kind from the shell's
 * reports). The browser UI keeps windows in the page (`ListPopOuts` is
 * empty, `PopOutWindow` answers `unsupported`).
 *
 * The contract: requests and hooks in `../../state/windowState.ts`
 * (`usePopOuts`, `usePopOutActions`; `PopOutChanged` applied in `App.tsx`),
 * routes and labels in `../../state/windows.ts`; the mock simulates a
 * desktop host with `enableMockPopOuts` / `simulatePopOutReport`
 * (`../../api/windowStateMock.ts`). WP173 also owns `app/src-tauri`
 * (`DesktopHost::open_pop_out` / `close_pop_out` / `focus_pop_out`,
 * `DesktopChange::PopOut` reports, the `popout-*` windows' capability).
 * Texts: `locales/{en,de}/popout.ftl` (contract ids in `core-windows.ftl`).
 *
 * Only WP173 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

export { PopOutHost } from "./PopOutHost";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialogs by name. */
export const dialogs: Feature["dialogs"] = {};

/** Core event handlers. */
export const events: Feature["events"] = [];

/** Background components. */
export const effects: Feature["effects"] = [];
