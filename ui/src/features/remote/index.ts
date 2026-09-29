/**
 * Feature slot of M2b WP43 (remote UI and privacy UI, ADR 0036): login
 * (passkey first, password + TOTP), pairing on both sides (the desktop
 * shows the code and the certificate fingerprint, the browser enters the
 * code), sessions and devices, remote-UI settings with internet mode shown
 * as locked with its reason, the "load remote content" bar in the reader
 * with per-sender / per-domain choices, and the security audit view.
 *
 * Only WP43 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/remote.rs` for widget kinds and
 * `locales/{en,de}/remote.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`. The browser's
 * login page talks to `POST /api/auth` (`AuthRequest` / `AuthResponse`),
 * not to `NailClient`: it runs before there is a session.
 */
import type { Feature } from "../types";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers (prefix `remote.`). */
export const actions: Feature["actions"] = {};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {};

/**
 * Handlers for core events (`RemoteUiStatusChanged`, `RemoteAccessChanged`,
 * `RemoteSessionEnded`, `RemoteSecurityAlert`, `RemoteContentPolicyChanged`).
 */
export const events: Feature["events"] = [];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [];
