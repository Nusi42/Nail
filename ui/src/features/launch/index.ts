/**
 * Launch requests of the desktop app (M2b integration, ADR 0041): `mailto:`
 * links and opened `.eml` / `.ics` / `.vcf` files. See `LaunchRequests.tsx`.
 */
import type { Feature } from "../types";
import { LaunchRequests } from "./LaunchRequests";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {};
export const dialogs: Feature["dialogs"] = {};
export const events: Feature["events"] = [];
/** Takes the pending launch requests at start-up and on `LaunchRequested`. */
export const effects: Feature["effects"] = [LaunchRequests];

export { handleLaunch, LaunchRequests, type LaunchNotice } from "./LaunchRequests";
