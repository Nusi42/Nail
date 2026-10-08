/**
 * The contract constants the tools check against, generated from nail-api
 * (`../src/generated/constants.json`, `npm run gen:api`): the single source
 * of the manifest's, packages' and schedules' limits (wave Q2, ADR 0171).
 */
import constants from "../src/generated/constants.json" with { type: "json" };

/** The plug-in contract's constants (`nail_api::ts_constants::plugin_constants`). */
export const CONTRACT = constants;
