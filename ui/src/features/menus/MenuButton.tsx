/**
 * A button that opens a menu of its own (the toolbar's "More", the rail's
 * account, R8): one place for the open state, the toggle (a click while the
 * menu is open closes it), the key that opens it with the first item
 * focused, Escape on the button closing it, the focus going back to the
 * button, and the `ContextMenu` running the chosen action.
 */
import { useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { useActions } from "../../actions/ActionsProvider";
import type { ResolvedMenuEntry } from "../../api/generated";
import { ContextMenu } from "./ContextMenu";

/** Where the menu opens (viewport pixels; `alignRight`: `x` is its right edge). */
export interface MenuPlace {
  x: number;
  y: number;
  alignRight?: boolean;
}

interface OpenMenu extends MenuPlace {
  /** Opened with the keyboard: the first item takes the focus. */
  keyboard: boolean;
}

/**
 * The menu button's state: spread `buttonProps` on the `<button>`, render
 * `menu(items, label)` after it. `place` says where the menu opens for the
 * button; `openKey` opens it from the keyboard (besides Enter and Space).
 */
export function useMenuButton(place: (button: HTMLButtonElement) => MenuPlace, openKey: string) {
  const { dispatch } = useActions();
  const ref = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState<OpenMenu | null>(null);
  const show = (keyboard: boolean) => {
    if (ref.current) setOpen({ ...place(ref.current), keyboard });
  };
  const close = (restoreFocus: boolean) => {
    setOpen(null);
    if (restoreFocus) ref.current?.focus();
  };
  const buttonProps = {
    ref,
    "data-state": open ? "expanded" : undefined,
    "aria-haspopup": "menu" as const,
    "aria-expanded": open !== null,
    // A click on the button while the menu is open closes it (the menu
    // leaves presses on its button to the button: `owner`).
    onClick: (event: MouseEvent<HTMLButtonElement>) => {
      if (open) close(false);
      else show(event.detail === 0);
    },
    onKeyDown: (event: KeyboardEvent<HTMLButtonElement>) => {
      if (event.key === openKey && !open) {
        event.preventDefault();
        event.stopPropagation();
        show(true);
      } else if (event.key === "Escape" && open) {
        // Opened with the pointer, the focus stayed on the button: Escape closes it there too.
        event.preventDefault();
        event.stopPropagation();
        close(true);
      }
    },
  };
  const menu = (items: readonly ResolvedMenuEntry[], label: string): ReactNode =>
    open && (
      <ContextMenu
        items={items}
        label={label}
        x={open.x}
        y={open.y}
        alignRight={open.alignRight === true}
        focusOnOpen={open.keyboard}
        owner={ref}
        onRun={(action, args) => {
          close(true);
          void dispatch(action, args);
        }}
        onClose={close}
      />
    );
  return { open: open !== null, buttonProps, menu };
}
