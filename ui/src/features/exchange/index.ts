/**
 * Feature slot of M3c WP94 for Exchange accounts (contract ADR 0072; UI
 * ADR 0076): shared mailboxes and delegate folders in the folder tree,
 * the room finder in the event editor, the automatic replies (out of
 * office) editor, Exchange rule placement badges, the Focused Inbox tabs,
 * categories with their colours, To Do lists in the task widget, and the
 * Exchange parts of account setup / settings.
 *
 * Mount points for the integrator (components exported below):
 * `MailboxFolderBadge` (folder tree rows), `MessageCategories` and
 * `FocusMoveButton` (list rows / reader), `RoomFinder` (event editor),
 * `ExchangeRuleBadges` (rule list), `TaskListChooser` / `TaskListBadge`
 * (task widget), `ExchangeAccountType` / `ExchangeServerReview` /
 * `ExchangeSignInFields` (account setup), `ExchangeOptionsFields` /
 * `ExchangeStatusPanel` / `AutomaticRepliesEditor` (account settings),
 * `EXCHANGE_BASE_CSS` (theme base CSS).
 *
 * Only WP94 edits the files in this directory (plus
 * `locales/{en,de}/exchange.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 */
import type { Event as NailEvent, InferenceClassification } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { findSummary } from "../../state/mailOps";
import { rememberTagTargets } from "../mail-actions/tags";
import { queryKeys } from "../../state/queries";
import type { Feature, FeatureEventHandler } from "../types";
import { AutomaticRepliesBanner, AutomaticRepliesDialog } from "./AutomaticReplies";
import {
  ApplyCategoriesDialog,
  CategoryDeleteDialog,
  CategoryEditorDialog,
  CategoryList,
} from "./Categories";
import { ClassifyDialog, FocusedTabs, otherFocus } from "./Focused";
import { MailboxEditorDialog, MailboxList, MailboxRemoveDialog } from "./Mailboxes";
import {
  APPLY_CATEGORIES_DIALOG,
  AUTOMATIC_REPLIES_BANNER_WIDGET,
  AUTOMATIC_REPLIES_DIALOG,
  CATEGORIES_WIDGET,
  CATEGORY_DELETE_DIALOG,
  CATEGORY_EDITOR_DIALOG,
  CLASSIFY_DIALOG,
  FOCUSED_TABS_WIDGET,
  MAILBOX_EDITOR_DIALOG,
  MAILBOX_REMOVE_DIALOG,
  MAILBOXES_WIDGET,
  RULES_WIDGET,
} from "./names";
import { exchangeKeys, isExchangeAccount, showFocus } from "./queries";
import { ExchangeRulesWidget } from "./Rules";
import { exchangeStoreFor } from "./store";

export { exchangeKeys } from "./queries";
export { EXCHANGE_BASE_CSS } from "./parts";
export { exchangeStoreFor } from "./store";
export * from "./names";
export { MailboxFolderBadge, MailboxList } from "./Mailboxes";
export { MessageCategories, CategoryList } from "./Categories";
export { FocusMoveButton, FocusedTabs } from "./Focused";
export { RoomFinder, type RoomFinderProps } from "./RoomFinder";
export { ExchangeRuleBadges, ExchangeRulesWidget } from "./Rules";
export { TaskListBadge, TaskListChooser, type TaskListChooserProps } from "./TaskLists";
export {
  ExchangeAccountType,
  ExchangeOptionsFields,
  ExchangeServerReview,
  ExchangeSignInFields,
  ExchangeStatusPanel,
  needsRedirectConfirmation,
  type ExchangeSetupChoice,
} from "./Setup";
export { AutomaticRepliesBanner, AutomaticRepliesEditor } from "./AutomaticReplies";
export {
  exchangeCandidate,
  exchangeCredentialsReady,
  exchangeSecretsFor,
  exchangeSignInOf,
  groupTaskLists,
} from "./model";

/** The core's events that change Exchange state or what it counts. */
export const EXCHANGE_EVENTS = [
  "ExchangeChanged",
  "MailChanged",
  "AccountsChanged",
] as const satisfies readonly NailEvent["type"][];

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  [FOCUSED_TABS_WIDGET]: FocusedTabs,
  [AUTOMATIC_REPLIES_BANNER_WIDGET]: AutomaticRepliesBanner,
  [MAILBOXES_WIDGET]: MailboxList,
  [CATEGORIES_WIDGET]: CategoryList,
  [RULES_WIDGET]: ExchangeRulesWidget,
};

const activeExchange = (ctx: ActionContext) =>
  isExchangeAccount(ctx.queryClient, ctx.store.getState().activeAccountId);

const anyExchange = (ctx: ActionContext) =>
  ctx.queryClient
    .getQueryData<{ kind: string }[]>(queryKeys.accounts)
    ?.some((a) => a.kind === "exchange") ?? false;

const focusArg = (arg: unknown): InferenceClassification | null =>
  arg === "focused" || arg === "other" ? arg : null;

