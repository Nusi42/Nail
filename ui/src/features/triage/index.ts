/**
 * Feature slot of M2c WP54 for junk mail, snooze and notifications
 * (contract ADR 0043): "Mark as junk / not junk" and the classifier's
 * settings and status, snoozing and the snoozed list, new-mail
 * notification settings (per account / folder, quiet hours) and showing
 * `NotificationRequested` as a Web Notification in the remote UI (the
 * desktop shell shows OS notifications itself).
 *
 * Created empty by WP50. Only WP54 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/triage.rs` for widget kinds and
 * `locales/{en,de}/triage.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};
/** Action ids → handlers. */
export const actions: Feature["actions"] = {};
/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {};
/** Handlers for the core's events. */
export const events: Feature["events"] = [];
/** Background components. */
export const effects: Feature["effects"] = [];
