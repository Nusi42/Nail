/**
 * The mock core's answers to the customization requests (WP24), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.customization`
 * ([`createCustomizationMockState`]).
 *
 * Saved searches work like in the core (WP20); the other requests answer
 * `unsupported` until WP24.
 */
import type { Request, Response, SavedSearch } from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { parseQuery } from "../../api/mockSearch";

/** The requests this feature answers. */
export const CUSTOMIZATION_REQUESTS = [
  "UpdateLayout",
  "PreviewConfigMerge",
  "ResolveConfigMerge",
  "ApplyConfigMerge",
  "ListConfigBackups",
  "RevertConfig",
  "OpenConfigFile",
  "ListSavedSearches",
  "SaveSavedSearch",
  "DeleteSavedSearch",
] as const satisfies readonly Request["type"][];

export type CustomizationRequest = Extract<
  Request,
  { type: (typeof CUSTOMIZATION_REQUESTS)[number] }
>;

export function isCustomizationRequest(request: Request): request is CustomizationRequest {
  return (CUSTOMIZATION_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface CustomizationMockState {
  savedSearches: SavedSearch[];
}

export function createCustomizationMockState(): CustomizationMockState {
  return { savedSearches: [] };
}

function state(core: MockCore): CustomizationMockState {
  return core.data.features.customization;
}

function saveSearch(core: MockCore, search: SavedSearch): Response {
  const error = parseQuery(search.query).error;
  if (error) return errorResponse("invalid_request", `the query does not parse: ${error.message}`);
  if (search.name.trim() === "") return errorResponse("invalid_request", "a name is required");
  const list = state(core).savedSearches;
  let saved: SavedSearch;
  if (search.id === null) {
    const position = list.reduce((max, s) => Math.max(max, s.position + 1), 0);
    saved = { ...search, id: core.nextId("search-"), position };
    list.push(saved);
  } else {
    const index = list.findIndex((s) => s.id === search.id);
    if (index < 0) return errorResponse("not_found", `saved search ${search.id}`);
    saved = { ...search };
    list[index] = saved;
  }
  list.sort((a, b) => a.position - b.position);
  core.emit({ type: "SavedSearchesChanged" });
  return { type: "SavedSearchSaved", search: { ...saved } };
}

export function answerCustomization(core: MockCore, request: CustomizationRequest): Response {
  switch (request.type) {
    case "ListSavedSearches":
      return {
        type: "SavedSearches",
        searches: state(core).savedSearches.map((s) => ({ ...s })),
      };
    case "SaveSavedSearch":
      return saveSearch(core, request.search);
    case "DeleteSavedSearch": {
      const list = state(core).savedSearches;
      const index = list.findIndex((s) => s.id === request.search_id);
      if (index < 0) return errorResponse("not_found", `saved search ${request.search_id}`);
      list.splice(index, 1);
      core.emit({ type: "SavedSearchesChanged" });
      return { type: "Done" };
    }
    default:
      return errorResponse("unsupported", `${request.type} arrives with M1b WP24`);
  }
}