function selectedInference(ctx: ActionContext): InferenceClassification | null {
  const id = ctx.store.getState().selectedMessageId;
  if (id === null) return null;
  return findSummary(ctx.queryClient, id)?.exchange?.inference ?? null;
}

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  /** Open another mailbox in the selected Exchange account. */
  "exchange.add-mailbox": {
    run: (ctx) => {
      const accountId = ctx.store.getState().activeAccountId;
      if (accountId === null) return;
      exchangeStoreFor(ctx.store).getState().editMailbox({ accountId, address: null });
      ctx.store.getState().openWindow(MAILBOX_EDITOR_DIALOG);
    },
    enabled: activeExchange,
  },
  /** A new category in the selected Exchange account. */
  "exchange.new-category": {
    run: (ctx) => {
      const accountId = ctx.store.getState().activeAccountId;
      if (accountId === null) return;
      exchangeStoreFor(ctx.store).getState().editCategory({ accountId, categoryId: null });
      ctx.store.getState().openWindow(CATEGORY_EDITOR_DIALOG);
    },
    enabled: activeExchange,
  },
  /** The automatic replies editor (`exchange.automatic-replies "<account>"` picks one). */
  "exchange.automatic-replies": {
    run: (ctx, args) => {
      const chosen = typeof args[0] === "string" ? args[0] : null;
      const active = ctx.store.getState().activeAccountId;
      exchangeStoreFor(ctx.store)
        .getState()
        .editReplies(chosen ?? (isExchangeAccount(ctx.queryClient, active) ? active : null));
      ctx.store.getState().openWindow(AUTOMATIC_REPLIES_DIALOG);
    },
    enabled: anyExchange,
  },
  /** `exchange.focus "focused"` / `"other"` (or no argument for the whole inbox). */
  "exchange.focus": {
    run: (ctx, args) => {
      const accountId = ctx.store.getState().activeAccountId;
      const folders = ctx.queryClient.getQueryData<{ id: string; role: string }[]>(
        queryKeys.folders(accountId),
      );
      const inbox = folders?.find((f) => f.role === "inbox");
      if (accountId === null || !inbox) return;
      showFocus(ctx.store, accountId, inbox.id, focusArg(args[0]));
    },
    enabled: activeExchange,
  },
  /** Move the selected message to `"focused"` / `"other"` (default: the other tab). */
  "exchange.classify": {
    run: (ctx, args) => {
      const id = ctx.store.getState().selectedMessageId;
      const current = selectedInference(ctx);
      if (id === null || current === null) return;
      const to = focusArg(args[0]) ?? otherFocus(current);
      exchangeStoreFor(ctx.store)
        .getState()
        .classifyMessages({ messageIds: [id], to });
      ctx.store.getState().openWindow(CLASSIFY_DIALOG);
    },
    enabled: (ctx) => selectedInference(ctx) !== null,
  },
  /** The categories of the selected Exchange message (tags written as categories). */
  "exchange.apply-categories": {
    run: (ctx) => {
      rememberTagTargets(ctx.store);
      ctx.store.getState().openWindow(APPLY_CATEGORIES_DIALOG);
    },
    enabled: (ctx) => {
      const id = ctx.store.getState().selectedMessageId;
      return id !== null && findSummary(ctx.queryClient, id)?.exchange != null;
    },
  },
  /** Switch the Focused Inbox tabs of the selected account on / off. */
  "exchange.toggle-focused": {
    run: (ctx) => {
      const accountId = ctx.store.getState().activeAccountId;
      if (accountId === null) return;
      const ui = exchangeStoreFor(ctx.store).getState();
      ui.setFocused(accountId, ui.focusedOff.includes(accountId));
    },
    enabled: activeExchange,
  },
};

/** Dialog names → components. */
export const dialogs: Feature["dialogs"] = {
  [MAILBOX_EDITOR_DIALOG]: MailboxEditorDialog,
  [MAILBOX_REMOVE_DIALOG]: MailboxRemoveDialog,
  [CATEGORY_EDITOR_DIALOG]: CategoryEditorDialog,
  [CATEGORY_DELETE_DIALOG]: CategoryDeleteDialog,
  [AUTOMATIC_REPLIES_DIALOG]: AutomaticRepliesDialog,
  [CLASSIFY_DIALOG]: ClassifyDialog,
  [APPLY_CATEGORIES_DIALOG]: ApplyCategoriesDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [MAILBOX_EDITOR_DIALOG]: {
    size: { width: 560, height: 520 },
    min_size: { width: 420, height: 360 },
  },
  [MAILBOX_REMOVE_DIALOG]: { confirmation: true },
  [CATEGORY_EDITOR_DIALOG]: {
    size: { width: 480, height: 420 },
    min_size: { width: 360, height: 300 },
  },
  [CATEGORY_DELETE_DIALOG]: { confirmation: true },
  [AUTOMATIC_REPLIES_DIALOG]: {
    size: { width: 720, height: 640 },
    min_size: { width: 520, height: 480 },
  },
  [CLASSIFY_DIALOG]: { size: { width: 520, height: 480 }, min_size: { width: 380, height: 320 } },
  [APPLY_CATEGORIES_DIALOG]: {
    size: { width: 480, height: 520 },
    min_size: { width: 360, height: 360 },
  },
};

/** Exchange state changed: refetch the named areas of the account (or everything on mail changes). */
export const handleExchangeEvent: FeatureEventHandler = (event, { queryClient }) => {
  if (event.type === "ExchangeChanged") {
    for (const area of event.areas) {
      void queryClient.invalidateQueries({ queryKey: exchangeKeys.area(event.account_id, area) });
    }
    // Other mailboxes change the folder tree.
    if (event.areas.includes("mailboxes")) {
      void queryClient.invalidateQueries({ queryKey: queryKeys.folders(event.account_id) });
    }
  } else if ((EXCHANGE_EVENTS as readonly string[]).includes(event.type)) {
    void queryClient.invalidateQueries({ queryKey: exchangeKeys.all });
  }
};

/** Handlers for the core's events. */
export const events: Feature["events"] = [handleExchangeEvent];
/** Background components. */
export const effects: Feature["effects"] = [];

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  [RULES_WIDGET]: {
    labels: [
      "exchange-settings-tab-rules",
      "exchange-rules-label",
      "exchange-rules-placements",
      "exchange-rules-server-title",
      "exchange-rules-server-switch",
    ],
    keywords: "settings-keywords-exchange-rules",
  },
};
