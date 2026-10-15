/**
 * Feature slot of "Show source" (ADR 0194): a message's complete source as
 * text — every header (`Received`, `Authentication-Results`, …), the MIME
 * structure and the content — to read, select and copy. Nothing is loaded
 * from the network.
 *
 * - The action `source.message` (reader toolbar "More", the card's and the
 *   rows' menus, Ctrl+U) opens `message-source:<n>` for the selected
 *   message; a message's window is raised when it is already open.
 * - The window (`./SourceWindow.tsx`): Original (as received) / Decoded
 *   (each part's content without transfer encoding; parts that are not
 *   text named in one line; encrypted mail decrypted), Wrap lines, Copy
 *   all.
 * - The core answers `GetMessageSource`; the mock core writes a source from
 *   the parsed body (`./mock.ts`). The look is `./skin.ts`
 *   (`SOURCE_SKIN_CSS`, in the theme compiler's skin list).
 *
 * Texts: `locales/{en,de}/source.ftl`. The central registry
 * (`../registry.ts`) merges these exports into the app; see `../README.md`.
 */
import type { ActionInfo } from "../../actions/palette";
import { MAIL_AVAILABILITY } from "../../actions/palette";
import { LOG_WINDOW } from "../connlog";
import type { Feature } from "../types";
import { MESSAGE_SOURCE_KIND, SHOW_SOURCE_ACTION } from "./names";
import { SourceWindow } from "./SourceWindow";
import { openMessageSource } from "./store";

export * from "./names";
export { SOURCE_SKIN_CSS } from "./skin";
export { openMessageSource } from "./store";

/** Layout widget kinds → components (none). */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [SHOW_SOURCE_ACTION]: {
    run: (ctx) => {
      const id = ctx.store.getState().selectedMessageId;
      if (id !== null) openMessageSource(ctx.store, id);
    },
    enabled: (ctx) => ctx.store.getState().selectedMessageId !== null,
  },
};

/** Command-palette descriptions (`source.ftl`). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [SHOW_SOURCE_ACTION]: {
    title: "source-action-message",
    keywords: "source-action-keywords",
    availability: MAIL_AVAILABILITY,
    icon: "file-code",
  },
};

/** Dialogs by window kind. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [MESSAGE_SOURCE_KIND]: SourceWindow,
};

/**
 * The window model: a non-modal tool window per message, sized like the
 * connection log viewer (both show long monospace lines).
 */
export const windows: NonNullable<Feature["windows"]> = {
  [MESSAGE_SOURCE_KIND]: LOG_WINDOW,
};
