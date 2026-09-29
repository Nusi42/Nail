/**
 * Feature slot of M1b WP21 (composer): the rich composer: HTML (TipTap) and
 * Markdown (CodeMirror) editing, the core-rendered preview, attachments and
 * inline images, address autocomplete, the attachment reminder and draft
 * autosave.
 *
 * The composer widget itself keeps its M1a layout kind `composer`
 * (`widgets/Composer.tsx` re-exports `./Composer`). Only WP21 edits the
 * files in this directory. The central registry (`../registry.ts`) merges
 * these exports into the app; see `../README.md`.
 */
import type { JsonValue } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import type { Feature } from "../types";
import { switchMode } from "./flows";
import { MODES } from "./Composer";
import { richStoreFor } from "./store";

export {
  cancelRichSend,
  openRichDraft,
  saveRich,
  sendRich,
  startMailtoCompose,
  startRichCompose,
  startRichReply,
  discardRich,
} from "./flows";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

const hasComposer = (ctx: ActionContext) => ctx.store.getState().composer !== null;

function modeArg(args: readonly JsonValue[]) {
  const mode = args[0];
  return MODES.find((m) => m === mode) ?? null;
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  "composer.set-mode": {
    run: (ctx, args) => {
      const mode = modeArg(args);
      if (mode) return switchMode(ctx, mode);
    },
    enabled: (ctx, args) => hasComposer(ctx) && modeArg(args) !== null,
  },
  "composer.toggle-preview": {
    run: (ctx) => {
      const rich = richStoreFor(ctx.store);
      rich.setState({ preview: !rich.getState().preview });
    },
    enabled: (ctx) => hasComposer(ctx) && richStoreFor(ctx.store).getState().mode === "markdown",
  },
  "composer.toggle-source": {
    run: (ctx) => {
      const rich = richStoreFor(ctx.store);
      rich.setState({ htmlSource: !rich.getState().htmlSource });
    },
    enabled: (ctx) => hasComposer(ctx) && richStoreFor(ctx.store).getState().mode === "html",
  },
  "composer.attach": {
    run: (ctx) => {
      const rich = richStoreFor(ctx.store);
      rich.setState({ attachRequest: rich.getState().attachRequest + 1 });
    },
    enabled: hasComposer,
  },
};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {};

/** Handlers for core events. */
export const events: Feature["events"] = [];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [];
