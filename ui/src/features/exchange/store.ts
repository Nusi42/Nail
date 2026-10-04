/**
 * UI state of the Exchange feature (one store per UI store): what the
 * mailbox / category dialogs and the automatic-replies editor work on, the
 * message the Focused Inbox dialog moves, and whether the Focused Inbox
 * tabs are switched off for an account (a view choice, like Outlook's
 * "Show Focused Inbox"; it changes nothing on the server).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  AccountId,
  ExchangeCategory,
  ExchangeCategoryId,
  ExchangeMailbox,
  InferenceClassification,
  MessageId,
} from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

export interface MailboxTarget {
  accountId: AccountId;
  /** `null` = a new mailbox. */
  address: string | null;
}

export interface CategoryTarget {
  accountId: AccountId;
  /** `null` = a new category. */
  categoryId: ExchangeCategoryId | null;
}

export interface ClassifyTarget {
  messageIds: MessageId[];
  to: InferenceClassification;
}

export interface ExchangeUiState {
  mailboxEditor: MailboxTarget | null;
  removing: ExchangeMailbox | null;
  categoryEditor: CategoryTarget | null;
  deleting: ExchangeCategory | null;
  /** Account the automatic-replies editor opens with (`null` = the first). */
  repliesAccount: AccountId | null;
  classify: ClassifyTarget | null;
  /** Accounts whose Focused Inbox tabs are switched off. */
  focusedOff: AccountId[];
  editMailbox: (target: MailboxTarget | null) => void;
  removeMailbox: (mailbox: ExchangeMailbox | null) => void;
  editCategory: (target: CategoryTarget | null) => void;
  deleteCategory: (category: ExchangeCategory | null) => void;
  editReplies: (accountId: AccountId | null) => void;
  classifyMessages: (target: ClassifyTarget | null) => void;
  setFocused: (accountId: AccountId, on: boolean) => void;
}

const stores = new WeakMap<UiStore, StoreApi<ExchangeUiState>>();

export function exchangeStoreFor(ui: UiStore): StoreApi<ExchangeUiState> {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<ExchangeUiState>()((set) => ({
    mailboxEditor: null,
    removing: null,
    categoryEditor: null,
    deleting: null,
    repliesAccount: null,
    classify: null,
    focusedOff: [],
    editMailbox: (mailboxEditor) => {
      set({ mailboxEditor });
    },
    removeMailbox: (removing) => {
      set({ removing });
    },
    editCategory: (categoryEditor) => {
      set({ categoryEditor });
    },
    deleteCategory: (deleting) => {
      set({ deleting });
    },
    editReplies: (repliesAccount) => {
      set({ repliesAccount });
    },
    classifyMessages: (classify) => {
      set({ classify });
    },
    setFocused: (accountId, on) => {
      set((s) => ({
        focusedOff: on
          ? s.focusedOff.filter((id) => id !== accountId)
          : [...s.focusedOff.filter((id) => id !== accountId), accountId],
      }));
    },
  }));
  stores.set(ui, store);
  return store;
}

export function useExchangeUi<T>(selector: (state: ExchangeUiState) => T): T {
  return useStore(exchangeStoreFor(useUiStoreApi()), selector);
}
