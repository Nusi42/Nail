/**
 * `@nail/plugin-api` (contract ADRs 0050 / 0079, SDK ADRs 0053 / WP103).
 *
 * - Types of the plug-in contract, generated from the Rust contract
 *   (`./generated`, `npm run gen:api`; never edited by hand).
 * - The runtime API (`nail`, `definePlugin`) and its types (`./api`).
 * - Hook and extension helpers (`veto`, `banner`, `previewTree`, … and
 *   checks that mirror the host's).
 * - Bytes helpers for the sandbox (UTF-8, base64).
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
  toPreview,
  type DefinedPlugin,
  type PluginContext,
  type PluginSetup,
  type PluginSetupAttachments,
  type PluginSetupComposer,
  type PluginSetupHooks,
  type SdkAttachmentsRegistry,
  type SdkPreviewRenderer,
} from "./define";
export {
  addContent,
  arrivalProblems,
  badge,
  banner,
  beforeSendProblems,
  createdId,
  HOOK_TIMEOUT_MS,
  isContributionName,
  isDone,
  isHeaderValue,
  isPluginHeaderName,
  isPreview,
  JOB_WALL_CLOCK_MS,
  markdownBlockProblems,
  MAX_ADDED_CONTENT,
  MAX_ADDED_HEADERS,
  MAX_ARRIVAL_ACTIONS,
  MAX_BADGE_TEXT,
  MAX_BANNER_BUTTONS,
  MAX_BLOCK_HTML,
  MAX_HEADER_NAME,
  MAX_HEADER_VALUE,
  MAX_HOOK_TEXT,
  MAX_HOOK_TIMEOUT_MS,
  MAX_OPEN_ITEMS,
  MAX_PREVIEW_IMAGE,
  MAX_PREVIEW_INPUT,
  mergeBeforeSend,
  MIN_HOOK_TIMEOUT_MS,
  openProblems,
  PREVIEW_IMAGE_TYPES,
  previewImage,
  previewProblems,
  previewTree,
  RESERVED_HEADER_PREFIX,
  veto,
} from "./extensions";
export {
  decodeLatin1,
  decodeText,
  decodeUtf8,
  encodeUtf8,
  fromBase64,
  isUtf8,
  toBase64,
  toBytes,
} from "./bytes";
export {
  Fragment,
  isUiNode,
  NODE_TYPES,
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
