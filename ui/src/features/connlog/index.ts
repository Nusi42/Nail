/**
 * Feature slot of the connection log (ADR 0190): what Nail said to the
 * mail servers and what they answered, without mail content or
 * credentials (the core redacts every line before it reaches the UI).
 *
 * - The viewer window `connection-log:<scope>` (`./ConnectionLogWindow.tsx`):
 *   filters by protocol and level, Pause, Copy / Export (the text the core
 *   formats, `ExportConnectionLog`; desktop: Save dialog, else a browser
 *   download), Clear; follows `ConnectionLogChanged`.
 * - "Show details" (`ShowDetailsButton`) next to every error the UI shows
 *   (status bar: sync, account, send) opens `error-details:<n>`
 *   (`./ErrorDetails.tsx`): step, protocol, folder, command, the server's
 *   answer verbatim (redacted), time, error class; Copy details, Show in
 *   connection log.
 * - Settings (`./Settings.tsx`): Accounts → the account's "Connection log"
 *   and recent problems (`ListSyncErrors`, refreshed by
 *   `SyncErrorsChanged`); Diagnostics → "Keep connection log on disk" and
 *   the application log (window `application-log`).
 * - The mock core answers through `./mock.ts`; the look is `./skin.ts`
 *   (`CONNLOG_SKIN_CSS`, in the theme compiler's skin list).
 *
 * Texts: `locales/{en,de}/connlog.ftl` (contract ids in `core-connlog.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { Feature } from "../types";
import { ConnectionLogWindow } from "./ConnectionLogWindow";
import { ErrorDetailsWindow } from "./ErrorDetails";
import { APP_LOG_KIND, CONNECTION_LOG_KIND, ERROR_DETAILS_KIND } from "./names";
import { handleConnlogEvent } from "./queries";
import { AppLogWindow } from "./Settings";

export * from "./names";
export { ShowDetailsButton } from "./ErrorDetails";
export { AccountConnection, ConnectionLogSettingsCard } from "./Settings";
export { CONNLOG_SKIN_CSS } from "./skin";
export { openConnectionLog, openErrorDetails, type ErrorReport } from "./store";

/** Layout widget kinds → components (none: the parts live in other widgets). */
export const widgets: NonNullable<Feature["widgets"]> = {};

/** Action ids → handlers. */
export const actions: NonNullable<Feature["actions"]> = {};

/** Dialogs by window kind. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [CONNECTION_LOG_KIND]: ConnectionLogWindow,
  [ERROR_DETAILS_KIND]: ErrorDetailsWindow,
  [APP_LOG_KIND]: AppLogWindow,
};

/** The window model: non-modal tool windows, one viewer per scope. */
export const windows: NonNullable<Feature["windows"]> = {
  [CONNECTION_LOG_KIND]: {
    modal: false,
    size: { width: 900, height: 640 },
    min_size: { width: 420, height: 360 },
    instances: "multiple",
    pop_out: false,
  },
  [ERROR_DETAILS_KIND]: {
    modal: false,
    size: { width: 620, height: 520 },
    min_size: { width: 360, height: 320 },
    instances: "multiple",
    pop_out: false,
  },
  [APP_LOG_KIND]: {
    modal: false,
    size: { width: 900, height: 600 },
    min_size: { width: 420, height: 320 },
    pop_out: false,
  },
};

/** Core event handlers: new log entries, changed sync errors. */
export const events: NonNullable<Feature["events"]> = [handleConnlogEvent];

/** Background components. */
export const effects: NonNullable<Feature["effects"]> = [];
