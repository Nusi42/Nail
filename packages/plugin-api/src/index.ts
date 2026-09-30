/**
 * `@nail/plugin-api` (M2d preview; contract ADR 0050, SDK docs/adr/0053-plugin-sdk.md).
 *
 * - Types of the plug-in contract, generated from the Rust contract
 *   (`./generated`, `npm run gen:api`; never edited by hand).
 * - The runtime API (`nail`, `definePlugin`) and its types (`./api`).
 * - A JSX runtime producing component trees (`@nail/plugin-api/jsx-runtime`).
 * - A test host for plug-in unit tests (`@nail/plugin-api/testing`).
 * - Typings of the sandbox globals (`@nail/plugin-api/globals`).
 * - Node-side tools: manifest check, `.nailplugin` packing (`@nail/plugin-api/tools`).
 */
export type * from "./generated";
export type * from "./api";
export {
  createPluginApiError,
  isPluginApiError,
  nail,
  toDialogSpec,
  toNotificationSpec,
  toPanelProvider,
  type DialogSpec,
  type PanelProvider,
  type PanelRender,
  type SdkNail,
  type SdkUiApi,
} from "./nail";
export {
  definePlugin,
  PLUGIN_MARKER,
  type DefinedPlugin,
  type PluginContext,
  type PluginSetup,
} from "./define";
export {
  Fragment,
  isUiNode,
  toNodes,
  type PluginUiNodeOf,
  type PluginUiNodeType,
  type UiChild,
  type UiComponent,
  type UiContent,
  type UiIntrinsicElements,
} from "./jsx-runtime";
export {
  checkTree,
  IMAGE_TYPES,
  MAX_UI_DEPTH,
  MAX_UI_ITEMS,
  MAX_UI_NODES,
  type TreeIssue,
} from "./tree";

/** The plug-in API version this SDK is written for (the manifest's `apiVersion`). */
export const API_VERSION = "1";
