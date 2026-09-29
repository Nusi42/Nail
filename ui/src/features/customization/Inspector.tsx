/**
 * The inspector (design.md, "Inspector"): a background component that, while
 * the element picker is on (`inspector.toggle`), highlights the element under
 * the pointer and shows its part path, states, the theme rules that apply
 * and the layout node that placed it, with "open file at line". A click
 * pins the panel to an element; Escape (or the action again) ends picking.
 *
 * The highlight box follows the element through a geometry rule of the
 * theme's stylesheet (`useGeometry`: viewport position and size), never an
 * inline style; everything else about the panel and the box (placement,
 * colours, layering) comes from the theme's `inspector.*` parts.
 */
import { useEffect, useId, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useWindowWidth } from "../../layout/useWindowWidth";
import { useGeometry, useTheme } from "../../theme/ThemeProvider";
import { SourceLink } from "./SourceLink";
import {
  INSPECTOR_PART_PREFIX,
  conditionsOf,
  layoutNodeOf,
  matchingRules,
  partElement,
  partPath,
  statesOf,
} from "./inspect";
import { customizationStore, useCustomization } from "./state";

function isInspectorElement(target: EventTarget | null): boolean {
  return (
    target instanceof Element &&
    (target.closest("[data-part]")?.getAttribute("data-part")?.startsWith(INSPECTOR_PART_PREFIX) ??
      false)
  );
}

/** Pointer, click and key handling while picking. */
function usePicker(inspecting: boolean, pinned: boolean): void {
  useEffect(() => {
    if (!inspecting) return;
    const { inspect, setInspecting } = customizationStore.getState();
    const onMove = (event: Event) => {
      if (pinned || isInspectorElement(event.target)) return;
      inspect(partElement(event.target as Element | null), false);
    };
    const onClick = (event: Event) => {
      if (isInspectorElement(event.target)) return;
      // The click picks an element instead of using it.
      event.preventDefault();
      event.stopPropagation();
      inspect(partElement(event.target as Element | null), true);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setInspecting(false);
      }
    };
    document.addEventListener("pointerover", onMove, true);
    document.addEventListener("mouseover", onMove, true);
    document.addEventListener("click", onClick, true);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerover", onMove, true);
      document.removeEventListener("mouseover", onMove, true);
      document.removeEventListener("click", onClick, true);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [inspecting, pinned]);
}

function Highlight({ element }: { element: Element }) {
  const geom = useId();
  const [rect, setRect] = useState(() => element.getBoundingClientRect());
  useEffect(() => {
    const update = () => setRect(element.getBoundingClientRect());
    update();
    window.addEventListener("scroll", update, true);
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("scroll", update, true);
      window.removeEventListener("resize", update);
    };
  }, [element]);
  useGeometry(geom, {
    position: "fixed",
    "pointer-events": "none",
    left: `${Math.round(rect.left)}px`,
    top: `${Math.round(rect.top)}px`,
    width: `${Math.round(rect.width)}px`,
    height: `${Math.round(rect.height)}px`,
  });
  return <div data-part="inspector.highlight" data-geom={geom} aria-hidden="true" />;
}

