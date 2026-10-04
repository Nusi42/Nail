/**
 * Layout engine: turns a resolved layout window into a React tree.
 * Containers: `split` (resizable by pointer and keyboard), `stack`, `tabs`.
 * Leaves are looked up in the widget registry.
 *
 * M5d (ADR 0131): hidden nodes (`hidden=#true`) are skipped, and every node
 * goes through the layout chrome (`./LayoutChrome.ts`), which the layout
 * editor uses to draw its pane headers in customize mode. A chrome
 * registered for a window (`setWindowChrome`, WP181) is provided around that
 * window and may frame it (`renderWindow`).
 */
import {
  Fragment,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from "react";
import type { LayoutNode, ResolvedLayout } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { useUi } from "../state/uiStore";
import { useGeometry, useStyleManager } from "../theme/ThemeProvider";
import {
  defaultStackSize,
  flexForSize,
  isValidSize,
  renderedStackTracks,
  resizeSplit,
  selectBreakpointIndex,
  trackForSize,
} from "./engine";
import {
  LayoutChromeContext,
  chromeForWindow,
  nodeRefOfPath,
  rendersNode,
  useLayoutChrome,
  useWindowChrome,
} from "./LayoutChrome";
import { useWindowWidth } from "./useWindowWidth";
import { useWidgetRegistry, type WidgetNode } from "./widgetRegistry";

type SplitNode = Extract<LayoutNode, { type: "split" }>;
type StackNode = Extract<LayoutNode, { type: "stack" }>;
type TabsNode = Extract<LayoutNode, { type: "tabs" }>;

/** Pixels a divider moves per arrow key press (Shift: ×4). */
export const KEYBOARD_STEP = 16;

/** Stable state key of a node: its id, or its path in the tree. */
function nodeKeyOf(node: LayoutNode, path: string): string {
  return node.id ?? path;
}

function useVisibleChildren(children: readonly LayoutNode[]) {
  const hidden = useUi((s) => s.hiddenNodes);
  const chrome = useLayoutChrome();
  return (
    children
      .map((child, index) => ({ child, index }))
      // Customize mode shows the panes `view.toggle` hid for now, too.
      .filter(({ child }) => chrome.showHidden || child.id === null || !hidden.includes(child.id))
      .filter(({ child }) => rendersNode(child, chrome))
  );
}

export interface WindowViewProps {
  layout: ResolvedLayout;
  /** Window type, e.g. `"main"`. */
  name?: string;
}

/** Renders the breakpoint of window `name` that matches the window width. */
export function WindowView({ layout, name = "main" }: WindowViewProps) {
  const t = useT();
  const width = useWindowWidth();
  const inherited = useLayoutChrome();
  const registered = useWindowChrome(name);
  const chrome = chromeForWindow(name, registered, inherited);
  const breakpoints = layout.windows[name] ?? [];
  const index = selectBreakpointIndex(breakpoints, width);
  const breakpoint = breakpoints[index];
  if (!breakpoint) {
    return <div data-part="layout.missing">{t("layout-missing-window", { name })}</div>;
  }
  const path = `${name}/${String(index)}`;
  const tree = <NodeView key={path} node={breakpoint.root} path={path} />;
  const content = chrome.renderWindow ? chrome.renderWindow(name, index, tree) : tree;
  return chrome === inherited ? (
    content
  ) : (
    <LayoutChromeContext.Provider value={chrome}>{content}</LayoutChromeContext.Provider>
  );
}

interface NodeViewProps {
  node: LayoutNode;
  path: string;
}

export function NodeView({ node, path }: NodeViewProps) {
  const chrome = useLayoutChrome();
  const content = nodeContent(node, path);
  const ref = nodeRefOfPath(path);
  return ref ? chrome.renderNode(node, ref, content) : content;
}

function nodeContent(node: LayoutNode, path: string): ReactNode {
  switch (node.type) {
    case "split":
      return <SplitView node={node} path={path} />;
    case "stack":
      return <StackView node={node} path={path} />;
    case "tabs":
      return <TabsView node={node} path={path} />;
    case "widget":
      return <WidgetView node={node} path={path} />;
  }
}

function WidgetView({ node, path }: { node: WidgetNode; path: string }) {
  const t = useT();
  const registry = useWidgetRegistry();
  const Widget = Object.hasOwn(registry, node.kind) ? registry[node.kind] : undefined;
  if (!Widget) {
    return (
      <div
        data-part="layout.unknown-widget"
        data-kind={node.kind}
        data-node-id={node.id ?? undefined}
      >
        {t("layout-unknown-widget", { kind: node.kind })}
      </div>
    );
  }
  return <Widget node={node} nodeKey={nodeKeyOf(node, path)} />;
}

function StackView({ node, path }: { node: StackNode; path: string }) {
  const label = useLabel();
  const geom = useId();
  const visible = useVisibleChildren(node.children);
  const tracks = visible.map(({ child, index }) => {
    // A hidden child the chrome still shows (customize mode) is a header only.
    if (child.hidden === true) return "auto";
    const size = node.sizes[index];
    return trackForSize(size && isValidSize(size) ? size : defaultStackSize(child));
  });
  const element = useRef<HTMLDivElement>(null);
  const manager = useStyleManager();
  const tracksKey = JSON.stringify(tracks);
  // The rows go only to the children that rendered something (see
  // `renderedStackTracks`). Written straight to the stylesheet: widgets
  // appear and disappear without this stack re-rendering.
  useLayoutEffect(
    () => () => {
      manager.removeGeometry(geom);
    },
    [manager, geom],
  );
  useLayoutEffect(() => {
    const el = element.current;
    if (!el) return;
    const all = JSON.parse(tracksKey) as string[];
    const update = () => {
      const marks = [...el.children].map((child) =>
        child.getAttribute("data-part") === "stack.slot"
          ? Number(child.getAttribute("data-slot"))
          : null,
      );
      const rows = renderedStackTracks(all, marks).join(" ");
      manager.setGeometry(geom, { "grid-template-rows": rows || "none" });
    };
    update();
    const observer = new MutationObserver(update);
    observer.observe(el, { childList: true });
    return () => {
      observer.disconnect();
    };
  }, [manager, geom, tracksKey]);
  return (
    <div
      ref={element}
      data-part="stack"
      data-geom={geom}
      data-node-id={node.id ?? undefined}
      role={node.label ? "region" : undefined}
      aria-label={node.label ? label(node.label) : undefined}
    >
      {visible.map(({ child, index }, i) => (
        <Fragment key={child.id ?? index}>
          {/* Not a grid item (hidden): marks where child `i` starts. */}
          <span data-part="stack.slot" data-slot={i} hidden />
          <NodeView node={child} path={`${path}/${String(index)}`} />
        </Fragment>
      ))}
    </div>
  );
}

interface PaneProps {
  geom: string;
  flex: string;
  min: string | null | undefined;
  max: string | null | undefined;
  horizontal: boolean;
  child: LayoutNode;
  path: string;
  paneRef: (el: HTMLDivElement | null) => void;
}

function Pane({ geom, flex, min, max, horizontal, child, path, paneRef }: PaneProps) {
  const declarations: Record<string, string> = { flex };
  const axis = horizontal ? "width" : "height";
  if (min && isValidSize(min)) declarations[`min-${axis}`] = min;
  if (max && isValidSize(max)) declarations[`max-${axis}`] = max;
  useGeometry(geom, declarations);
  return (
    <div data-part="split.pane" data-geom={geom} ref={paneRef}>
      <NodeView node={child} path={path} />
    </div>
  );
}

interface DragState {
  pointerId: number;
  start: number;
  sizes: string[];
  measured: number[];
  index: number;
}

function SplitView({ node, path }: { node: SplitNode; path: string }) {
  const t = useT();
  const geom = useId();
  const key = nodeKeyOf(node, path);
  const stored = useUi((s) => s.splitSizes[key]);
  const setSplitSizes = useUi((s) => s.setSplitSizes);
  const visible = useVisibleChildren(node.children);
  const panes = useRef(new Map<number, HTMLDivElement>());
  const drag = useRef<DragState | null>(null);
  const [valueNow, setValueNow] = useState(50);
  const horizontal = node.orientation === "horizontal";

  const allSizes = node.children.map((_, i) => stored?.[i] ?? node.sizes[i] ?? "1fr");
  // A hidden child the chrome still shows (customize mode) is a header only.
  const visibleSizes = visible.map(({ child, index }) =>
    child.hidden === true ? "auto" : (allSizes[index] ?? "1fr"),
  );

  const measure = () =>
    visible.map(({ index }) => {
      const rect = panes.current.get(index)?.getBoundingClientRect();
      return rect ? (horizontal ? rect.width : rect.height) : 0;
    });

  const apply = (visibleIndex: number, delta: number, base: string[], measured: number[]) => {
    const next = resizeSplit({
      sizes: base,
      measured,
      index: visibleIndex,
      delta,
      minSizes: visible.map(({ index }) => node.min_sizes[index] ?? null),
      maxSizes: visible.map(({ index }) => node.max_sizes[index] ?? null),
    });
    const full = [...allSizes];
    visible.forEach(({ index }, i) => {
      full[index] = next[i] ?? full[index] ?? "1fr";
    });
    setSplitSizes(key, full);
    const a = measured[visibleIndex] ?? 0;
    const b = measured[visibleIndex + 1] ?? 0;
    if (a + b > 0) setValueNow(Math.round(((a + delta) / (a + b)) * 100));
  };

  const onPointerDown = (visibleIndex: number) => (event: PointerEvent<HTMLDivElement>) => {
    if (!node.resizable || event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = {
      pointerId: event.pointerId,
      start: horizontal ? event.clientX : event.clientY,
      sizes: visibleSizes,
      measured: measure(),
      index: visibleIndex,
    };
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const state = drag.current;
    if (state?.pointerId !== event.pointerId) return;
    const position = horizontal ? event.clientX : event.clientY;
    apply(state.index, position - state.start, state.sizes, state.measured);
  };

  const onPointerUp = (event: PointerEvent<HTMLDivElement>) => {
    if (drag.current?.pointerId === event.pointerId) {
      drag.current = null;
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
  };

  const onKeyDown = (visibleIndex: number) => (event: KeyboardEvent<HTMLDivElement>) => {
    const back = horizontal ? "ArrowLeft" : "ArrowUp";
    const forward = horizontal ? "ArrowRight" : "ArrowDown";
    if (event.key !== back && event.key !== forward) return;
    event.preventDefault();
    const step = KEYBOARD_STEP * (event.shiftKey ? 4 : 1);
    apply(visibleIndex, event.key === forward ? step : -step, visibleSizes, measure());
  };

  const children: ReactNode[] = [];
  visible.forEach(({ child, index }, i) => {
    if (i > 0) {
      children.push(
        node.resizable ? (
          // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- a focusable separator is the window splitter widget (WAI-ARIA): pointer drag and arrow keys.
          <div
            key={`divider-${String(index)}`}
            data-part="split.divider"
            role="separator"
            // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- a focusable separator is the window splitter widget (WAI-ARIA), moved with the arrow keys.
            tabIndex={0}
            aria-orientation={horizontal ? "vertical" : "horizontal"}
            aria-valuenow={valueNow}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-label={t("layout-divider")}
            onPointerDown={onPointerDown(i - 1)}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={onPointerUp}
            onKeyDown={onKeyDown(i - 1)}
          />
        ) : (
          <div key={`divider-${String(index)}`} data-part="split.divider" aria-hidden="true" />
        ),
      );
    }
    children.push(
      <Pane
        key={child.id ?? `pane-${String(index)}`}
        geom={`${geom}-${String(index)}`}
        flex={flexForSize(visibleSizes[i])}
        min={child.hidden === true ? null : node.min_sizes[index]}
        max={child.hidden === true ? null : node.max_sizes[index]}
        horizontal={horizontal}
        child={child}
        path={`${path}/${String(index)}`}
        paneRef={(el) => {
          if (el) panes.current.set(index, el);
          else panes.current.delete(index);
        }}
      />,
    );
  });

  return (
    <div data-part="split" data-orientation={node.orientation} data-node-id={node.id ?? undefined}>
      {children}
    </div>
  );
}

function TabsView({ node, path }: { node: TabsNode; path: string }) {
  const t = useT();
  const label = useLabel();
  const idPrefix = useId();
  const key = nodeKeyOf(node, path);
  const stored = useUi((s) => s.activeTabs[key]);
  const setActiveTab = useUi((s) => s.setActiveTab);
  const visible = useVisibleChildren(node.children);
  const tabRefs = useRef(new Map<number, HTMLButtonElement>());
  const wanted = stored ?? node.active;
  const activeIndex = visible.some(({ index }) => index === wanted)
    ? wanted
    : (visible[0]?.index ?? 0);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const position = visible.findIndex(({ index }) => index === activeIndex);
    let next: number;
    if (event.key === "ArrowRight") next = (position + 1) % visible.length;
    else if (event.key === "ArrowLeft") next = (position - 1 + visible.length) % visible.length;
    else if (event.key === "Home") next = 0;
    else if (event.key === "End") next = visible.length - 1;
    else return;
    event.preventDefault();
    const target = visible[next];
    if (target) {
      setActiveTab(key, target.index);
      tabRefs.current.get(target.index)?.focus();
    }
  };

  const tabLabel = (child: LayoutNode, index: number) =>
    child.label ? label(child.label) : t("layout-tab-untitled", { index: index + 1 });

  return (
    <div data-part="tabs" data-node-id={node.id ?? undefined}>
      {/* eslint-disable-next-line jsx-a11y/interactive-supports-focus -- the tabs take the focus (roving tabindex), not the tab list itself. */}
      <div
        data-part="tabs.tab-list"
        role="tablist"
        aria-label={node.label ? label(node.label) : undefined}
        onKeyDown={onKeyDown}
      >
        {visible.map(({ child, index }) => {
          const selected = index === activeIndex;
          return (
            <button
              key={child.id ?? index}
              type="button"
              role="tab"
              id={`${idPrefix}-tab-${String(index)}`}
              aria-selected={selected}
              aria-controls={`${idPrefix}-panel-${String(index)}`}
              tabIndex={selected ? 0 : -1}
              data-part="tabs.tab"
              data-state={selected ? "selected" : undefined}
              data-node-id={child.id ?? undefined}
              ref={(el) => {
                if (el) tabRefs.current.set(index, el);
                else tabRefs.current.delete(index);
              }}
              onClick={() => {
                setActiveTab(key, index);
              }}
            >
              {tabLabel(child, index)}
            </button>
          );
        })}
      </div>
      {visible.map(({ child, index }) => (
        <div
          key={child.id ?? index}
          role="tabpanel"
          id={`${idPrefix}-panel-${String(index)}`}
          aria-labelledby={`${idPrefix}-tab-${String(index)}`}
          data-part="tabs.panel"
          hidden={index !== activeIndex}
        >
          <NodeView node={child} path={`${path}/${String(index)}`} />
        </div>
      ))}
    </div>
  );
}
