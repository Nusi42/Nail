/**
 * The view host (layout widget `view-host`, M5b WP161, ADR 0109): shows the
 * active view's window tree (`views { view "<id>" window="<name>" }`), so
 * Mail, Calendar, Contacts, Tasks and Settings are full views with the
 * widgets their windows already hold (the PIM widgets in the default
 * layout's `calendar`, `contacts` and `tasks` windows).
 *
 * - **Remembered view:** the last view is kept in the core's UI state
 *   (`shell.view`) and shown again at the next start; a remembered view the
 *   layout no longer defines is ignored. The host waits for the UI state
 *   (one request) so the start view does not flash first.
 * - **State kept:** a view stays mounted (hidden) once shown, so switching
 *   back keeps the selected message, scroll positions and the calendar's
 *   date.
 * - **Focus:** each view is a labelled region ("Calendar view": the widgets
 *   inside may use the view's own name) with its own keyboard context
 *   (`calendar`, `contacts`, `tasks`, else `global`). When the view changes
 *   while focus is not on the rail (a shortcut such as Ctrl+2, the palette,
 *   a menu), focus moves to the new view's region, so it never stays in a
 *   hidden view; a click on the rail keeps focus on the rail button.
 */
import { useEffect, useEffectEvent, useLayoutEffect, useRef, useState } from "react";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { WindowView } from "../../layout/LayoutRenderer";
import { currentView, viewDef, viewLabel } from "../../layout/views";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { UI_STATE_VIEW, useSetUiState, useUiState } from "../../state/uiState";
import { useUi } from "../../state/uiStore";
import { canHost, rememberedView, viewKeyContext } from "./model";

/** Widget `view-host`. */
export function ViewHost({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const uiState = useUiState();
  const setUiState = useSetUiState();
  const activeView = useUi((s) => s.activeView);
  const setActiveView = useUi((s) => s.setActiveView);
  const ready = !uiState.isPending;
  const storedValue =
    uiState.data !== undefined && Object.hasOwn(uiState.data, UI_STATE_VIEW)
      ? (uiState.data[UI_STATE_VIEW] ?? null)
      : null;
  const remembered = rememberedView(layout, storedValue);
  const view = ready ? currentView(layout, activeView ?? remembered) : null;

  // Restore the remembered view once (unless the user switched already).
  const restored = useRef(false);
  useEffect(() => {
    if (!ready || restored.current) return;
    restored.current = true;
    if (activeView === null && remembered !== null) setActiveView(remembered);
  }, [ready, activeView, remembered, setActiveView]);

  // Remember every switch of this window's view — only a switch: when
  // another session (a remote browser shares the UI state) remembers its
  // view, writing ours back would make the two answer each other forever.
  const { mutate } = setUiState;
  const lastSaved = useRef<string | null>(null);
  // `storedValue` is read, not watched (see above).
  const remember = useEffectEvent((view: string) => {
    if (storedValue !== view) mutate({ key: UI_STATE_VIEW, value: view });
  });
  useEffect(() => {
    if (!ready || activeView === null || viewDef(layout, activeView) === null) return;
    if (lastSaved.current === activeView) return;
    lastSaved.current = activeView;
    remember(activeView);
  }, [ready, activeView, layout]);

  // Views shown so far stay mounted (hidden).
  const [visited, setVisited] = useState<readonly string[]>([]);
  if (view !== null && !visited.includes(view)) setVisited([...visited, view]);

  // Focus follows a switch that did not start on the rail.
  const regions = useRef(new Map<string, HTMLElement>());
  const shownBefore = useRef<string | null>(null);
  useLayoutEffect(() => {
    const before = shownBefore.current;
    shownBefore.current = view;
    if (before === null || view === null || before === view) return;
    const focused = document.activeElement;
    if (focused instanceof Element && focused.closest('[data-part="nav-rail"]')) return;
    regions.current.get(view)?.focus();
  }, [view]);

  if (view === null) return null;
  const mounted = visited.filter((id) => viewDef(layout, id) !== null);
  return (
    <div data-part="view-host" data-node-id={node.id ?? undefined} data-view={view}>
      {mounted.map((id) => {
        const def = viewDef(layout, id);
        if (def === null) return null;
        return (
          <section
            key={id}
            data-part="view-host.view"
            data-view={id}
            data-window={def.window}
            data-key-context={viewKeyContext(id, def)}
            aria-label={t("shell-view-region", { view: label(viewLabel(layout, id)) })}
            tabIndex={-1}
            hidden={id !== view}
            ref={(el) => {
              if (el) regions.current.set(id, el);
              else regions.current.delete(id);
            }}
          >
            {canHost(layout, id) ? (
              <WindowView layout={layout} name={def.window} />
            ) : (
              <div data-part="view-host.missing" role="status">
                {t("shell-view-unavailable", { view: label(viewLabel(layout, id)) })}
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}
