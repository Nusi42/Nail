/**
 * Feature slot of live search (R1 WR2, contract ADR 0219): the search
 * field searches while the user types, after `LIVE_SEARCH_PAUSE_MS`
 * (`../../state/debounce.ts`, `useDebouncedAction`; Enter searches at
 * once), and a query the core cannot parse shows a compact indicator in the
 * field with its explanation in an overlay that never moves the content
 * below. The field itself stays `../../widgets/SearchBar.tsx` (its state in
 * `../../state/uiStore.ts` and `../../state/queries.ts`); this slot holds
 * what the package adds around it.
 *
 * Texts: `locales/{en,de}/search.ftl` (prefixes in `./messages.ts`; the
 * field's older texts are `search-bar-*` / `search-error-*` in `app.ftl`).
 * The central registry (`../registry.ts`) merges these exports into the
 * app; see `../README.md`.
 */
import type { Feature } from "../types";

export const widgets: Feature["widgets"] = {};
export const actions: Feature["actions"] = {};
