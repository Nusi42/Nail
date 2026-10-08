/**
 * What a context menu is about: the part under the pointer (or the focused
 * element for Shift+F10 / the menu key), mapped from the DOM's `data-part`
 * names to the contract's context-menu parts (`CONTEXT_MENU_PARTS`,
 * ADR 0043), with the message / folder / attachment it refers to.
 */
import { selectableRoot } from "../../theme/selection";
import { CONTEXT_MENU_PARTS } from "./resolve";

/** DOM part → context-menu part (`context:<part>` in `layout.kdl`). */
export const DOM_PARTS: Readonly<Record<string, string>> = {
  "message-list.row": "message-list.row",
  // P2 Design B: a conversation's message under its expanded row.
  "message-list.child": "message-list.row",
  "thread-view.item": "thread-view.message",
  // P2 WP203: a card of the reader's feed (its open body is a message-view).
  "thread-view.card": "thread-view.message",
  "message-view.attachment": "message-view.attachment",
  "message-view": "message-view",
  "folder-tree.item": "folder-tree.folder",
  "folder-tree.account": "folder-tree.account",
  composer: "composer",
  "nav-rail": "nav-rail",
  // ADR 0184: the folder pane around its folders and accounts.
  "folder-tree": "folder-tree",
};

/** The context-menu part of text: fields, editors, selectable text (ADR 0184). */
export const TEXT_PART = "text";

/**
 * The selection when a text menu opened (the menu takes the focus; its
 * actions put the selection back first).
 */
export interface SavedSelection {
  /** The document's selection range (selectable text, editors). */
  range: Range | null;
  /** The selected characters of a text field (`[start, end]`). */
  field: readonly [number, number] | null;
}

/** The thing a context menu acts on. */
export interface ContextTarget {
  /** Context-menu part, e.g. `message-list.row`. */
  part: string;
  /** The element the menu belongs to (focus returns near it). */
  element: HTMLElement;
  messageId: string | null;
  threadId: string | null;
  folderId: string | null;
  accountId: string | null;
  /** Attachment index (`message-view.attachment`). */
  attachment: number | null;
  /** A Design B child row (P3: acts on its message, never the conversation). */
  child: boolean;
  /**
   * A card of the reader's feed, or something inside one (P3, ADR 0164: a
   * card's own actions act on its message).
   */
  card: boolean;
  /** The selection of a text target (`part` = {@link TEXT_PART}). */
  selection?: SavedSelection | null;
}

/** Whether an element takes text input (it gets the text menu). */
export function isTextInput(element: Element | null): boolean {
  if (!(element instanceof HTMLElement)) return false;
  if (element.isContentEditable) return true;
  if (element.tagName === "TEXTAREA" || element.tagName === "SELECT") return true;
  if (element.tagName !== "INPUT") return false;
  const type = (element as HTMLInputElement).type;
  return !["button", "checkbox", "radio", "submit", "reset", "file", "range", "color"].includes(
    type,
  );
}

function attr(element: Element, name: string): string | null {
  return element.getAttribute(name);
}

/** The element an `aria-activedescendant` points to (listboxes). */
function activeDescendant(element: Element): Element | null {
  const id = attr(element, "aria-activedescendant");
  return id ? element.ownerDocument.getElementById(id) : null;
}

function targetOf(element: HTMLElement, part: string): ContextTarget {
  const message =
    attr(element, "data-message") ??
    element.closest("[data-message]")?.getAttribute("data-message") ??
    null;
  const index = attr(element, "data-index");
  return {
    part,
    element,
    messageId: message,
    threadId: attr(element, "data-thread"),
    folderId: attr(element, "data-folder"),
    accountId: attr(element, "data-account"),
    attachment: part === "message-view.attachment" && index !== null ? Number(index) : null,
    child: element.getAttribute("data-part") === "message-list.child",
    card: element.closest('[data-part="thread-view.card"]') !== null,
  };
}

/** The text field or editor `element` is in (`null` for a select or a checkbox). */
function editableAround(element: Element): HTMLElement | null {
  const root = selectableRoot(element);
  return root instanceof HTMLElement && isTextInput(root) ? root : null;
}

/** The selection now, within `root`. */
export function saveSelection(root: Element): SavedSelection {
  if (root instanceof HTMLInputElement || root instanceof HTMLTextAreaElement) {
    const { selectionStart: start, selectionEnd: end } = root;
    return { range: null, field: start === null || end === null ? null : [start, end] };
  }
  const selection = root.ownerDocument.getSelection();
  const range =
    selection !== null && selection.rangeCount > 0 && !selection.isCollapsed
      ? selection.getRangeAt(0)
      : null;
  return { range: range?.intersectsNode(root) ? range.cloneRange() : null, field: null };
}

/** Whether a saved selection holds any text. */
export function selectsText({ range, field }: SavedSelection): boolean {
  return field !== null ? field[0] !== field[1] : range !== null && !range.collapsed;
}

/** A text target on `root` with the selection of now. */
export function textTarget(root: HTMLElement): ContextTarget {
  return { ...targetOf(root, TEXT_PART), selection: saveSelection(root) };
}

/**
 * The context-menu target around `element`, or `null` (then nothing opens:
 * never the browser's menu, ADR 0184):
 *
 * - a text field or editor: the text menu;
 * - selected text under the pointer: the text menu (Copy);
 * - else the innermost part with a menu (a row, a folder, the reader);
 * - else selectable text: the text menu (Select all).
 */
export function findContextTarget(element: Element | null): ContextTarget | null {
  if (element === null) return null;
  const editable = editableAround(element);
  if (editable !== null) return textTarget(editable);
  const selectable = selectableRoot(element);
  const text = selectable instanceof HTMLElement ? textTarget(selectable) : null;
  if (text?.selection && selectsText(text.selection)) return text;
  const descendant = activeDescendant(element);
  let current: Element | null = descendant ?? element;
  while (current) {
    const domPart = current.getAttribute("data-part");
    const part = domPart === null ? undefined : DOM_PARTS[domPart];
    if (part !== undefined && Object.hasOwn(CONTEXT_MENU_PARTS, part)) {
      return targetOf(current as HTMLElement, part);
    }
    current = current.parentElement;
  }
  return text;
}