function Details({ element }: { element: Element }) {
  const t = useT();
  const theme = useTheme();
  const layout = useLayoutConfig();
  const width = useWindowWidth();
  const path = partPath(element);
  const states = statesOf(element);
  const conditions = conditionsOf(element);
  const rules = matchingRules(element, theme);
  const node = layoutNodeOf(element, layout, width);

  return (
    <>
      <section data-part="inspector.section" data-section="part">
        <h3 data-part="inspector.heading">{t("inspector-part")}</h3>
        <ol data-part="inspector.path">
          {path.map((part, index) => (
            <li
              key={index}
              data-part="inspector.path.item"
              data-state={index === path.length - 1 ? "current" : undefined}
            >
              {part}
            </li>
          ))}
        </ol>
      </section>
      <section data-part="inspector.section" data-section="states">
        <h3 data-part="inspector.heading">{t("inspector-states")}</h3>
        {states.length === 0 && conditions.length === 0 ? (
          <p data-part="inspector.none">{t("inspector-no-states")}</p>
        ) : (
          <ul data-part="inspector.states">
            {states.map((state) => (
              <li key={`s-${state}`} data-part="inspector.state">
                {t("inspector-state", { state })}
              </li>
            ))}
            {conditions.map((c) => (
              <li key={`c-${c.key}`} data-part="inspector.condition">
                {c.value === null
                  ? t("inspector-condition-set", { key: c.key })
                  : t("inspector-condition", { key: c.key, value: c.value })}
              </li>
            ))}
          </ul>
        )}
      </section>
      <section data-part="inspector.section" data-section="rules">
        <h3 data-part="inspector.heading">{t("inspector-rules", { count: rules.length })}</h3>
        {rules.length === 0 && <p data-part="inspector.none">{t("inspector-no-rules")}</p>}
        <ul data-part="inspector.rules">
          {rules.map((rule, index) => (
            <li key={index} data-part="inspector.rule">
              <code data-part="inspector.rule.selector">{rule.text}</code>
              {rule.when.length > 0 && (
                <span data-part="inspector.rule.when">
                  {t("inspector-rule-when", { states: rule.when.join(", ") })}
                </span>
              )}
              <ul data-part="inspector.rule.declarations">
                {Object.entries(rule.declarations).map(([property, value]) => (
                  <li key={property} data-part="inspector.rule.declaration">
                    {t("inspector-declaration", { property, value })}
                  </li>
                ))}
              </ul>
              {rule.source && (
                <SourceLink
                  part="inspector.source"
                  file={rule.source.file}
                  line={rule.source.line}
                  column={rule.source.column}
                />
              )}
            </li>
          ))}
        </ul>
      </section>
      <section data-part="inspector.section" data-section="layout">
        <h3 data-part="inspector.heading">{t("inspector-layout")}</h3>
        {node ? (
          <div data-part="inspector.node">
            <code data-part="inspector.node.kind">
              {node.node.id
                ? t("inspector-node-with-id", { kind: node.kind, id: node.node.id })
                : t("inspector-node", { kind: node.kind })}
            </code>
            <span data-part="inspector.node.place">
              {t("inspector-node-place", {
                window: node.window,
                breakpoint: node.breakpoint,
                path: node.path.join("/") || "-",
              })}
            </span>
            {node.source ? (
              <SourceLink
                part="inspector.source"
                file={node.source.file}
                line={node.source.line}
                column={node.source.column}
              />
            ) : (
              <span data-part="inspector.none">{t("inspector-no-source")}</span>
            )}
          </div>
        ) : (
          <p data-part="inspector.none">{t("inspector-no-node")}</p>
        )}
      </section>
    </>
  );
}

function Panel({ element, pinned }: { element: Element | null; pinned: boolean }) {
  const t = useT();
  const { setInspecting, inspect } = customizationStore.getState();
  return (
    <aside
      data-part="inspector"
      data-state={pinned ? "pinned" : "picking"}
      aria-label={t("inspector-title")}
    >
      <div data-part="inspector.header">
        <h2 data-part="inspector.title">{t("inspector-title")}</h2>
        {pinned && (
          <button
            type="button"
            data-part="inspector.button"
            data-action="pick"
            onClick={() => inspect(null, false)}
          >
            {t("inspector-pick-again")}
          </button>
        )}
        <button
          type="button"
          data-part="inspector.button"
          data-action="close"
          onClick={() => setInspecting(false)}
        >
          {t("inspector-close")}
        </button>
      </div>
      <p data-part="inspector.hint">{t(pinned ? "inspector-hint-pinned" : "inspector-hint")}</p>
      {element?.isConnected ? (
        <Details element={element} />
      ) : (
        <p data-part="inspector.none">{t("inspector-empty")}</p>
      )}
    </aside>
  );
}

/** The inspector overlay; renders nothing unless picking is on. */
export function Inspector() {
  const inspecting = useCustomization((s) => s.inspecting);
  const inspected = useCustomization((s) => s.inspected);
  const pinned = useCustomization((s) => s.pinned);
  usePicker(inspecting, pinned);
  if (!inspecting) return null;
  return (
    <>
      {inspected?.isConnected && <Highlight element={inspected} />}
      <Panel element={inspected} pinned={pinned} />
    </>
  );
}
