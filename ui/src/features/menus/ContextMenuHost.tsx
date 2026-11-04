/**
 * Mounted once in the app shell: opens the layout's context menu for the
 * part under a right-click (and for keyboard `contextmenu` events), renders
 * the open menu without the actions that do not apply to its target (M5b)
 * and runs its actions with the menu's target as context.
 *
 * ADR 0184: the one `contextmenu` handler of the app. It cancels every
 * right-click — the browser's (WebView2's) menu never shows — and opens
 * Nail's menu for the place, or nothing. A mail frame's document (opaque
 * origin, ADR 0212) cancels its own right-click in its script and asks for
 * the menu; its `<iframe>` then dispatches `FRAME_MENU_EVENT` with the
 * point and the selected text, and this host opens the text menu.
 *
 * Shift+F10 and the menu key are ordinary key bindings of the action
 * `menu.context` (default layout), so they can be rebound.
 */
import { useCallback, useEffect } from "react";
import type { JsonValue } from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useUiStoreApi } from "../../state/uiStore";
import { ContextMenu } from "./ContextMenu";
import { withEditShortcuts } from "./editing";
import { withoutConversationShortcuts, withoutDisabled } from "./resolve";
import { menuStoreFor, openContextMenu, openTextMenu, useMenu } from "./store";
import { TEXT_PART } from "./targets";
import { FRAME_MENU_EVENT, type FrameMenu } from "../../theme/frameHeight";

/** Whether a `contextmenu` event came from the keyboard (no pointer position). */
const fromKeyboard = (event: Pick<MouseEvent, "clientX" | "clientY" | "button">) =>
  event.clientX === 0 && event.clientY === 0 && event.button !== 2;

export function ContextMenuHost() {
  const t = useT();
  const ui = useUiStoreApi();
  const layout = useLayoutConfig();
  const { dispatch, isEnabled } = useActions();
  const open = useMenu((s) => s.open);

  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      // A menu of a part's own (a toolbar's) opened already.
      if (event.defaultPrevented) return;
      event.preventDefault();
      const element = event.target instanceof Element ? event.target : null;
      const point = fromKeyboard(event) ? null : { x: event.clientX, y: event.clientY };
      openContextMenu(ui, layout, element, { point });
    };
    // Mail frames: the text menu, placed in the page's coordinates.
    const onFrameMenu = (event: Event) => {
      const frame = event.target;
      if (!(frame instanceof HTMLIFrameElement) || !(event instanceof CustomEvent)) return;
      const menu = event.detail as FrameMenu;
      const keyboard = fromKeyboard({ clientX: menu.x, clientY: menu.y, button: menu.button });
      const rect = frame.getBoundingClientRect();
      const inside = keyboard ? { x: 0, y: 0 } : { x: menu.x, y: menu.y };
      openTextMenu(
        ui,
        layout,
        frame,
        { point: { x: rect.left + inside.x, y: rect.top + inside.y }, keyboard },
        { range: null, field: null, frame: { text: menu.text, window: frame.contentWindow } },
      );
    };
    const listening = new AbortController();
    const { signal } = listening;
    document.addEventListener("contextmenu", onContextMenu, { signal });
    document.addEventListener(FRAME_MENU_EVENT, onFrameMenu, { signal });
    return () => {
      listening.abort();
    };
  }, [ui, layout]);

  const close = useCallback(
    (restoreFocus: boolean) => {
      menuStoreFor(ui).getState().close(restoreFocus);
    },
    [ui],
  );

  const run = useCallback(
    (action: string, args: readonly JsonValue[]) => {
      // The handler reads the menu's target synchronously, before the close.
      const done = dispatch(action, args);
      close(true);
      void done;
    },
    [dispatch, close],
  );

  // Actions that do not apply to the target are hidden, not greyed out.
  const enabled = open ? withoutDisabled(open.menu.items, isEnabled) : [];
  // The text menu shows the editing keys; a card's actions act on its
  // message, not on the conversation the keys reach.
  const items =
    open?.target.part === TEXT_PART
      ? withEditShortcuts(enabled)
      : open?.target.card
        ? withoutConversationShortcuts(enabled)
        : enabled;
  useEffect(() => {
    if (open && items.length === 0) close(true);
  }, [open, items.length, close]);

  if (!open || items.length === 0) return null;
  return (
    <ContextMenu
      key={`${open.menu.name}-${String(open.x)}-${String(open.y)}`}
      items={items}
      label={t(`context-menu-label-${open.target.part.replaceAll(".", "-")}`)}
      x={open.x}
      y={open.y}
      focusOnOpen
      onRun={run}
      onClose={close}
    />
  );
}
