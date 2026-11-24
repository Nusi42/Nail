/**
 * A context menu (WAI-ARIA menu pattern): items are `menuitem`s with a
 * roving focus — Up / Down / Home / End move, Enter / Space run, Right
 * opens a submenu and Left closes it, Escape closes (focus goes back to
 * where it was), Tab closes, a letter jumps to the next item starting with
 * it. Disabled actions stay focusable but do nothing (`aria-disabled`).
 * A submenu also opens while the pointer rests on its item.
 *
 * Dynamic submenus (R2, ADR 0222): an action item whose action has a menu
 * source (`./sources.ts`) shows as a submenu whose entries the source loads
 * when the list appears; a source without entries shows its item disabled
 * (or, with `runsWhenEmpty`, as a plain item running its action).
 *
 * The menu is positioned through the theme's stylesheet (`useGeometry`),
 * never with inline styles.
 */
import {
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
  type RefObject,
} from "react";
import type { JsonValue, ResolvedMenuEntry } from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { ariaShortcut, keyLabel } from "./keyLabels";
import { menuLabelText, type MenuSourceLoader } from "./sources";

export interface ContextMenuProps {
  items: readonly ResolvedMenuEntry[];
  /** Accessible name (the target's kind). */
  label: string;
  x: number;
  y: number;
  /** Focus the first item at once (opened with the keyboard). */
  focusOnOpen: boolean;
  /** Runs an action, then the host closes the menu. */
  onRun: (action: string, args: readonly JsonValue[]) => void;
  /** Closes the menu; `restoreFocus` for keyboard closing. */
  onClose: (restoreFocus: boolean) => void;
  /** The dynamic submenus (none: every action item is a plain item). */
  sources?: MenuSourceLoader | undefined;
  /**
   * `x` is where the menu's right edge goes (a button's right edge: the
   * menu opens under the button and grows to the left, R8), not its left.
   */
  alignRight?: boolean;
  /**
   * The button the menu belongs to (a menu button, R8): a press on it is
   * the button's own (it closes the menu), not a press outside.
   */
  owner?: RefObject<HTMLElement | null>;
}

/**
 * The menu's top-left corner: at `x` (or ending at `x` with `alignRight`)
 * and `y`, moved back inside the viewport.
 */
export function menuPosition(
  anchor: { x: number; y: number },
  size: { width: number; height: number },
  viewport: { width: number; height: number },
  alignRight = false,
): { x: number; y: number } {
  const left = alignRight ? anchor.x - size.width : anchor.x;
  return {
    x: Math.max(0, Math.min(left, viewport.width - size.width)),
    y: Math.max(0, Math.min(anchor.y, viewport.height - size.height)),
  };
}

type Navigable = Exclude<ResolvedMenuEntry, { type: "separator" } | { type: "header" }>;

function entryLabel(entry: Navigable, label: (l: string) => string): string {
  if (entry.type === "submenu") return menuLabelText(entry.label, label);
  return menuLabelText(entry.label ?? entry.action, label);
}

/** A dynamic submenu's entries, or `"loading"`. */
type Loaded = readonly ResolvedMenuEntry[] | "loading";

/**
 * The entries of the list's items that have a menu source (by index),
 * loaded when the list appears. Items without a source are not in the map.
 */
function useSourceEntries(
  items: readonly ResolvedMenuEntry[],
  sources: MenuSourceLoader | undefined,
): ReadonlyMap<number, Loaded> {
  const [loaded, setLoaded] = useState<ReadonlyMap<number, Loaded>>(() => {
    const out = new Map<number, Loaded>();
    items.forEach((entry, index) => {
      if (entry.type === "action" && sources?.has(entry.action) === true) {
        out.set(index, "loading");
      }
    });
    return out;
  });
  // Only when the list appears (a list never gets other items).
  const start = useEffectEvent((alive: () => boolean) => {
    for (const index of loaded.keys()) {
      const entry = items[index];
      if (entry?.type !== "action" || !sources) continue;
      const settle = (entries: readonly ResolvedMenuEntry[]) => {
        if (alive()) setLoaded((previous) => new Map(previous).set(index, entries));
      };
      sources.load(entry.action).then(settle, (error: unknown) => {
        console.error(`nail: the menu source of "${entry.action}" failed`, error);
        settle([]);
      });
    }
  });
  useEffect(() => {
    let alive = true;
    start(() => alive);
    return () => {
      alive = false;
    };
  }, []);
  return loaded;
}

