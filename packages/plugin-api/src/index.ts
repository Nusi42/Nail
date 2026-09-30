/**
 * `@nail/plugin-api` (M2d; contract ADR 0050). The types in `./generated`
 * are generated from the Rust contract (`nail_api::export_plugin_typescript`,
 * `npm run gen:api`); do not edit them by hand.
 *
 * Slot created by WP60; WP63 adds the API surface (`definePlugin`, `nail.*`)
 * and the JSX runtime producing `PluginUiNode` trees.
 */
export type * from "./generated";
