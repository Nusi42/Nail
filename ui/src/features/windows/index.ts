/**
 * Feature slot of M5c WP171 (contract ADR 0116): the window manager — move
 * by the title bar, resize from 8 handles, maximize / restore (double-click),
 * snap to the left or right half, keyboard move / resize (arrow keys and
 * Shift+arrow keys on a focused title bar, Alt+F7 / Alt+F8 equivalents),
 * focus order and z-order, minimum sizes, staying on screen when the page
 * shrinks, and remembered geometry.
 *
 * The contract: window ids, specs and checks in `../../state/windows.ts`
 * (mirrors `nail_api::windows`; `resolveWindowSpec` = defaults →
 * `Feature.windows` → the layout's `window` properties), remembered state in
 * `../../state/windowState.ts` (`useWindowState`, `useSetWindowState`, per
 * kind), z-order = `UiState.windows` (topmost last; `focusWindow`), the
 * window instance of a component = `WindowInstanceContext`. WP171 also owns
 * `../../layout/` (the window layer `WindowLayer.tsx` and the view
 * components). Texts: `locales/{en,de}/windows.ftl` (contract ids in
 * `core-windows.ftl`).
 *
 * Only WP171 edits the files in this directory. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {};

/** Dialogs by name. */
export const dialogs: Feature["dialogs"] = {};

/** Window model registrations by kind. */
export const windows: NonNullable<Feature["windows"]> = {};

/** Core event handlers. */
export const events: Feature["events"] = [];

/** Background components. */
export const effects: Feature["effects"] = [];