/** Where a submenu sits beside its item: flipped to the other side, moved up. */
export interface SubmenuPlace {
  flip: boolean;
  /** Pixels to move up (0 or negative). */
  shift: number;
}

/**
 * Keeps a submenu inside the window: beside its item on the inline end,
 * on the other side when that end overflows, moved up when it would
 * overflow the bottom. `rect` is the popup's box at its default place.
 */
export function submenuPlace(
  rect: Pick<DOMRect, "left" | "right" | "bottom">,
  viewport: { width: number; height: number },
  rtl: boolean,
): SubmenuPlace {
  const flip = rtl ? rect.left < 0 : rect.right > viewport.width;
  return { flip, shift: Math.min(0, Math.round(viewport.height - rect.bottom)) };
}

/** A submenu's popup, placed through the theme's stylesheet (`useGeometry`). */
function SubmenuPopup({ id, children }: { id: string; children: ReactNode }) {
  const geom = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<SubmenuPlace>({ flip: false, shift: 0 });
  useGeometry(geom, {
    top: `${String(place.shift)}px`,
    ...(place.flip ? { "inset-inline-start": "auto", "inset-inline-end": "100%" } : {}),
  });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    if (rect.width === 0) return;
    const rtl = getComputedStyle(el).direction === "rtl";
    const next = submenuPlace(rect, { width: window.innerWidth, height: window.innerHeight }, rtl);
    if (next.flip || next.shift !== 0) setPlace(next);
  }, []);
  return (
    <div ref={ref} id={id} data-part="context-menu.popup" data-geom={geom}>
      {children}
    </div>
  );
}

/** What a submenu item opens: a layout submenu's items, or a source's entries. */
type SubmenuOf =
  { kind: "static"; items: readonly ResolvedMenuEntry[] } | { kind: "source"; loaded: Loaded };

interface ListProps {
  items: readonly ResolvedMenuEntry[];
  label: string;
  level: number;
  focusOnOpen: boolean;
  onRun: (action: string, args: readonly JsonValue[]) => void;
  onClose: (restoreFocus: boolean) => void;
  /** Closes this submenu and focuses its parent item (level > 0). */
  onBack?: () => void;
  sources: MenuSourceLoader | undefined;
}

