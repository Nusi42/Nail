/**
 * PIM menus: a menu button for the calendar list's per-calendar "…" menus
 * and the "+" menus of its groups (WP164; also the rules' add menus), and
 * the right-click menus of events, contacts and tasks (R8, ADR 0251:
 * `usePimContextMenu`). They are the app's one menu popup
 * (`features/menus` `ContextMenu`, local items) opened by the one menu
 * button (`useMenuButton`): the same look, keyboard model and placement as
 * every other menu; they run local commands instead of layout actions.
 *
 * Keyboard: Enter / Space / Down on the button open the menu on its first
 * item (Up: on the last); a right-click, the menu key or Shift+F10 on an
 * item open its menu on the first item; Escape closes and returns to where
 * the menu was opened from.
 *
 * A right-click menu is rendered on the app's root (`data-part="app"`):
 * outside the item's containers (container queries make them containing
 * blocks for fixed boxes) but inside the element that carries the theme's
 * font and colours.
 */
import { useState, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { comboFromEvent } from "../../actions/keys";
import { useActions } from "../../actions/ActionsProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { Icon } from "../../widgets/Icon";
import { opensContextMenu } from "../../widgets/toolbarModel";
import { ContextMenu, type MenuEntry } from "../menus/ContextMenu";
import { useMenuButton } from "../menus/MenuButton";
import { literalMenuLabel } from "../menus/sources";
import { Swatch } from "./common";

/** One entry of a menu. Texts come in localized. */
export type PimMenuEntry =
  | {
      type: "item";
      id: string;
      label: string;
      icon?: string;
      /** A colour square instead of an icon (colour choices). */
      color?: string;
      /** A choice (`menuitemradio`): whether it is the current one. */
      checked?: boolean;
      danger?: boolean;
      onSelect: () => void;
    }
  | { type: "header"; id: string; label: string }
  | { type: "separator"; id: string };

/** The app menu's entries for PIM entries (`geom`: prefix of the colour swatches' geometry ids). */
function menuEntries(entries: readonly PimMenuEntry[], geom: string): MenuEntry[] {
  return entries.map((entry): MenuEntry => {
    if (entry.type === "separator") return { type: "separator" };
    if (entry.type === "header") return { type: "header", label: literalMenuLabel(entry.label) };
    const { color, ...item } = entry;
    if (color === undefined) return item;
    const id = `${geom}-${entry.id}`.replace(/[^a-zA-Z0-9_-]/g, "_");
    return { ...item, start: <Swatch geom={id} color={color} part="context-menu.item.swatch" /> };
  });
}

export interface PimMenuProps {
  /** Accessible name of the button (and the menu). */
  label: string;
  /** Lucide icon of the button. */
  icon: string;
  entries: readonly PimMenuEntry[];
  /** `data-field` of the button (tests, theme rules). */
  field?: string;
  /** Part of the button (default `pim.menu-button`). */
  part?: string;
  /** Show `label` beside the icon (`<part>.label`) instead of only as the name and hover text. */
  showLabel?: boolean;
}

export function PimMenu({
  label,
  icon,
  entries,
  field,
  part = "pim.menu-button",
  showLabel = false,
}: PimMenuProps) {
  // Below the button, its left edge at the button's.
  const { buttonProps, menu } = useMenuButton(
    (button) => {
      const rect = button.getBoundingClientRect();
      return { x: rect.left, y: rect.bottom };
    },
    "ArrowDown",
    "ArrowUp",
  );
  return (
    <>
      <button
        {...buttonProps}
        type="button"
        data-part={part}
        data-field={field}
        aria-label={showLabel ? undefined : label}
        title={showLabel ? undefined : label}
      >
        <Icon name={icon} part={`${part}.icon`} />
        {showLabel && <span data-part={`${part}.label`}>{label}</span>}
      </button>
      {menu(menuEntries(entries, `pim-menu-${field ?? part}`), label)}
    </>
  );
}

/** The menu of the item a right-click or a key reached. */
export interface PimItemMenu {
  /** Accessible name of the menu. */
  label: string;
  entries: readonly PimMenuEntry[];
  /** The item: the menu opens under it from the keyboard and gives the focus back to it. */
  item: HTMLElement;
}

/** What a list or an item with right-click menus wires up (`usePimContextMenu`). */
export interface PimContextMenu {
  /** For `onContextMenu`: a right-click (or the menu key) opens the menu. */
  onContextMenu: (event: MouseEvent<HTMLElement>) => void;
  /** For `onKeyDown`: Shift+F10 (the layout's `menu.context` keys) opens it; `true` = handled. */
  onKeyDown: (event: KeyboardEvent<HTMLElement>) => boolean;
  /** The open menu (render it beside the item or list), or `null`. */
  menu: ReactNode;
}

/**
 * Right-click menus for PIM items (R8, ADR 0251): events, contacts and
 * tasks. `menuFor` answers the menu of the item under an element (the one
 * the pointer or the focus is on), built when it opens so it shows the
 * item as it is then; `null` = none, and the app's own menu handling goes
 * on. The menu takes the place of the app's layout menu for the item (the
 * app's one `contextmenu` handler leaves an event a part already handled
 * alone, ADR 0184), opens at the pointer — or under the item from the
 * keyboard — on its first item, and gives the focus back to the item when
 * it closes.
 */
export function usePimContextMenu(
  menuFor: (target: Element) => PimItemMenu | null,
): PimContextMenu {
  const layout = useLayoutConfig();
  const { dispatch } = useActions();
  const [open, setOpen] = useState<(PimItemMenu & { x: number; y: number }) | null>(null);

  const show = (target: EventTarget, point: { x: number; y: number } | null) => {
    const found = target instanceof Element ? menuFor(target) : null;
    if (!found?.entries.some((e) => e.type === "item")) return false;
    const rect = found.item.getBoundingClientRect();
    setOpen({ ...found, ...(point ?? { x: rect.left, y: rect.bottom }) });
    return true;
  };

  const onContextMenu = (event: MouseEvent<HTMLElement>) => {
    if (event.defaultPrevented) return;
    // A keyboard-invoked event (the menu key) has no pointer position.
    const keyboard = event.clientX === 0 && event.clientY === 0 && event.button !== 2;
    if (show(event.target, keyboard ? null : { x: event.clientX, y: event.clientY })) {
      event.preventDefault();
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    if (!opensContextMenu(layout, comboFromEvent(event))) return false;
    if (!show(event.target, null)) return false;
    event.preventDefault();
    event.stopPropagation();
    return true;
  };
  const menu =
    open === null
      ? null
      : createPortal(
          <ContextMenu
            items={menuEntries(open.entries, "pimItemMenu")}
            label={open.label}
            x={open.x}
            y={open.y}
            focusOnOpen
            onRun={(action, args) => void dispatch(action, args)}
            onClose={(refocus) => {
              const back = open.item;
              setOpen(null);
              if (refocus && back.isConnected) back.focus();
            }}
          />,
          open.item.closest('[data-part="app"]') ?? document.body,
        );
  return { onContextMenu, onKeyDown, menu };
}
