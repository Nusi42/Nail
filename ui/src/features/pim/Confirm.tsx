/**
 * The PIM questions before something is lost (R6, ADR 0239): deleting an
 * event, a task, contacts or a contact group, removing a calendar source.
 * Each asks in its own modal confirmation window (`PIM_CONFIRMS`, title
 * `window-<name>`) with the shared question window
 * (`../../widgets/dialog/QuestionWindow.tsx`): a red verb ("Delete event"),
 * the focus on Cancel, Enter never running it; ×, Esc and Cancel forget the
 * question. The component that asks words the question (`askPim`); `run`
 * does it — a failure keeps the window open with the core's reason.
 */
import type { ComponentType } from "react";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { UiStore } from "../../state/uiStore";
import { askQuestion, questionOf, QuestionWindow, type DialogQuestion } from "../../widgets/dialog";
import {
  DELETE_CONTACTS_DIALOG,
  DELETE_EVENT_DIALOG,
  DELETE_GROUP_DIALOG,
  DELETE_TASK_DIALOG,
  REMOVE_SOURCE_DIALOG,
} from "./names";

/** The PIM confirmation windows. */
export const PIM_CONFIRMS = [
  DELETE_EVENT_DIALOG,
  DELETE_TASK_DIALOG,
  DELETE_CONTACTS_DIALOG,
  DELETE_GROUP_DIALOG,
  REMOVE_SOURCE_DIALOG,
] as const;

export type PimConfirm = (typeof PIM_CONFIRMS)[number];

/** Asks `question` in the confirmation window `kind`. */
export function askPim(store: UiStore, kind: PimConfirm, question: DialogQuestion): void {
  askQuestion(store, kind, question);
}

/** The question open in window `kind` (`null`: none). */
export function pimQuestion(store: UiStore, kind: PimConfirm): DialogQuestion | null {
  return questionOf(store, kind);
}

/** The window components, by name (`dialogs` in `./index.ts`). */
export const pimConfirmDialogs = Object.fromEntries(
  PIM_CONFIRMS.map((kind): [PimConfirm, ComponentType<DialogProps>] => [kind, QuestionWindow]),
) as Readonly<Record<PimConfirm, ComponentType<DialogProps>>>;
