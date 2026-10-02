/**
 * Names this feature registers: layout widget kinds (also in
 * `crates/nail-config/src/widgets/system.rs`), dialog names (window titles
 * `window-<name>` in `system.ftl`) and action ids.
 */

/** Widget: updates (version, channel, check now, release notes, install). */
export const UPDATE_SETTINGS_WIDGET = "update-settings";
/** Widget: tray and startup, the `nail plugin dev` endpoint. */
export const DESKTOP_SETTINGS_WIDGET = "desktop-settings";
/** Widget: default apps and Simple MAPI, "Choose in Default apps". */
export const DEFAULT_APPS_WIDGET = "default-apps";

/** Dialog: "About Nail" (version, installation, updates). */
export const ABOUT_DIALOG = "about-nail";
/** Dialog: the first-run screen. */
export const FIRST_RUN_DIALOG = "first-run";

/** Action: opens "About Nail". */
export const ABOUT_ACTION = "system.about";
/** Action: checks for updates now. */
export const CHECK_UPDATES_ACTION = "system.check-updates";
/** Action: opens the first-run screen again. */
export const FIRST_RUN_ACTION = "system.first-run";
/** Action: opens Windows Settings → Default apps. */
export const DEFAULT_APPS_ACTION = "system.default-apps";

/**
 * The import wizard's action id (M4c WP124). The first-run screen offers
 * "Import" only while an action with this id is registered.
 */
export const IMPORT_WIZARD_ACTION = "import.open";
