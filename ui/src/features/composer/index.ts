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
 *
 * M5c (WP172, ADR 0116): several composers, one window per draft
 * (`composer:<key>`, `./sessions.ts`), non-modal; the confirmations
 * `composer-close` / `composer-discard` are the only modal composer
 * windows. `windowTitles.composer` shows the subject in the title bar;
 * `skin.ts` reflows the composer to its window's width.
 */
import { useEffect } from "react";
import type { JsonValue } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { useUiStoreApi, type UiState } from "../../state/uiStore";
import type { Feature } from "../types";
import { CloseConfirmDialog, DiscardConfirmDialog } from "./ConfirmDialogs";
import { switchMode } from "./flows";
import { composerSubjectTitle, MODES } from "./Composer";
import {
  CLOSE_CONFIRM_KIND,
  composerOf,
  composerSessions,
  composerTarget,
  DISCARD_CONFIRM_KIND,
} from "./sessions";
import { COMPOSER_KIND, richStoreFor } from "./store";
import { useComposerPopOutPreparer } from "./popOut";
import { useComposeContext } from "../../state/hooks";

export {
  cancelRichSend,
  openRichDraft,
  saveRich,
  sendRich,
  startMailtoCompose,
  startRichCompose,
  startRichReply,
  discardRich,
  openComposer,
  requestClose,
  requestDiscard,
} from "./flows";
export {
  activeComposer,
  composerOf,
  composerTarget,
  findComposerByDraft,
  isUnsaved,
} from "./sessions";
export { ComposerWindowTitle, composerSubjectTitle, useComposerWindowOf } from "./Composer";
export { COMPOSER_SKIN_CSS } from "./skin";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** The composer the actions work on: the focused one (else the plain one). */
const target = (ctx: ActionContext) => composerTarget(ctx.store.getState());
const hasComposer = (ctx: ActionContext) => composerOf(ctx.store.getState(), target(ctx)) !== null;
const richOf = (ctx: ActionContext) => richStoreFor(ctx.store, target(ctx));

function modeArg(args: readonly JsonValue[]) {
  const mode = args[0];
  return MODES.find((m) => m === mode) ?? null;
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  "composer.set-mode": {
    run: (ctx, args) => {
      const mode = modeArg(args);
      if (mode) return switchMode(ctx, mode, target(ctx));
    },
    enabled: (ctx, args) => hasComposer(ctx) && modeArg(args) !== null,
  },
  "composer.toggle-preview": {
    run: (ctx) => {
      const rich = richOf(ctx);
      rich.setState({ preview: !rich.getState().preview });
    },
    enabled: (ctx) => hasComposer(ctx) && richOf(ctx).getState().mode === "markdown",
  },
  "composer.toggle-source": {
    run: (ctx) => {
      const rich = richOf(ctx);
      rich.setState({ htmlSource: !rich.getState().htmlSource });
    },
    enabled: (ctx) => hasComposer(ctx) && richOf(ctx).getState().mode === "html",
  },
  "composer.attach": {
    run: (ctx) => {
      const rich = richOf(ctx);
      rich.setState({ attachRequest: rich.getState().attachRequest + 1 });
    },
    enabled: hasComposer,
  },
};

/**
 * Named dialogs: the composer's confirmations, opened per composer as
 * `composer-close:<key>` / `composer-discard:<key>`.
 */
export const dialogs: Feature["dialogs"] = {
  [CLOSE_CONFIRM_KIND]: CloseConfirmDialog,
  [DISCARD_CONFIRM_KIND]: DiscardConfirmDialog,
};

/**
 * The window model (ADR 0116): composers are non-modal, resizable windows,
 * several at once (one per draft); the default layout's `window
 * "composer"` sets the same sizes. Only the confirmations are modal.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [COMPOSER_KIND]: {
    modal: false,
    size: { width: 820, height: 720 },
    min_size: { width: 520, height: 400 },
    instances: "multiple",
  },
  [CLOSE_CONFIRM_KIND]: {
    confirmation: true,
    size: { width: 480, height: 220 },
    instances: "multiple",
  },
  [DISCARD_CONFIRM_KIND]: {
    confirmation: true,
    size: { width: 440, height: 200 },
    instances: "multiple",
  },
};

/**
 * Window titles by kind (for the window layer, WP174): the title of a
 * window from the UI state, `null` = the kind's `window-<kind>` text. A
 * composer window shows its message's subject (`ComposerWindowTitle` is the
 * same as a component).
 */
export const windowTitles: Readonly<
  Record<string, (state: Pick<UiState, "composer" | "composers">, id: string) => string | null>
> = {
  [COMPOSER_KIND]: composerSubjectTitle,
};

/** Handlers for core events. */
export const events: Feature["events"] = [];

/**
 * Installs the composer sessions (focused-composer mirror, close guard) as
 * soon as the app starts, so composers restored or opened by any path are
 * covered.
 */
function ComposerSessions() {
  const store = useUiStoreApi();
  useEffect(() => void composerSessions(store), [store]);
  // Saves a composer's draft before it pops out (desktop, M5c).
  useComposerPopOutPreparer(useComposeContext());
  return null;
}

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [ComposerSessions];
