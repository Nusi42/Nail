/**
 * Values of the nail-api contract the UI names and the generated bindings
 * do not carry as constants: one definition each (Q2, SSOT).
 */
import type { FolderRole } from "./generated";

/** `nail_api::AccountId::ALL`: the built-in "All accounts" virtual account. */
export const ALL_ACCOUNTS = "all";

/** Every folder role (`FolderRole`), in the order the UI lists them. */
export const FOLDER_ROLES: readonly FolderRole[] = [
  "inbox",
  "sent",
  "drafts",
  "archive",
  "junk",
  "trash",
];

/** Error codes of a core that does not have a request (an older core answers `invalid_request`). */
export const UNSUPPORTED_CODES: ReadonlySet<string> = new Set(["unsupported", "invalid_request"]);
