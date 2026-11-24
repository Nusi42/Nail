/**
 * PIM menus: a menu button (WAI-ARIA menu button pattern) for the calendar
 * list's per-calendar "…" menus and the "+" menus of its groups (WP164),
 * and the right-click menus of events, contacts and tasks (R8, ADR 0251:
 * `usePimContextMenu`). Both look like the app's context menus (the shared
 * `context-menu.*` parts) and share one popup (`PimMenuPopup`) with one
 * keyboard model, but run local commands instead of layout actions.
 *
 * Keyboard: Enter / Space / Down on the button open the menu on its first
 * item (Up: on the last); a right-click, the menu key or Shift+F10 on an
 * item open its menu on the first item; in the menu Up / Down / Home / End
 * move, Enter / Space choose, Escape closes and returns to where the menu
 * was opened from, Tab closes. A click elsewhere closes it. The popup is
 * positioned below the button or at the pointer through a geometry rule
 * (`useGeometry`), never inline styles.
 */
import {
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { comboFromEvent } from "../../actions/keys";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { anchorOf, placePopover, type AnchorRect } from "../../widgets/Popover";
import { opensContextMenu } from "../../widgets/toolbarModel";
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

type Item = Extract<PimMenuEntry, { type: "item" }>;

/** Which item has the focus when a menu opens. */
type OpenAt = "first" | "last";

interface PopupProps {
  /** The DOM id of the menu (`aria-controls` of the button). */
  id: string;
  /** Accessible name of the menu. */
  label: string;
  entries: readonly PimMenuEntry[];
  /** Where it opens: below this rectangle (a point is a rectangle without size). */
  anchor: AnchorRect;
  openAt: OpenAt;
  /** Closes the menu; `refocus` = give the focus back to where it was opened from. */
  onClose: (refocus: boolean) => void;
  /** A press on this element does not count as "elsewhere" (the menu button toggles itself). */
  owner?: RefObject<HTMLElement | null>;
}

/** The open menu: the shared look and keyboard model of every PIM menu. */
function PimMenuPopup({ id, label, entries, anchor, openAt, onClose, owner }: PopupProps) {
  const geom = `pim-menu-${id.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  const popupRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  useGeometry(geom, { left: `${String(position.x)}px`, top: `${String(position.y)}px` });

  const items = entries
    .map((entry, index) => ({ entry, index }))
    .filter((e): e is { entry: Item; index: number } => e.entry.type === "item");

  // Place the popup under its anchor, inside the window.
  useLayoutEffect(() => {
    const popup = popupRef.current?.getBoundingClientRect();
    const { x, y } = placePopover(
      anchor,
      { width: popup?.width ?? 0, height: popup?.height ?? 0 },
      { width: window.innerWidth, height: window.innerHeight },
      "below",
      anchor.width === 0 && anchor.height === 0 ? 0 : undefined,
    );
    setPosition((p) => (p.x === x && p.y === y ? p : { x, y }));
  }, [anchor]);

  // Focus the first / last item when the menu opens (with the items of then).
  const focusItem = useEffectEvent((which: OpenAt) => {
    const target = which === "first" ? items[0] : items[items.length - 1];
    if (target) itemRefs.current[target.index]?.focus();
  });
  useEffect(() => {
    focusItem(openAt);
  }, [openAt]);

  // A press outside closes the menu.
  const closeOutside = useEffectEvent((target: Node) => {
    if (popupRef.current?.contains(target) || owner?.current?.contains(target)) return;
    onClose(false);
  });
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node) closeOutside(event.target);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, []);

  const onItemKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const at = items.findIndex((e) => e.index === index);
    const focus = (i: number) => {
      const target = items[(i + items.length) % items.length];
      if (target) itemRefs.current[target.index]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focus(at + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        focus(at - 1);
        break;
      case "Home":
        event.preventDefault();
        focus(0);
        break;
      case "End":
        event.preventDefault();
        focus(items.length - 1);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        onClose(true);
        break;
      case "Tab":
        onClose(false);
        break;
    }
  };

  return (
    <div
      ref={popupRef}
      data-part="context-menu"
      data-geom={geom}
      data-key-context="menu"
      data-key-modal=""
    >
      <div id={id} data-part="context-menu.list" role="menu" aria-label={label}>
        {entries.map((entry, index) => {
          if (entry.type === "separator") {
            return <div key={entry.id} data-part="context-menu.separator" role="separator" />;
          }
          if (entry.type === "header") {
            return (
              <div key={entry.id} data-part="context-menu.header" role="presentation">
                {entry.label}
              </div>
            );
          }
          const radio = entry.checked !== undefined;
          return (
            <button
              key={entry.id}
              ref={(el) => {
                itemRefs.current[index] = el;
              }}
              type="button"
              data-part="context-menu.item"
              data-item={entry.id}
              data-tone={entry.danger ? "danger" : undefined}
              data-state={entry.checked ? "checked" : undefined}
              role={radio ? "menuitemradio" : "menuitem"}
              aria-checked={radio ? entry.checked : undefined}
              tabIndex={-1}
              onClick={() => {
                onClose(true);
                entry.onSelect();
              }}
              onPointerEnter={(event) => {
                event.currentTarget.focus();
              }}
              onKeyDown={(event) => {
                onItemKeyDown(event, index);
              }}
            >
              {entry.color !== undefined ? (
                <Swatch
                  geom={`${geom}-${entry.id}`}
                  color={entry.color}
                  part="context-menu.item.swatch"
                />
              ) : (
                <Icon name={entry.icon} part="context-menu.item.icon" />
              )}
              <span data-part="context-menu.item.label">{entry.label}</span>
              {entry.checked && <Icon name="check" part="context-menu.item.check" />}
            </button>
          );
        })}
      </div>
    </div>
  );
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
  const id = `${useId()}-menu`;
  const buttonRef = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState<{ at: OpenAt; anchor: AnchorRect } | null>(null);

  const show = (at: OpenAt) => {
    const button = buttonRef.current;
    if (button) setOpen({ at, anchor: anchorOf(button) });
  };
  const close = (refocus: boolean) => {
    setOpen(null);
    if (refocus) buttonRef.current?.focus();
  };

  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      show("first");
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      show("last");
    }
  };

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        data-part={part}
        data-field={field}
        data-state={open !== null ? "open" : undefined}
        aria-label={showLabel ? undefined : label}
        title={showLabel ? undefined : label}
        aria-haspopup="menu"
        aria-expanded={open !== null}
        aria-controls={open !== null ? id : undefined}
        onClick={() => {
          if (open !== null) close(false);
          else show("first");
        }}
        onKeyDown={onButtonKeyDown}
      >
        <Icon name={icon} part={`${part}.icon`} />
        {showLabel && <span data-part={`${part}.label`}>{label}</span>}
      </button>
      {open !== null && (
        <PimMenuPopup
          id={id}
          label={label}
          entries={entries}
          anchor={open.anchor}
          openAt={open.at}
          onClose={close}
          owner={buttonRef}
        />
      )}
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
 * keyboard — and gives the focus back to the item when it closes.
 */
export function usePimContextMenu(
  menuFor: (target: Element) => PimItemMenu | null,
): PimContextMenu {
  const id = `${useId()}-menu`;
  const layout = useLayoutConfig();
  const [open, setOpen] = useState<(PimItemMenu & { anchor: AnchorRect }) | null>(null);

  const show = (target: EventTarget, point: { x: number; y: number } | null) => {
    const found = target instanceof Element ? menuFor(target) : null;
    if (!found?.entries.some((e) => e.type === "item")) return false;
    const anchor = point
      ? { left: point.x, top: point.y, width: 0, height: 0 }
      : anchorOf(found.item);
    setOpen({ ...found, anchor });
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
  // On the page itself: an item's containers may contain fixed-position boxes (container queries).
  const menu =
    open === null
      ? null
      : createPortal(
          <PimMenuPopup
            id={id}
            label={open.label}
            entries={open.entries}
            anchor={open.anchor}
            openAt="first"
            onClose={(refocus) => {
              const back = open.item;
              setOpen(null);
              if (refocus && back.isConnected) back.focus();
            }}
          />,
          document.body,
        );
  return { onContextMenu, onKeyDown, menu };
}
