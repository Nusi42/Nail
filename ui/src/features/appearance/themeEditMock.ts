/**
 * The mock core's theme editor requests (M5d, ADR 0131): `GetThemeDetails`,
 * `EditTheme`, `DuplicateTheme`, `RenameTheme`, `DeleteTheme`. Edits are
 * recorded in the `appearance` undo history with
 * `recordMockEdit(core, "appearance", kind, before)` (the appearance mock
 * registers the scope's snapshot, `./mock.ts`).
 *
 * Slot of WP183: until it lands every request answers like the contract's
 * core — `not_found` (`edit-issue-theme-not-found`) for an unknown theme,
 * `invalid_request` (`edit-issue-not-editable`) for a built-in theme where a
 * user theme is needed, else `unsupported` (`edit-issue-not-ready`).
 */
import type { EditIssue, Request, Response } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";

/** The requests answered here. */
export const THEME_EDIT_REQUESTS = [
  "GetThemeDetails",
  "EditTheme",
  "DuplicateTheme",
  "RenameTheme",
  "DeleteTheme",
] as const satisfies readonly Request["type"][];

export type ThemeEditRequest = Extract<Request, { type: (typeof THEME_EDIT_REQUESTS)[number] }>;

export function isThemeEditRequest(request: Request): request is ThemeEditRequest {
  return (THEME_EDIT_REQUESTS as readonly string[]).includes(request.type);
}

function error(
  code: Extract<Response, { type: "Error" }>["code"],
  issue: EditIssue,
  theme: string,
): Response {
  return {
    type: "Error",
    code,
    message: `${issue}: ${theme}`,
    localized: { id: `edit-issue-${issue}`, args: { theme } },
  };
}

/** The theme a request names, and whether it must be a user theme. */
function target(request: ThemeEditRequest): { id: string; userOnly: boolean } | null {
  switch (request.type) {
    case "GetThemeDetails":
    case "DuplicateTheme":
      return { id: request.theme_id, userOnly: false };
    case "RenameTheme":
    case "DeleteTheme":
      return { id: request.theme_id, userOnly: true };
    case "EditTheme":
      return request.target.kind === "theme"
        ? { id: request.target.theme_id, userOnly: true }
        : null;
  }
}

export function answerThemeEdit(core: MockCore, request: ThemeEditRequest): Response {
  const named = target(request);
  if (named) {
    const theme = core.data.features.appearance.themes.find((t) => t.id === named.id);
    if (!theme) return error("not_found", "theme-not-found", named.id);
    if (named.userOnly && theme.source !== "user") {
      return error("invalid_request", "not-editable", named.id);
    }
  }
  return error("unsupported", "not-ready", named?.id ?? "theme.kdl");
}
