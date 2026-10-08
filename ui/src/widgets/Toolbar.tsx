/**
 * A toolbar from the layout's `toolbars` section (`toolbar` option), with
 * the M5b ranked overflow (WP162, ADRs 0109 / 0111):
 *
 * - each action shows as icon, label or both (`display`, default both) and
 *   has a tooltip with its key binding ("Archive (E)");
 * - `hide-disabled` toolbars leave out actions that cannot run now (in the
 *   "More" menu too, `shownToolbarItems`);
 * - `overflow="more"` (the default) never wraps: whatever does not fit moves
 *   into a "More" menu in rank order (`fitToolbar`), and separators left at
 *   an edge disappear; the menu also holds the `more { … }` entries with
 *   their headings and shortcuts;
 * - `overflow="wrap"` wraps, and a separator never starts a line.
 *
 * Widths are measured, never assumed: when the items or the bar's width
 * change, every item is rendered once (before paint) and measured, then
 * the fit is applied.
 *
 * M5d (WP182, ADR 0131): a right-click on the bar (or Shift+F10 / the menu
 * key on one of its buttons) opens the layout's `menu "context:toolbar"`;
 * its actions run for this toolbar (Customize toolbar opens the toolbar
 * customizer, `../features/toolbar-editor`). While the customizer of a bar
 * is open, the bar carries `data-customizing`.
 */
