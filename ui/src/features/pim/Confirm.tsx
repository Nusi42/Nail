/**
 * The PIM questions before something is lost (R6, ADR 0239): deleting an
 * event or a task, deleting a contact group, removing a calendar source.
 * Each asks in its own modal confirmation window (`PIM_CONFIRMS`, title
 * `window-<name>`) with the shared `ConfirmDialog`: a red verb ("Delete
 * event"), the focus on Cancel, Enter never running it; ×, Esc and Cancel
 * forget the question. The component that asks words the question
 * (`askPim`); `go` does it — a failure keeps the window open with the
 * core's reason.
 */
import { useEffect, useState, type ComponentType } from "react";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { UiStore } from "../../state/uiStore";
import { useUiStoreApi } from "../../state/uiStore";
import { ConfirmDialog } from "../../widgets/dialog";
import { problemOf } from "./common";
import {
  DELETE_EVENT_DIALOG,
  DELETE_GROUP_DIALOG,
  DELETE_TASK_DIALOG,
  REMOVE_SOURCE_DIALOG,
} from "./names";

/** The PIM confirmation windows. */
export const PIM_CONFIRMS = [
  DELETE_EVENT_DIALOG,
  DELETE_TASK_DIALOG,
  DELETE_GROUP_DIALOG,
  REMOVE_SOURCE_DIALOG,
] as const;

export type PimConfirm = (typeof PIM_CONFIRMS)[number];

/** What a confirmation window asks, localized by the component that asks. */
export interface PimQuestion {
  question: string;
  /** A note under the question. */
  detail?: string;
  /** The verb ("Delete event"). */
  action: string;
  /** Does it; a rejection keeps the window open with the reason. */
  go: () => Promise<void>;
}

const pending = new WeakMap<UiStore, Map<PimConfirm, PimQuestion>>();

function questionsOf(store: UiStore): Map<PimConfirm, PimQuestion> {
  let map = pending.get(store);
  if (!map) {
    map = new Map();
    pending.set(store, map);
  }
  return map;
}

/** Asks `question` in the confirmation window `kind`. */
export function askPim(store: UiStore, kind: PimConfirm, question: PimQuestion): void {
  questionsOf(store).set(kind, question);
  store.getState().openWindow(kind);
}

/** The question open in window `kind` (`null`: none). */
export function pimQuestion(store: UiStore, kind: PimConfirm): PimQuestion | null {
  return pending.get(store)?.get(kind) ?? null;
}

function PimConfirmDialog({ kind, close }: { kind: PimConfirm; close: () => void }) {
  const t = useT();
  const store = useUiStoreApi();
  const [asked] = useState(() => pimQuestion(store, kind));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // However the question ends, it is forgotten.
  useEffect(
    () => () => {
      const map = pending.get(store);
      if (asked !== null && map?.get(kind) === asked) map.delete(kind);
    },
    [store, kind, asked],
  );
  return (
    <ConfirmDialog
      kind={kind}
      risk="destructive"
      question={asked?.question ?? t("pim-confirm-gone")}
      {...(asked?.detail === undefined
        ? {}
        : { detail: <p data-part="dialog.note">{asked.detail}</p> })}
      action={asked?.action ?? t("pim-delete")}
      canConfirm={asked !== null}
      busy={busy}
      error={error}
      onConfirm={() => {
        if (asked === null) return;
        setBusy(true);
        setError(null);
        asked.go().then(
          () => {
            close();
          },
          (e: unknown) => {
            const problem = problemOf(e);
            setError(t(problem.id, problem.args));
            setBusy(false);
          },
        );
      }}
      cancel={{ onClick: close }}
    />
  );
}

/** The window components, by name (`dialogs` in `./index.ts`). */
export const pimConfirmDialogs = Object.fromEntries(
  PIM_CONFIRMS.map((kind): [PimConfirm, ComponentType<DialogProps>] => [
    kind,
    ({ close }: DialogProps) => <PimConfirmDialog kind={kind} close={close} />,
  ]),
) as Readonly<Record<PimConfirm, ComponentType<DialogProps>>>;
