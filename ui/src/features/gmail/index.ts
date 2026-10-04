/**
 * Feature slot of M3b WP84 for Gmail accounts (contract ADR 0065; UI
 * ADR 0069): label management (create, rename / nest, recolour from
 * Gmail's palette, visibility, delete), applying labels to a message,
 * label-based views (a label's messages across folders) in the folder pane,
 * Gmail categories as a filterable property of the inbox.
 *
 * Mount point for the integrator: `MessageLabels` (label chips and the
 * category of one Gmail message, for message list rows and the reader).
 *
 * Only WP84 edits the files in this directory (plus
 * `locales/{en,de}/gmail.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Event as NailEvent } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { findSummary } from "../../state/mailOps";
import { queryKeys } from "../../state/queries";
import type { Feature, FeatureEventHandler } from "../types";
import { CategoryTabs } from "./CategoryTabs";
import { ApplyLabelsDialog, LabelDeleteDialog, LabelEditorDialog } from "./LabelDialogs";
import { LabelList } from "./Labels";
import { CATEGORIES } from "./model";
import {
  APPLY_LABELS_DIALOG,
  CATEGORY_TABS_WIDGET,
  LABEL_DELETE_DIALOG,
  LABEL_EDITOR_DIALOG,
  LABEL_LIST_WIDGET,
} from "./names";
import { gmailKeys, isGmailAccount, showCategory, showLabel } from "./queries";
import { gmailStoreFor } from "./store";

export { gmailKeys } from "./queries";
export { MessageLabels, LabelList, LabelSwatch } from "./Labels";
export { CategoryTabs } from "./CategoryTabs";
export { GMAIL_BASE_CSS } from "./parts";
export { APPLY_LABELS_DIALOG, LABEL_DELETE_DIALOG, LABEL_EDITOR_DIALOG } from "./names";
export { gmailStoreFor } from "./store";

/** The core's events that change Gmail labels or their counts. */
export const GMAIL_EVENTS = [
  "GmailLabelsChanged",
  "MailChanged",
  "AccountsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  [LABEL_LIST_WIDGET]: LabelList,
  [CATEGORY_TABS_WIDGET]: CategoryTabs,
};

const activeGmail = (ctx: ActionContext) =>
  isGmailAccount(ctx.queryClient, ctx.store.getState().activeAccountId);

const selectedGmail = (ctx: ActionContext) => {
  const id = ctx.store.getState().selectedMessageId;
  return id !== null && findSummary(ctx.queryClient, id)?.gmail !== undefined;
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** A new label in the selected Gmail account. */
  "gmail.new-label": {
    run: (ctx) => {
      const accountId = ctx.store.getState().activeAccountId;
      if (accountId === null) return;
      gmailStoreFor(ctx.store).getState().editLabel({ accountId, labelId: null, parent: null });
      ctx.store.getState().openWindow(LABEL_EDITOR_DIALOG);
    },
    enabled: activeGmail,
  },
  /** Labels of the selected message. */
  "gmail.apply-labels": {
    run: (ctx) => {
      ctx.store.getState().openWindow(APPLY_LABELS_DIALOG);
    },
    enabled: selectedGmail,
  },
  /** `gmail.label-view "Work"`: a label's messages across folders. */
  "gmail.label-view": {
    run: (ctx, args) => {
      const accountId = ctx.store.getState().activeAccountId;
      const name = args[0];
      if (accountId !== null && typeof name === "string" && name !== "") {
        showLabel(ctx.store, accountId, name);
      }
    },
    enabled: (ctx, args) => activeGmail(ctx) && typeof args[0] === "string",
  },
  /** `gmail.category "social"` (or no argument for the whole inbox). */
  "gmail.category": {
    run: (ctx, args) => {
      const accountId = ctx.store.getState().activeAccountId;
      const folders = ctx.queryClient.getQueryData<{ id: string; role: string }[]>(
        queryKeys.folders(accountId),
      );
      const inbox = folders?.find((f) => f.role === "inbox");
      if (accountId === null || !inbox) return;
      const category = CATEGORIES.find((c) => c === args[0]) ?? null;
      showCategory(ctx.store, accountId, inbox.id, category);
    },
    enabled: activeGmail,
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [LABEL_EDITOR_DIALOG]: LabelEditorDialog,
  [LABEL_DELETE_DIALOG]: LabelDeleteDialog,
  [APPLY_LABELS_DIALOG]: ApplyLabelsDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [LABEL_EDITOR_DIALOG]: {
    size: { width: 480, height: 460 },
    min_size: { width: 360, height: 320 },
  },
  [LABEL_DELETE_DIALOG]: { confirmation: true },
  [APPLY_LABELS_DIALOG]: {
    size: { width: 480, height: 520 },
    min_size: { width: 360, height: 360 },
  },
};

/** Labels (or messages, whose counts they show) changed: refetch. */
export const handleGmailEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "GmailLabelsChanged") {
    void queryClient.invalidateQueries({ queryKey: gmailKeys.labels(event.account_id) });
  } else if ((GMAIL_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: gmailKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleGmailEvent];
/** Background components. */
export const effects: Feature["effects"] = [];
