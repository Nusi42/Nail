/**
 * The event, contact and task editors load on demand, so the app's own
 * chunk stays small (the date / time picker brings React Aria). The
 * dialogs show "Loading…" for the moment it takes.
 */
import { Suspense, lazy, type ComponentType } from "react";
import type { DialogProps } from "../../layout/dialogRegistry";
import type * as Editors from "./editorDialogs";
import { LoadingDialog } from "../../widgets/DialogStates";

type EditorModule = typeof Editors;

let loading: Promise<EditorModule> | null = null;

/** Loads the editor dialogs (tests and an idle preload call it). */
export function loadPimEditors(): Promise<EditorModule> {
  loading ??= import("./editorDialogs");
  return loading;
}

function Loading() {
  return <LoadingDialog part="pim.editor" textPart="pim.editor.loading" />;
}

function lazyDialog(
  pick: (module: EditorModule) => ComponentType<DialogProps>,
): ComponentType<DialogProps> {
  const Lazy = lazy(async () => ({ default: pick(await loadPimEditors()) }));
  return function LazyDialog(props: DialogProps) {
    return (
      <Suspense fallback={<Loading />}>
        <Lazy {...props} />
      </Suspense>
    );
  };
}

export const EventEditorDialog = lazyDialog((m) => m.EventEditorDialog);
export const ContactEditorDialog = lazyDialog((m) => m.ContactEditorDialog);
export const TaskEditorDialog = lazyDialog((m) => m.TaskEditorDialog);
