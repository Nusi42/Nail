/**
 * Feature slot of M2a WP34 (security UI, ADR 0029): key and certificate
 * manager, per-identity crypto settings, composer sign / encrypt toggles
 * with recipient key status, the message security and auth-results badges
 * with details, link and attachment warnings in the reader, the
 * encryption-at-rest settings with progress, the Autocrypt setup flow.
 *
 * Only WP34 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/security.rs` for widget kinds and
 * `locales/{en,de}/security.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Feature } from "../types";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers (prefix `security.`). */
export const actions: Feature["actions"] = {};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {};

/** Handlers for core events (`KeysChanged`, `VaultProgress`, …). */
export const events: Feature["events"] = [];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [];
