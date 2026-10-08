/**
 * Names of the connection log feature (ADR 0190): window kinds (titles
 * `window-<kind>` in `connlog.ftl`) and root parts. Limits and file names
 * come from the core (`ConnectionLog.capacity`, `ConnectionLogExport.file_name`).
 */

/** The connection log viewer: `connection-log:<scope instance>`. */
export const CONNECTION_LOG_KIND = "connection-log";

/** One error's details: `error-details:<n>`. */
export const ERROR_DETAILS_KIND = "error-details";

/** The application log's recent lines (Settings → Diagnostics). */
export const APP_LOG_KIND = "application-log";

/** Root part of the viewer. */
export const VIEWER_PART = "connection-log";
/** Root part of the details window. */
export const DETAILS_PART = "error-details";
/** Root part of the account's connection section (Settings → Accounts). */
export const ACCOUNT_PART = "connection-log-account";
/** Root part of the log settings (Settings → Diagnostics). */
export const SETTINGS_PART = "connection-log-settings";
/** Root part of the application log window. */
export const APP_LOG_PART = "application-log";
