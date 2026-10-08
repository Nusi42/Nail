/**
 * The text menu's actions (ADR 0184): Cut, Copy, Paste and Select all on
 * the text the menu opened on — a field, an editor, selectable text
 * (`../../theme/selection.ts`) or a mail document — in place of the
 * browser's menu. They act on the selection saved when the menu opened
 * (the menu took the focus) and apply only while a text menu is open.
 */
import type { ResolvedMenuEntry } from "../../api/generated";
import type { ActionContext, ActionHandler } from "../../actions/registry";
import { contextTarget } from "./store";
import { isTextInput, selectsText, TEXT_PART, type ContextTarget } from "./targets";

/**
 * The keys the text menu shows: the platform's own editing keys. They are
 * not key bindings (fields and editors handle them themselves).
 */
export const EDIT_SHORTCUTS: Readonly<Record<string, string>> = {
  "edit.cut": "ctrl+x",
  "edit.copy": "ctrl+c",
  "edit.paste": "ctrl+v",
  "edit.select-all": "ctrl+a",
};

/** The text menu's entries with the editing keys where the layout binds none. */
export function withEditShortcuts(items: readonly ResolvedMenuEntry[]): ResolvedMenuEntry[] {
  return items.map((entry) =>
    entry.type === "action" &&
    entry.shortcut === null &&
    Object.hasOwn(EDIT_SHORTCUTS, entry.action)
      ? { ...entry, shortcut: EDIT_SHORTCUTS[entry.action] ?? null }
      : entry,
  );
}

/** The open text menu's target, else `null`. */
function textTargetOf(ctx: ActionContext): ContextTarget | null {
  const target = contextTarget(ctx.store);
  return target?.part === TEXT_PART ? target : null;
}

/** A field or editor that takes input now. */
function writable(element: HTMLElement): boolean {
  if (!isTextInput(element)) return false;
  if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
    return !element.readOnly && !element.disabled;
  }
  return element.isContentEditable;
}

/** Puts the saved selection back (and the focus into a field or editor). */
function restore(target: ContextTarget): void {
  const { element, selection } = target;
  if (isTextInput(element)) element.focus({ preventScroll: true });
  const field = selection?.field ?? null;
  if (
    field !== null &&
    (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement)
  ) {
    element.setSelectionRange(field[0], field[1]);
    return;
  }
  const range = selection?.range ?? null;
  const current = element.ownerDocument.getSelection();
  if (range !== null && current !== null) {
    current.removeAllRanges();
    current.addRange(range);
  }
}

/**
 * Runs an editing command on the target's document: the one way that edits
 * fields and editors with their undo history and input events.
 */
function edit(target: ContextTarget, command: "copy" | "cut" | "insertText", text?: string): void {
  // eslint-disable-next-line @typescript-eslint/no-deprecated -- execCommand is the only API that edits a field or editor with its undo history and input events (and copies the selection with the user's gesture).
  target.element.ownerDocument.execCommand(command, false, text);
}

const hasText = (target: ContextTarget) =>
  target.selection === undefined || target.selection === null || selectsText(target.selection);

/** The clipboard can be read (Paste; not on an insecure origin). */
const canRead = () => "clipboard" in navigator;

export const editActions: Readonly<Record<string, ActionHandler>> = {
  "edit.cut": {
    run: (ctx) => {
      const target = textTargetOf(ctx);
      if (target === null) return;
      restore(target);
      edit(target, "cut");
    },
    enabled: (ctx) => {
      const target = textTargetOf(ctx);
      return target !== null && writable(target.element) && hasText(target);
    },
  },
  "edit.copy": {
    run: (ctx) => {
      const target = textTargetOf(ctx);
      if (target === null) return;
      restore(target);
      edit(target, "copy");
    },
    enabled: (ctx) => {
      const target = textTargetOf(ctx);
      return target !== null && hasText(target);
    },
  },
  "edit.paste": {
    run: async (ctx) => {
      const target = textTargetOf(ctx);
      if (target === null) return;
      const text = await navigator.clipboard.readText();
      restore(target);
      edit(target, "insertText", text);
    },
    enabled: (ctx) => {
      const target = textTargetOf(ctx);
      return target !== null && writable(target.element) && canRead();
    },
  },
  "edit.select-all": {
    run: (ctx) => {
      const target = textTargetOf(ctx);
      if (target === null) return;
      const { element } = target;
      if (element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement) {
        element.focus({ preventScroll: true });
        element.select();
        return;
      }
      if (isTextInput(element)) element.focus({ preventScroll: true });
      const selection = element.ownerDocument.getSelection();
      if (selection === null) return;
      const range = element.ownerDocument.createRange();
      range.selectNodeContents(element);
      selection.removeAllRanges();
      selection.addRange(range);
    },
    enabled: (ctx) => textTargetOf(ctx) !== null,
  },
};
