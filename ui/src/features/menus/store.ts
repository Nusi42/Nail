/**
 * The open context menu, one store per UI store (tests create many apps).
 *
 * While a menu is open its target is the "context" of the actions it runs:
 * `contextTarget(ui)` tells a handler which folder or message the menu was
 * opened on (a folder's menu must not act on the active folder).
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { ResolvedLayout, ResolvedMenu } from "../../api/generated";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";
import { contextMenuName, keyContextOf, resolveMenu } from "./resolve";
import { findContextTarget, textTarget, type ContextTarget } from "./targets";

export interface OpenMenu {
  menu: ResolvedMenu;
  target: ContextTarget;
  /** Viewport position of the menu's top-left corner. */
  x: number;
  y: number;
  /** Where focus goes back to when the menu closes by keyboard. */
  returnFocus: HTMLElement | null;
  /** Opened with the keyboard (focus the first item). */
  keyboard: boolean;
}

export interface MenuState {
  open: OpenMenu | null;
  show: (open: OpenMenu) => void;
  /** Closes the menu; `restoreFocus` puts focus back where it was. */
  close: (restoreFocus: boolean) => void;
}

export type MenuStore = StoreApi<MenuState>;

const stores = new WeakMap<UiStore, MenuStore>();

/** The menu store of a UI store (created on first use). */
export function menuStoreFor(ui: UiStore): MenuStore {
  const existing = stores.get(ui);
  if (existing) return existing;
  const store = createStore<MenuState>()((set, get) => ({
    open: null,
    show: (open) => {
      set({ open });
    },
    close: (restoreFocus) => {
      const open = get().open;
      if (!open) return;
      set({ open: null });
      if (restoreFocus && open.returnFocus?.isConnected) open.returnFocus.focus();
    },
  }));
  stores.set(ui, store);
  return store;
}

export function useMenuStoreApi(): MenuStore {
  return menuStoreFor(useUiStoreApi());
}

export function useMenu<T>(selector: (state: MenuState) => T): T {
  return useStore(useMenuStoreApi(), selector);
}

/** The target of the open context menu (for action handlers), else `null`. */
export function contextTarget(ui: UiStore): ContextTarget | null {
  return menuStoreFor(ui).getState().open?.target ?? null;
}

/** Makes the target the current selection where that is what actions use. */
function select(ui: UiStore, target: ContextTarget): void {
  const state = ui.getState();
  if (
    (target.part === "message-list.row" || target.part === "thread-view.message") &&
    target.messageId !== null
  ) {
    state.selectMessage(target.messageId);
    // A Design B child row stands for its message alone (P3).
    if (target.part === "message-list.row") {
      state.selectThread(target.child ? null : target.threadId);
    }
  }
}

const FOCUSABLE = '[tabindex]:not([tabindex="-1"]), button, a[href], input, select, textarea';

export interface OpenOptions {
  /** Pointer position; `null` = next to the element (keyboard). */
  point: { x: number; y: number } | null;
  /**
   * Opened with the keyboard although a position is given (a "More"
   * button activated with Enter / Space): the first item gets the focus.
   */
  keyboard?: boolean;
}

/**
 * Opens the context menu for `element` when the layout has one for its
 * part. Returns whether a menu opened (ADR 0184: else nothing opens).
 */
export function openContextMenu(
  ui: UiStore,
  layout: ResolvedLayout,
  element: Element | null,
  options: OpenOptions,
): boolean {
  const target = findContextTarget(element);
  return target !== null && showMenu(ui, layout, target, element, options);
}

/**
 * Opens the text menu on `root` (ADR 0184): a mail document's body, whose
 * events the page's handler does not see.
 */
export function openTextMenu(
  ui: UiStore,
  layout: ResolvedLayout,
  root: HTMLElement,
  options: OpenOptions,
): boolean {
  return showMenu(ui, layout, textTarget(root), root, options);
}

function showMenu(
  ui: UiStore,
  layout: ResolvedLayout,
  target: ContextTarget,
  element: Element | null,
  { point, keyboard }: OpenOptions,
): boolean {
  const menu = resolveMenu(layout, contextMenuName(target.part), keyContextOf(target.part));
  if (!menu) return false;
  select(ui, target);
  const rect = target.element.getBoundingClientRect();
  const active = element?.ownerDocument.activeElement;
  const focused =
    active instanceof HTMLElement && active !== element?.ownerDocument.body ? active : null;
  // By keyboard: back to what had focus; by pointer: the focusable around the target.
  const around = target.element.closest<HTMLElement>(FOCUSABLE);
  const returnFocus =
    (point === null || keyboard === true) && focused ? focused : (around ?? focused);
  menuStoreFor(ui)
    .getState()
    .show({
      menu,
      target,
      x: Math.round(point?.x ?? rect.left),
      y: Math.round(point?.y ?? rect.bottom),
      returnFocus,
      keyboard: keyboard ?? point === null,
    });
  return true;
}
