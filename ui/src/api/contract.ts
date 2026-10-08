/**
 * Values of the nail-api contract the UI names and the generated bindings
 * do not carry as constants: one definition each (Q2, SSOT).
 */
import { FOLDER_ROLE_VALUES, type ErrorCode, type FolderRole } from "./generated";

/** `nail_api::AccountId::ALL`: the built-in "All accounts" virtual account. */
export { ALL_ACCOUNTS } from "./generated";

/** The special-use folder roles (every `FolderRole` but `other`), in the contract's order. */
export const FOLDER_ROLES: readonly FolderRole[] = FOLDER_ROLE_VALUES.filter(
  (role) => role !== "other",
);

/** Error codes of a core that does not have a request (an older core answers `invalid_request`). */
export const UNSUPPORTED_CODES: ReadonlySet<ErrorCode> = new Set<ErrorCode>([
  "unsupported",
  "invalid_request",
]);
