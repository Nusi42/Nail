/**
 * The undo toast and the last undoable batch, one store per UI store (tests
 * create many apps).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  LocalizedMessage,
  MailActionKind,
  MailBatchId,
  MessageId,
  ThreadId,
} from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** What a tag change did, for the toast. */
export interface TagToast {
  change: "added" | "removed" | "changed";
  noun: "label" | "category" | "tag";
  /** Tags added and removed. */
  count: number;
}

/** What the toast says. */
export type MailToast =
  | {
      kind: "done";
      /** Counter, so the same text again restarts the timer. */
      seq: number;
      action: MailActionKind;
      conversations: number;
      messages: number;
      /** `null` = nothing to undo (Delete permanently). */
      batchId: MailBatchId | null;
      /** A tag change: what it did, to how many tags, and what they are called. */
      tags?: TagToast | undefined;
    }
  | { kind: "undone"; seq: number }
  | { kind: "error"; seq: number; message: LocalizedMessage };

/** The selection before an action, restored by its undo. */
export interface SelectionBefore {
  messageId: MessageId | null;
  threadId: ThreadId | null;
}

/** The last batch `mail.undo` undoes. */
export interface LastBatch {
  batchId: MailBatchId;
  /** Until when the core keeps it (ms since the epoch). */
  until: number;
  selection: SelectionBefore | null;
}

export interface MailActionsState {
  toast: MailToast | null;
  last: LastBatch | null;
  seq: number;
  show: (toast: DistributiveOmit<MailToast, "seq">) => void;
  dismiss: () => void;
  setLast: (last: LastBatch | null) => void;
}

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;

export type MailActionsStore = StoreApi<MailActionsState>;

const stores = new WeakMap<UiStore, MailActionsStore>();

/** The mail-actions store of a UI store (created on first use). */
export function mailActionsStoreFor(ui: UiStore): MailActionsStore {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<MailActionsState>()((set, get) => ({
    toast: null,
    last: null,
    seq: 0,
    show: (toast) => {
      const seq = get().seq + 1;
      set({ seq, toast: { ...toast, seq } });
    },
    dismiss: () => {
      set({ toast: null });
    },
    setLast: (last) => {
      set({ last });
    },
  }));
  stores.set(ui, store);
  return store;
}

export function useMailActions<T>(selector: (state: MailActionsState) => T): T {
  return useStore(mailActionsStoreFor(useUiStoreApi()), selector);
}