function MenuList({
  items,
  label: name,
  level,
  focusOnOpen,
  onRun,
  onClose,
  onBack,
  sources,
}: ListProps) {
  const t = useT();
  const label = useLabel();
  const { isEnabled } = useActions();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const [openSub, setOpenSub] = useState<number | null>(null);
  const [subFocus, setSubFocus] = useState(false);
  const ids = useId();
  const sourced = useSourceEntries(items, sources);

  /** The submenu an item opens, or `null` for a plain item. */
  const submenuOf = (entry: Navigable, index: number): SubmenuOf | null => {
    if (entry.type === "submenu") return { kind: "static", items: entry.items };
    const loaded = sourced.get(index);
    if (loaded === undefined) return null;
    // A source without entries that runs its own action: a plain item.
    if (
      loaded !== "loading" &&
      loaded.length === 0 &&
      sources?.runsWhenEmpty?.(entry.action) === true
    ) {
      return null;
    }
    return { kind: "source", loaded };
  };
  /** A source that loaded no entries: its item is disabled. */
  const emptySource = (index: number) => {
    const loaded = sourced.get(index);
    return loaded !== undefined && loaded !== "loading" && loaded.length === 0;
  };

  const navigable = items
    .map((entry, index) => ({ entry, index }))
    .filter(
      (e): e is { entry: Navigable; index: number } =>
        e.entry.type !== "separator" && e.entry.type !== "header",
    );

  // Only when the list appears (with the items and settings of then).
  const focusFirst = useEffectEvent(() => {
    if (!focusOnOpen) return;
    const first = navigable[0];
    if (first) refs.current[first.index]?.focus();
  });
  useEffect(() => {
    focusFirst();
  }, []);

  const focusAt = (position: number) => {
    const count = navigable.length;
    if (count === 0) return;
    const target = navigable[((position % count) + count) % count];
    if (target) refs.current[target.index]?.focus();
  };

  const positionOf = (index: number) => navigable.findIndex((n) => n.index === index);

  const activate = (entry: Navigable, index: number, byKeyboard: boolean) => {
    if (submenuOf(entry, index) !== null) {
      if (emptySource(index)) return;
      setOpenSub(index);
      setSubFocus(byKeyboard);
      return;
    }
    if (entry.type === "submenu" || !isEnabled(entry.action, entry.args)) return;
    onRun(entry.action, entry.args);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>, entry: Navigable, index: number) => {
    const at = positionOf(index);
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        focusAt(at + 1);
        break;
      case "ArrowUp":
        focusAt(at - 1);
        break;
      case "Home":
        focusAt(0);
        break;
      case "End":
        focusAt(navigable.length - 1);
        break;
      case "ArrowRight":
        if (submenuOf(entry, index) !== null) activate(entry, index, true);
        break;
      case "ArrowLeft":
        if (onBack) onBack();
        break;
      case "Enter":
      case " ":
        activate(entry, index, true);
        break;
      case "Escape":
        if (onBack) onBack();
        else onClose(true);
        break;
      case "Tab":
        onClose(true);
        break;
      default: {
        const letter = event.key.length === 1 ? event.key.toLocaleLowerCase() : null;
        if (letter === null || event.ctrlKey || event.altKey || event.metaKey) {
          handled = false;
          break;
        }
        for (let step = 1; step <= navigable.length; step++) {
          const candidate = navigable[(at + step) % navigable.length];
          if (
            candidate &&
            entryLabel(candidate.entry, label).toLocaleLowerCase().startsWith(letter)
          ) {
            refs.current[candidate.index]?.focus();
            break;
          }
        }
      }
    }
    if (handled) {
      // Key bindings of the app must not see keys meant for the menu.
      event.preventDefault();
      event.stopPropagation();
    }
  };

  return (
    <div
      data-part="context-menu.list"
      data-level={level}
      role="menu"
      aria-label={name}
      aria-orientation="vertical"
    >
      {items.map((entry, index): ReactNode => {
        if (entry.type === "separator") {
          return <div key={index} data-part="context-menu.separator" role="separator" />;
        }
        if (entry.type === "header") {
          // M5b: a group heading (not focusable).
          return (
            <div key={index} data-part="context-menu.header" role="presentation">
              {menuLabelText(entry.label, label)}
            </div>
          );
        }
        const text = entryLabel(entry, label);
        const submenu = submenuOf(entry, index);
        if (submenu !== null) {
          const expanded = openSub === index;
          const disabled = emptySource(index);
          // A source's entries show once loaded; the item is expanded meanwhile.
          const children =
            submenu.kind === "static"
              ? submenu.items
              : submenu.loaded === "loading"
                ? null
                : submenu.loaded;
          const popupId = `${ids}-sub-${String(index)}`;
          return (
            <div key={index} data-part="context-menu.submenu" role="none">
              <button
                ref={(el) => {
                  refs.current[index] = el;
                }}
                type="button"
                data-part="context-menu.item"
                data-action={entry.type === "action" ? entry.action : undefined}
                data-state={expanded ? "expanded" : disabled ? "disabled" : undefined}
                role="menuitem"
                tabIndex={-1}
                aria-haspopup="menu"
                aria-expanded={expanded}
                aria-controls={expanded && children !== null ? popupId : undefined}
                aria-disabled={disabled ? true : undefined}
                aria-busy={children === null ? true : undefined}
                onClick={() => {
                  activate(entry, index, false);
                }}
                onPointerEnter={(event) => {
                  // Resting on the item opens its submenu; the focus stays here.
                  event.currentTarget.focus();
                  if (openSub !== index) activate(entry, index, false);
                }}
                onKeyDown={(event) => {
                  onKeyDown(event, entry, index);
                }}
              >
                {entry.type === "action" && (
                  <Icon name={entry.icon} part="context-menu.item.icon" />
                )}
                <span data-part="context-menu.item.label">{text}</span>
                <Icon name="chevron-right" part="context-menu.item.arrow" />
              </button>
              {expanded && children !== null && (
                <SubmenuPopup id={popupId}>
                  <MenuList
                    items={children}
                    label={text}
                    level={level + 1}
                    focusOnOpen={subFocus}
                    onRun={onRun}
                    onClose={onClose}
                    sources={sources}
                    onBack={() => {
                      setOpenSub(null);
                      refs.current[index]?.focus();
                    }}
                  />
                </SubmenuPopup>
              )}
            </div>
          );
        }
        // (A layout submenu always has a `submenu` above.)
        if (entry.type === "submenu") return null;
        const enabled = isEnabled(entry.action, entry.args);
        const shortcut = entry.shortcut;
        return (
          <button
            key={index}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            data-part="context-menu.item"
            data-action={entry.action}
            data-state={enabled ? undefined : "disabled"}
            role="menuitem"
            tabIndex={-1}
            aria-disabled={enabled ? undefined : true}
            aria-keyshortcuts={shortcut ? ariaShortcut(shortcut) : undefined}
            onClick={() => {
              activate(entry, index, false);
            }}
            onPointerEnter={(event) => {
              event.currentTarget.focus();
              setOpenSub(null);
            }}
            onKeyDown={(event) => {
              onKeyDown(event, entry, index);
            }}
          >
            <Icon name={entry.icon} part="context-menu.item.icon" />
            <span data-part="context-menu.item.label">{text}</span>
            {shortcut && (
              <span data-part="context-menu.item.shortcut" aria-hidden="true">
                {keyLabel(shortcut, t)}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/** The menu at a viewport position; closes on a click elsewhere. */
export function ContextMenu({
  items,
  label,
  x,
  y,
  focusOnOpen,
  onRun,
  onClose,
  sources,
  alignRight = false,
  owner,
}: ContextMenuProps) {
  const geom = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });
  useGeometry(geom, { left: `${String(position.x)}px`, top: `${String(position.y)}px` });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const next = menuPosition(
      { x, y },
      { width: rect.width, height: rect.height },
      { width: window.innerWidth, height: window.innerHeight },
      alignRight,
    );
    if (rect.width > 0 && (next.x !== x || next.y !== y)) setPosition(next);
  }, [x, y, alignRight]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && ref.current?.contains(event.target)) return;
      if (event.target instanceof Node && owner?.current?.contains(event.target)) return;
      onClose(false);
    };
    const onBlur = () => {
      onClose(false);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
    };
  }, [onClose, owner]);

  return (
    <div
      ref={ref}
      data-part="context-menu"
      data-geom={geom}
      data-key-context="menu"
      data-key-modal=""
      onContextMenu={(event) => {
        event.preventDefault();
      }}
    >
      <MenuList
        items={items}
        label={label}
        level={0}
        focusOnOpen={focusOnOpen}
        onRun={onRun}
        onClose={onClose}
        sources={sources}
      />
    </div>
  );
}
