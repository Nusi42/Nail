/**
 * Feature slot of M5c WP173 (contract ADR 0116): pop-out windows in the
 * desktop app — "Open in new window" for windows whose spec allows it
 * (`WindowSpec.pop_out`), a second webview on the same core connection that
 * shows one window (route `#/window/<id>`, `PopOutHost.tsx`, rendered by
 * `App.tsx` instead of the main window), "Return to main window", geometry
 * remembered per pop-out (the core keeps it per kind from the shell's
 * reports). The browser UI keeps windows in the page (`ListPopOuts` is
 * empty, `PopOutWindow` answers `unsupported`): the button is hidden there.
 *
 * What this feature contributes:
 * - `PopOutButton` — the title-bar button (`window.pop-out`); the window
 *   manager's title bar (`../../layout/`, WP171) renders it per window.
 * - `PopOutHost` — the pop-out page; `usePopOutTitle` names its OS window
 *   (taskbar), `usePopOutContext` reads the context it was opened with.
 * - `registerPopOutPreparer(kind, fn)` — what a kind does before it pops
 *   out (the composer saves its draft and returns `{draft}`).
 * - actions `popout.open` (the focused window, or the id given) and
 *   `popout.return` (the pop-out page's window, or the id given);
 * - the event handler that takes a popped-out window off the main page and
 *   puts a returned one back; the effect `PopOutRouter` that sends windows
 *   opened in the page to their OS window when they are (or were last)
 *   popped out.
 *
 * The contract: requests and hooks in `../../state/windowState.ts`
 * (`usePopOuts`, `usePopOutActions`; `PopOutChanged` applied in `App.tsx`),
 * routes and labels in `../../state/windows.ts`; the mock simulates a
 * desktop host with `enableMockPopOuts` / `simulatePopOutReport`
 * (`../../api/windowStateMock.ts`). WP173 also owns `app/src-tauri`
 * (`DesktopHost::open_pop_out` / `close_pop_out` / `focus_pop_out`,
 * `DesktopChange::PopOut` reports, the `popout-*` windows' capability).
 * Texts: `locales/{en,de}/popout.ftl` (contract ids in `core-windows.ftl`);
 * styles: `parts.ts` (`POPOUT_BASE_CSS`).
 *
 * Only WP173 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import type { JsonValue } from "../../api/generated";
import type { Feature } from "../types";
import {
  POP_OUT_ACTION,
  RETURN_ACTION,
  pageEffectOf,
  popOutPageWindow,
  popOutWindow,
  returnPopOut,
  windowTitleId,
} from "./model";
import { PopOutRouter } from "./PopOutRouter";
import { resolveWindowSpec } from "../../state/windows";

export { PopOutHost } from "./PopOutHost";
export { PopOutButton } from "./PopOutButton";
export { usePopOutContext, usePopOutTitle, usePopOutsAvailable } from "./hooks";
export {
  POP_OUT_ACTION,
  RETURN_ACTION,
  isPopOutPage,
  popOutPageWindow,
  registerPopOutPreparer,
  type PopOutPreparer,
} from "./model";
export { POPOUT_BASE_CSS } from "./parts";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

function windowArg(args: readonly JsonValue[]): string | null {
  const [first] = args;
  return typeof first === "string" && first !== "" ? first : null;
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [POP_OUT_ACTION]: {
    run: async (ctx, args) => {
      const window = windowArg(args) ?? ctx.store.getState().windows.at(-1);
      if (window === undefined || popOutPageWindow() !== null) return;
      // Only windows that may pop out (opt-in: the layout's `pop-out=#true`;
      // feature dialogs and confirmations stay in the page).
      const spec = resolveWindowSpec(window, ctx.layout, {});
      if (!spec.pop_out) return;
      await popOutWindow(ctx, window, ctx.t(windowTitleId(window)), spec).catch(() => undefined);
    },
    enabled: (ctx, args) => {
      if (popOutPageWindow() !== null) return false;
      const window = windowArg(args) ?? ctx.store.getState().windows.at(-1);
      return window !== undefined && resolveWindowSpec(window, ctx.layout, {}).pop_out;
    },
  },
  [RETURN_ACTION]: {
    run: async (ctx, args) => {
      const window = windowArg(args) ?? popOutPageWindow();
      if (window === null) return;
      await returnPopOut(ctx, window).catch(() => undefined);
    },
    enabled: (_ctx, args) => windowArg(args) !== null || popOutPageWindow() !== null,
  },
};

/** Command-palette descriptions. */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [POP_OUT_ACTION]: {
    title: "popout-action-open",
    keywords: "popout-action-keywords",
    availability: ["desktop"],
    icon: "external-link",
  },
  [RETURN_ACTION]: {
    title: "popout-action-return",
    keywords: "popout-action-keywords",
    availability: ["desktop"],
    icon: "app-window",
  },
};

/** Core event handlers. */
export const events: NonNullable<Feature["events"]> = [
  // The main page: a window that popped out leaves it, a returned one comes back.
  (event, { store }) => {
    if (event.type !== "PopOutChanged" || popOutPageWindow() !== null) return;
    const state = store.getState();
    switch (pageEffectOf(event.change)) {
      case "close":
        if (state.windows.includes(event.window)) state.closeWindow(event.window);
        break;
      case "open":
        state.openWindow(event.window);
        break;
      default:
        break;
    }
  },
];

/** Background components. */
export const effects: NonNullable<Feature["effects"]> = [PopOutRouter];