import { useLayoutEffect, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import type { JsonValue, ResolvedMenuEntry, ToolbarItem } from "../api/generated";
import { useActions, useEnabledInputs } from "../actions/ActionsProvider";
import { comboFromEvent } from "../actions/keys";
import { keyLabel } from "../features/menus";
import { ContextMenu } from "../features/menus/ContextMenu";
import { ariaShortcut } from "../features/menus/keyLabels";
import { customizerWindow } from "../features/toolbar-editor/ids";
import { useLabel, useT } from "../i18n/I18nProvider";
import { useLayoutConfig } from "../layout/LayoutContext";
import { fitToolbar, normalizeSeparators, shownToolbarItems } from "../layout/overflow";
import { optionString, type WidgetProps } from "../layout/widgetRegistry";
import { useUi } from "../state/uiStore";
import { Icon } from "./Icon";
import {
  bindingOf,
  displayOf,
  moreMenuEntries,
  opensContextMenu,
  toolbarContextMenu,
  toolbarKeyContext,
  toolbarMenuArgs,
  toolbarSettingsOf,
  tooltipMessage,
  wrapEdges,
} from "./toolbarModel";
import { HORIZONTAL, moveRovingFocus } from "./rovingFocus";

type ActionItem = Extract<ToolbarItem, { type: "action" }>;

/** Parts the arrow keys move between. */
const FOCUSABLE = '[data-part="toolbar.button"]:not(:disabled), [data-part="toolbar.more"]';

/** Left / Right / Home / End move between a toolbar's enabled buttons. */
export function onToolbarKeyDown(event: KeyboardEvent<HTMLElement>): void {
  moveRovingFocus(event, FOCUSABLE, HORIZONTAL);
}

/**
 * `isEnabled` of the dispatcher, re-evaluated when the UI state the
 * built-in checks read changes (like `useActionEnabled`).
 */
function useEnabledCheck(): (action: string, args: readonly JsonValue[]) => boolean {
  const dispatcher = useActions();
  // The whole order: checks that read the loaded summaries (rules, Gmail
  // labels) change when the list loads.
  useUi((s) => s.messageOrder);
  useEnabledInputs();
  return (action, args) => dispatcher.isEnabled(action, args);
}

interface ButtonProps {
  item: ActionItem;
  index: number;
  keyContext: string;
}

function ToolbarButton({ item, index, keyContext }: ButtonProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const { dispatch, isEnabled } = useActions();
  const enabled = isEnabled(item.action, item.args);
  const def = Object.hasOwn(layout.actions, item.action) ? layout.actions[item.action] : undefined;
  const text = label(item.label ?? def?.label ?? item.action);
  const icon = item.icon ?? def?.icon ?? null;
  const display = displayOf(item);
  const binding = bindingOf(layout, item.action, item.args, keyContext);
  const tooltip = tooltipMessage(text, binding === null ? null : keyLabel(binding, t));
  return (
    <button
      type="button"
      data-part="toolbar.button"
      data-item={index}
      data-action={item.action}
      data-display={display}
      data-rank={item.rank}
      data-state={enabled ? undefined : "disabled"}
      disabled={!enabled}
      title={tooltip ? t(tooltip.id, tooltip.args) : text}
      aria-label={display === "icon" ? text : undefined}
      aria-keyshortcuts={binding ? ariaShortcut(binding) : undefined}
      onClick={() => void dispatch(item.action, item.args)}
    >
      {display !== "label" && <Icon name={icon} part="toolbar.button.icon" />}
      {display !== "icon" && <span data-part="toolbar.button.label">{text}</span>}
    </button>
  );
}

interface MoreProps {
  /** Index of the toolbar's own `more` item, `null` for the automatic one. */
  index: number | null;
  /** The `more` item's label, `null` = "More". */
  labelOverride: string | null;
  icon: string | null;
  entries: readonly ResolvedMenuEntry[];
  /** Rendered only to be measured (no menu yet, nothing to run). */
  measuring: boolean;
}

/** The "More" button and its menu (a context menu under the button). */
function MoreButton({ index, labelOverride, icon, entries, measuring }: MoreProps) {
  const label = useLabel();
  const { dispatch } = useActions();
  const button = useRef<HTMLButtonElement>(null);
  const [open, setOpen] = useState<{ x: number; y: number; keyboard: boolean } | null>(null);
  const wasOpen = useRef(false);
  const text = label(labelOverride ?? "@toolbar.more");
  const empty = entries.length === 0;

  const show = (keyboard: boolean) => {
    const rect = button.current?.getBoundingClientRect();
    setOpen({ x: rect?.left ?? 0, y: rect?.bottom ?? 0, keyboard });
  };
  const close = (restoreFocus: boolean) => {
    setOpen(null);
    if (restoreFocus) button.current?.focus();
  };

  return (
    <>
      <button
        ref={button}
        type="button"
        data-part="toolbar.more"
        data-item={index ?? undefined}
        data-state={open ? "expanded" : undefined}
        aria-haspopup="menu"
        aria-expanded={open !== null}
        disabled={empty && !measuring}
        title={text}
        // A click on the button while the menu is open closes it (the
        // menu's outside-click handler already did; do not reopen).
        onPointerDown={() => {
          wasOpen.current = open !== null;
        }}
        onClick={(event) => {
          if (wasOpen.current) {
            wasOpen.current = false;
            return;
          }
          if (open) close(false);
          else show(event.detail === 0);
        }}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && !open) {
            event.preventDefault();
            show(true);
          }
        }}
      >
        <Icon name={icon ?? "ellipsis"} part="toolbar.more.icon" />
        <span data-part="toolbar.more.label">{text}</span>
      </button>
      {open && !empty && (
        <ContextMenu
          items={entries}
          label={text}
          x={open.x}
          y={open.y}
          focusOnOpen={open.keyboard}
          onRun={(action, args) => {
            close(true);
            void dispatch(action, args);
          }}
          onClose={close}
        />
      )}
    </>
  );
}

/** The toolbar's own right-click menu while it is open. */
interface OpenToolbarMenu {
  items: ResolvedMenuEntry[];
  x: number;
  y: number;
  /** Opened with the keyboard. */
  keyboard: boolean;
  /** Where focus goes back to when the menu closes. */
  returnFocus: HTMLElement | null;
}

/** What the measuring pass decided: shown indices and the moved ones. */
interface Fit {
  visible: number[];
  overflow: number[];
}

function px(value: string | undefined): number {
  const n = Number.parseFloat(value ?? "");
  return Number.isFinite(n) ? n : 0;
}

/** How long a jump back to the previous width counts as a measuring flip (ms). */
const FLIP_MS = 250;

