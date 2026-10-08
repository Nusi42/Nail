/**
 * Feature slot of M4c WP124 (contract ADR 0094): printing and saving as PDF
 * — print jobs (`CreatePrintJob` for a message, thread, event or agenda),
 * the print view (route `#/print/<job id>`, rendered with the theme's
 * `print.*` parts and its `print` colour mode, reports `PrintViewReady`),
 * the system print dialog and, on the desktop, `SavePrintPdf` (WebView2
 * `PrintToPdf` in the shell).
 *
 * Contributions: dialog `print`; actions `print.message`, `print.thread`,
 * `print.event`, `print.agenda`, `print.save-pdf` with palette
 * descriptions; the effect `PrintHost` (shows the print view and local
 * sheets); `printSheet` for other slots (the recovery kit); theme base CSS
 * `PRINT_BASE_CSS`.
 *
 * Only WP124 edits the files in this directory (plus
 * `locales/{en,de}/print.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { ReactNode } from "react";
import type { ActionInfo } from "../../actions/palette";
import type { ActionContext } from "../../actions/registry";
import type { PrintTarget } from "../../api/generated";
import type { UiStore } from "../../state/uiStore";
import { pimStoreFor } from "../pim/store";
import type { Feature } from "../types";
import { defaultAgenda, occurrenceOf } from "./model";
import {
  PRINT_AGENDA_ACTION,
  PRINT_DIALOG,
  PRINT_EVENT_ACTION,
  PRINT_MESSAGE_ACTION,
  PRINT_SAVE_PDF_ACTION,
  PRINT_THREAD_ACTION,
} from "./names";
import { PrintDialog } from "./PrintDialog";
import { PrintHost } from "./PrintView";
import { printStoreFor, type PrintMode } from "./store";

export * from "./names";
export { printViewRoute } from "./model";
export { PRINT_BASE_CSS } from "./parts";
export { printStoreFor } from "./store";
export { closePrintView, openPrintView, PrintHost, PrintView } from "./PrintView";

/** Query keys of this feature. */
export const printKeys = {
  all: ["print"] as const,
  job: (jobId: string) => ["print", "job", jobId] as const,
};

/** Prints content without a core job (e.g. the recovery kit). */
export function printSheet(ui: UiStore, sheet: ReactNode): void {
  printStoreFor(ui).getState().setSheet(sheet);
}

function messageTarget(ctx: ActionContext): PrintTarget | null {
  const id = ctx.store.getState().selectedMessageId;
  return id === null ? null : { type: "message", message_id: id };
}

function threadTarget(ctx: ActionContext): PrintTarget | null {
  const thread = ctx.store.getState().selectedThreadId;
  return thread === null ? messageTarget(ctx) : { type: "thread", thread_id: thread };
}

function eventTarget(ctx: ActionContext, args: readonly unknown[]): PrintTarget | null {
  if (typeof args[0] === "string" && args[0] !== "") {
    return {
      type: "event",
      event_id: args[0],
      occurrence: typeof args[1] === "string" ? args[1] : null,
    };
  }
  const selected = pimStoreFor(ctx.store).getState().selected;
  return selected
    ? { type: "event", event_id: selected.eventId, occurrence: occurrenceOf(selected.recurrenceId) }
    : null;
}

function open(ctx: ActionContext, target: PrintTarget | null, mode: PrintMode): void {
  printStoreFor(ctx.store).getState().prepare(target, mode);
  ctx.store.getState().openWindow(PRINT_DIALOG);
}

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [PRINT_MESSAGE_ACTION]: {
    run: (ctx) => {
      open(ctx, messageTarget(ctx), "print");
    },
    enabled: (ctx) => ctx.store.getState().selectedMessageId !== null,
  },
  [PRINT_THREAD_ACTION]: {
    run: (ctx) => {
      open(ctx, threadTarget(ctx), "print");
    },
    enabled: (ctx) =>
      ctx.store.getState().selectedThreadId !== null ||
      ctx.store.getState().selectedMessageId !== null,
  },
  [PRINT_EVENT_ACTION]: {
    run: (ctx, args) => {
      open(ctx, eventTarget(ctx, args), "print");
    },
    enabled: (ctx, args) => eventTarget(ctx, args) !== null,
  },
  [PRINT_AGENDA_ACTION]: {
    run: (ctx) => {
      open(ctx, defaultAgenda(), "print");
    },
  },
  [PRINT_SAVE_PDF_ACTION]: {
    run: (ctx) => {
      open(ctx, messageTarget(ctx), "pdf");
    },
    enabled: (ctx) => ctx.store.getState().selectedMessageId !== null,
  },
};

/** Command-palette descriptions (`print.ftl`). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [PRINT_MESSAGE_ACTION]: {
    title: "print-action-message",
    keywords: "print-action-keywords",
    availability: ["list", "reader"],
  },
  [PRINT_THREAD_ACTION]: {
    title: "print-action-thread",
    keywords: "print-action-keywords",
    availability: ["list", "reader"],
  },
  [PRINT_EVENT_ACTION]: {
    title: "print-action-event",
    keywords: "print-action-keywords",
    availability: ["calendar"],
  },
  [PRINT_AGENDA_ACTION]: {
    title: "print-action-agenda",
    keywords: "print-action-agenda-keywords",
  },
  [PRINT_SAVE_PDF_ACTION]: {
    title: "print-action-save-pdf",
    keywords: "print-action-save-pdf-keywords",
    availability: ["list", "reader"],
  },
};

/** Dialog names → components. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [PRINT_DIALOG]: PrintDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [PRINT_DIALOG]: { size: { width: 760, height: 680 }, min_size: { width: 520, height: 480 } },
};

/** Background components: the print view and local sheets. */
export const effects: NonNullable<Feature["effects"]> = [PrintHost];