/** Measures the rendered items and fits them into the bar. */
function measure(root: HTMLElement, items: readonly ToolbarItem[]): Fit {
  const style = getComputedStyle(root);
  const gap = px(style.columnGap);
  const available = root.clientWidth - px(style.paddingLeft) - px(style.paddingRight) + gap;
  const widths = items.map((_item, i) => {
    const el = root.querySelector<HTMLElement>(`:scope > [data-item="${String(i)}"]`);
    return el ? el.getBoundingClientRect().width + gap : 0;
  });
  const auto = root.querySelector<HTMLElement>(
    ':scope > [data-part="toolbar.more"]:not([data-item])',
  );
  const moreWidth = auto ? auto.getBoundingClientRect().width + gap : 0;
  return fitToolbar(items, widths, available, moreWidth);
}

/** A toolbar from the layout's `toolbars` section (`toolbar` option). */
export function Toolbar({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const name = optionString(node, "toolbar", "");
  const items = Object.hasOwn(layout.toolbars, name) ? (layout.toolbars[name] ?? []) : [];
  const settings = toolbarSettingsOf(layout, name);
  const isEnabled = useEnabledCheck();
  const shown = shownToolbarItems(items, settings, isEnabled);
  const wraps = settings.overflow === "wrap";
  const root = useRef<HTMLDivElement>(null);
  const [keyContext, setKeyContext] = useState("global");
  const { dispatch } = useActions();
  const customizing = useUi((s) => s.windows.includes(customizerWindow(name)));
  const [menu, setMenu] = useState<OpenToolbarMenu | null>(null);

  // What the bar shows; a change measures again.
  const signature = JSON.stringify(shown);
  const [fit, setFit] = useState<{ signature: string; fit: Fit } | null>(null);
  const current = fit !== null && fit.signature === signature ? fit.fit : null;
  const measuring = !wraps && current === null;

  // Bindings are looked up in the toolbar's key context, then `global`.
  useLayoutEffect(() => {
    const closest = root.current?.closest("[data-key-context]")?.getAttribute("data-key-context");
    setKeyContext(toolbarKeyContext(name, closest));
  }, [name]);

  // Measure (before paint) whenever nothing fitted the current items yet.
  useLayoutEffect(() => {
    const el = root.current;
    if (!el || wraps || current !== null) return;
    setFit({ signature, fit: measure(el, shown) });
  }, [wraps, current, signature, shown]);

  // A new width measures again; a wrapping bar re-checks its separators.
  const [, setWidthTick] = useState(0);
  useLayoutEffect(() => {
    const el = root.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    let last = el.clientWidth;
    let previous: number | null = null;
    let changedAt = 0;
    let recheck: ReturnType<typeof setTimeout> | undefined;
    const refit = (width: number) => {
      previous = last;
      last = width;
      changedAt = performance.now();
      setFit(null);
      setWidthTick((n) => n + 1);
    };
    const observer = new ResizeObserver(() => {
      const width = el.clientWidth;
      if (width === last) return;
      // A bar whose width follows its content (a flex item without a set
      // width) would flip between the measuring and the fitted width: a
      // jump straight back to the previous width is ignored. A window made
      // wider and quickly narrower again (P1) is no flip, though: when the
      // bar still overflows once the moment has passed, it fits again.
      if (width === previous && performance.now() - changedAt < FLIP_MS) {
        clearTimeout(recheck);
        recheck = setTimeout(() => {
          if (el.isConnected && el.scrollWidth > el.clientWidth + 1) refit(el.clientWidth);
        }, FLIP_MS);
        return;
      }
      refit(width);
    });
    observer.observe(el);
    return () => {
      clearTimeout(recheck);
      observer.disconnect();
    };
  }, []);

  // Wrapping: separators at a line's start are removed, at its end hidden.
  useLayoutEffect(() => {
    const el = root.current;
    if (!el || !wraps) return;
    const rendered = [...el.querySelectorAll<HTMLElement>(":scope > [data-item]")];
    for (const child of rendered) child.removeAttribute("data-wrap-edge");
    const kinds = rendered.map((child) => ({
      kind: shown[Number(child.dataset.item)]?.type ?? "action",
      top: child.offsetTop,
    }));
    wrapEdges(kinds).forEach((edge, i) => {
      if (edge) rendered[i]?.setAttribute("data-wrap-edge", edge);
    });
  });

  /** Opens the toolbar's own menu; `false` when the layout has none. */
  const openMenu = (point: { x: number; y: number } | null, from: Element | null): boolean => {
    const items = toolbarContextMenu(layout, keyContext);
    if (items === null) return false;
    const rect = (from ?? root.current)?.getBoundingClientRect();
    const active = document.activeElement;
    setMenu({
      items,
      x: Math.round(point?.x ?? rect?.left ?? 0),
      y: Math.round(point?.y ?? rect?.bottom ?? 0),
      keyboard: point === null,
      returnFocus: active instanceof HTMLElement && active !== document.body ? active : null,
    });
    return true;
  };
  const onContextMenu = (event: MouseEvent<HTMLDivElement>) => {
    if (event.defaultPrevented) return;
    // A keyboard-invoked event (the menu key) has no pointer position.
    const keyboard = event.clientX === 0 && event.clientY === 0 && event.button !== 2;
    const target = event.target instanceof Element ? event.target : null;
    if (openMenu(keyboard ? null : { x: event.clientX, y: event.clientY }, target)) {
      event.preventDefault();
    }
  };
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // Shift+F10 (a `menu.context` binding) on one of the bar's buttons.
    if (opensContextMenu(layout, comboFromEvent(event))) {
      const target = event.target instanceof Element ? event.target : null;
      if (openMenu(null, target)) event.preventDefault();
      return;
    }
    onToolbarKeyDown(event);
  };
  const closeMenu = (restoreFocus: boolean) => {
    const back = menu?.returnFocus ?? null;
    setMenu(null);
    if (restoreFocus && back?.isConnected) back.focus();
  };

  const all = shown.map((_item, i) => i);
  const indices = wraps ? normalizeSeparators(shown, all) : (current?.visible ?? all);
  const overflow = current?.overflow ?? [];
  const entries = moreMenuEntries(layout, shown, overflow, keyContext);
  const ownMore = shown.findIndex((item) => item.type === "more");
  const autoMore = ownMore < 0 && (measuring || overflow.length > 0);

  return (
    <>
      <div
        ref={root}
        data-part="toolbar"
        data-toolbar={name}
        data-node-id={node.id ?? undefined}
        data-overflow={wraps ? "wrap" : "more"}
        data-state={measuring ? "measuring" : undefined}
        data-customizing={customizing ? "" : undefined}
        role="toolbar"
        aria-label={node.label ? label(node.label) : t("common-message-actions")}
        onKeyDown={onKeyDown}
        onContextMenu={onContextMenu}
      >
        {indices.map((index) => {
          const item = shown[index];
          switch (item?.type) {
            case "action":
              return (
                <ToolbarButton key={index} item={item} index={index} keyContext={keyContext} />
              );
            case "spacer":
              return <span key={index} data-part="toolbar.spacer" data-item={index} />;
            case "separator":
              return (
                <span
                  key={index}
                  data-part="toolbar.separator"
                  data-item={index}
                  role="separator"
                />
              );
            case "more":
              // Its own entries or overflow to show (or being measured).
              return entries.length > 0 || measuring ? (
                <MoreButton
                  key={index}
                  index={index}
                  labelOverride={item.label}
                  icon={item.icon}
                  entries={entries}
                  measuring={measuring}
                />
              ) : null;
            default:
              return null;
          }
        })}
        {autoMore && (
          <MoreButton
            index={null}
            labelOverride={null}
            icon={null}
            entries={entries}
            measuring={measuring}
          />
        )}
      </div>
      {menu && (
        <ContextMenu
          items={menu.items}
          label={t("customize-toolbar-choose")}
          x={menu.x}
          y={menu.y}
          focusOnOpen
          onRun={(action, args) => {
            // The action may open a window that takes focus.
            closeMenu(false);
            void dispatch(action, toolbarMenuArgs(name, args));
          }}
          onClose={closeMenu}
        />
      )}
    </>
  );
}
