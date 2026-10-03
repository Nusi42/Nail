/**
 * The settings page (`settings-page`, M5b WP163, ADR 0109): the layout's
 * `settings-page` section as one page — grouped navigation on the left
 * (General, Look & feel, Security, Extensions → sections), the shown
 * section on the right with each of its widgets on a card, and a search
 * over every section, group and setting label (`../../layout/settingsIndex.ts`
 * over the labels each feature registers) with highlighted matches and
 * deep links to the setting. The shown section is the core's UI state
 * (`settings.section`), so the page opens where it was left.
 */
import {
  Fragment,
  useEffect,
  useId,
  useMemo,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type { LayoutNode, SettingsGroup, SettingsSection } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { NodeView } from "../../layout/LayoutRenderer";
import {
  buildSettingsIndex,
  searchSettings,
  type SettingsIndexEntry,
} from "../../layout/settingsIndex";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { UI_STATE_SETTINGS_SECTION, useSetUiState, useUiStateValue } from "../../state/uiState";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { appRegistry } from "../registry";
import { currentSection, findSection, highlightText, rankSettingsHits } from "./model";
import { settingsStoreFor, useSettingsState, type RevealRequest } from "./store";

/** Setting matches listed under a search result. */
export const RESULT_SETTINGS = 4;
/** How long a revealed setting stays marked (ms). */
export const FOUND_MARK_MS = 2500;
/** How long the page waits for a setting that loads late (ms). */
export const REVEAL_WAIT_MS = 3000;

/** The attribute that marks a revealed setting for the skin. */
export const FOUND_ATTRIBUTE = "data-settings-found";

function Highlight({ text, query }: { text: string; query: string }) {
  return (
    <>
      {highlightText(text, query).map((piece, i) =>
        piece.match ? (
          <mark key={i} data-part="settings-page.mark">
            {piece.text}
          </mark>
        ) : (
          <Fragment key={i}>{piece.text}</Fragment>
        ),
      )}
    </>
  );
}

const sectionLabelOf = (section: SettingsSection) =>
  section.label ?? `@settings.section.${section.id}`;
const groupLabelOf = (group: SettingsGroup) => group.label ?? `@settings.group.${group.id}`;

const FOCUSABLE =
  'input:not([type="hidden"]):not(:disabled), select:not(:disabled), textarea:not(:disabled), button:not(:disabled), [tabindex]:not([tabindex="-1"])';
const ROWS = '[data-part="form.field"], [data-part="switch-field"], [data-part="form.check"]';

/** The deepest element under `scope` whose whole text is `text`. */
function elementWithText(scope: Element, text: string): HTMLElement | null {
  let found: HTMLElement | null = null;
  const walk = (el: Element) => {
    if (el.getAttribute("aria-hidden") === "true" || el.closest("[hidden]")) return;
    if (el instanceof HTMLElement && el.textContent?.trim() === text) found = el;
    for (const child of el.children) walk(child);
  };
  walk(scope);
  return found;
}

/** Where a reveal request points inside the section body (`null`: not rendered yet). */
function revealTarget(body: Element, request: RevealRequest): HTMLElement | null {
  const cards = [...body.querySelectorAll<HTMLElement>('[data-part="settings-page.card"]')];
  const card =
    (request.node !== null
      ? cards.find((c) => c.getAttribute("data-card-for") === request.node)
      : undefined) ??
    (request.widget !== null
      ? cards.find((c) => c.getAttribute("data-widget") === request.widget)
      : undefined) ??
    null;
  if (request.text === null) return card;
  return elementWithText(card ?? body, request.text);
}

/** The control a revealed label belongs to, if any. */
function controlOf(el: HTMLElement): HTMLElement | null {
  if (el instanceof HTMLLabelElement && el.htmlFor) {
    const target = document.getElementById(el.htmlFor);
    if (target) return target;
  }
  if (el.matches(FOCUSABLE)) return el;
  return el.closest(ROWS)?.querySelector<HTMLElement>(FOCUSABLE) ?? null;
}

/** Scrolls to a found setting, marks it for a moment and focuses its control. */
function showFound(el: HTMLElement, fallbackFocus: HTMLElement | null): () => void {
  el.scrollIntoView?.({ block: "center" });
  el.setAttribute(FOUND_ATTRIBUTE, "");
  const control = controlOf(el);
  (control ?? fallbackFocus)?.focus({ preventScroll: true });
  const timer = setTimeout(() => el.removeAttribute(FOUND_ATTRIBUTE), FOUND_MARK_MS);
  return () => {
    clearTimeout(timer);
    el.removeAttribute(FOUND_ATTRIBUTE);
  };
}

/** The section's widgets, each on a card (a stack's rows; else the whole tree). */
function SectionCards({ section }: { section: SettingsSection }) {
  const hidden = useUi((s) => s.hiddenNodes);
  const root = section.root;
  const children: LayoutNode[] =
    root.type === "stack"
      ? root.children.filter((child) => child.id === null || !hidden.includes(child.id))
      : [root];
  return (
    <>
      {children.map((child, index) => (
        <div
          key={child.id ?? index}
          data-part="settings-page.card"
          data-widget={child.type === "widget" ? child.kind : undefined}
          data-card-for={child.id ?? undefined}
          tabIndex={-1}
        >
          <NodeView node={child} path={`settings/${section.id}/${index}`} />
        </div>
      ))}
    </>
  );
}

interface SearchProps {
  query: string;
  onQuery(query: string): void;
  onFirst(): void;
  onDown(): void;
  focusSignal: number;
}

function SearchBox({ query, onQuery, onFirst, onDown, focusSignal }: SearchProps) {
  const t = useT();
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (focusSignal === 0) return;
    input.current?.focus();
    input.current?.select();
  }, [focusSignal]);
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      onFirst();
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      onDown();
    } else if (event.key === "Escape" && query !== "") {
      // Clears the search; with an empty box, Escape goes on (closes the window).
      event.preventDefault();
      event.stopPropagation();
      onQuery("");
    }
  };
  return (
    <div data-part="settings-page.search" role="search">
      <Icon name="search" part="settings-page.search.icon" />
      <input
        ref={input}
        type="search"
        data-part="settings-page.search.input"
        aria-label={t("settings-search-label")}
        placeholder={t("settings-search-placeholder")}
        autoComplete="off"
        spellCheck={false}
        value={query}
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={onKeyDown}
      />
      {query !== "" && (
        <button
          type="button"
          data-part="settings-page.search.clear"
          aria-label={t("settings-search-clear")}
          onClick={() => {
            onQuery("");
            input.current?.focus();
          }}
        >
          <Icon name="x" part="settings-page.search.clear.icon" />
        </button>
      )}
    </div>
  );
}

/** The setting entries listed under a result: settings only, each text once. */
function settingMatches(matches: readonly SettingsIndexEntry[]): SettingsIndexEntry[] {
  const seen = new Set<string>();
  const out: SettingsIndexEntry[] = [];
  for (const entry of matches) {
    if (entry.kind !== "setting" || seen.has(entry.text)) continue;
    seen.add(entry.text);
    out.push(entry);
  }
  return out.slice(0, RESULT_SETTINGS);
}

export function SettingsPage({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const groupId = useId();
  const layout = useLayoutConfig();
  const uiStore = useUiStoreApi();
  const settingsStore = settingsStoreFor(uiStore);
  const query = useSettingsState((s) => s.query);
  const focusSearch = useSettingsState((s) => s.focusSearch);
  const reveal = useSettingsState((s) => s.reveal);
  const remembered = useUiStateValue(UI_STATE_SETTINGS_SECTION);
  const setUiState = useSetUiState();
  const showSearch = optionBool(node, "search", true);
  const sectionId = currentSection(layout, remembered);
  const section = sectionId === null ? null : findSection(layout, sectionId);
  const group = layout.settings.find((g) => g.sections.some((s) => s.id === sectionId)) ?? null;
  const body = useRef<HTMLDivElement>(null);
  const title = useRef<HTMLHeadingElement>(null);
  const results = useRef<HTMLUListElement>(null);
  /** Takes the mark off the setting revealed last. */
  const unmark = useRef<(() => void) | null>(null);

  const index = useMemo(
    () => buildSettingsIndex(layout, appRegistry.settings, label, t),
    [layout, label, t],
  );
  const hits = useMemo(() => rankSettingsHits(searchSettings(index, query), query), [index, query]);
  const searching = showSearch && query.trim() !== "";

  const go = (id: string) => {
    if (id !== sectionId) setUiState.mutate({ key: UI_STATE_SETTINGS_SECTION, value: id });
  };
  const open = (id: string, entry: SettingsIndexEntry | null) => {
    go(id);
    settingsStore.getState().requestReveal({
      section: id,
      node: null,
      widget: entry?.widget ?? null,
      text: entry?.text ?? null,
    });
  };

  // A new section starts at its top.
  useEffect(() => {
    body.current?.scrollTo?.({ top: 0 });
  }, [sectionId]);

  // Reveal the requested setting once the section shows it (widgets may
  // render their rows after their data arrived).
  useEffect(() => {
    if (reveal === null || reveal.section !== sectionId) return;
    const root = body.current;
    if (!root) return;
    const { seq } = reveal;
    let observer: MutationObserver | null = null;
    const finish = (el: HTMLElement | null) => {
      observer?.disconnect();
      observer = null;
      clearTimeout(timer);
      unmark.current?.();
      unmark.current = null;
      if (el) {
        unmark.current = showFound(el, el.closest<HTMLElement>('[data-part="settings-page.card"]'));
      } else title.current?.focus();
      settingsStore.getState().revealed(seq);
    };
    const timer = setTimeout(() => {
      // Not found in time: the widget's card (or the section) instead.
      const fallback = revealTarget(root, { ...reveal, text: null });
      finish(fallback);
    }, REVEAL_WAIT_MS);
    const found = revealTarget(root, reveal);
    if (found) finish(found);
    else {
      observer = new MutationObserver(() => {
        const el = revealTarget(root, reveal);
        if (el) finish(el);
      });
      observer.observe(root, { childList: true, subtree: true, characterData: true });
    }
    return () => {
      observer?.disconnect();
      clearTimeout(timer);
    };
  }, [reveal, sectionId, settingsStore]);
  useEffect(() => {
    const mark = unmark;
    return () => mark.current?.();
  }, []);

  if (layout.settings.length === 0) {
    return (
      <div data-part="settings-page" data-node-id={node.id ?? undefined} data-state="empty">
        <p data-part="settings-page.empty">{t("settings-page-empty")}</p>
      </div>
    );
  }

  const sectionText = (s: SettingsSection) => label(sectionLabelOf(s));
  const navigation: ReactNode = searching ? (
    <>
      <p data-part="settings-page.result-count" role="status">
        {t("settings-search-count", { count: hits.length, query: query.trim() })}
      </p>
      {hits.length > 0 && (
        <ul
          ref={results}
          data-part="settings-page.results"
          aria-label={t("settings-search-results")}
        >
          {hits.map((hit) => {
            const s = findSection(layout, hit.section);
            if (!s) return null;
            const settings = settingMatches(hit.matches);
            return (
              <li key={hit.section} data-part="settings-page.result" data-section={hit.section}>
                <button
                  type="button"
                  data-part="settings-page.result.section"
                  aria-current={hit.section === sectionId ? "page" : undefined}
                  onClick={() => open(hit.section, null)}
                >
                  <Icon name={s.icon} part="settings-page.nav.icon" />
                  <span data-part="settings-page.nav.label">
                    <Highlight text={sectionText(s)} query={query} />
                  </span>
                </button>
                {settings.length > 0 && (
                  <ul data-part="settings-page.result.settings">
                    {settings.map((entry) => (
                      <li key={entry.text}>
                        <button
                          type="button"
                          data-part="settings-page.result.setting"
                          onClick={() => open(hit.section, entry)}
                        >
                          <Highlight text={entry.text} query={query} />
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </>
  ) : (
    layout.settings.map((g) => (
      <div
        key={g.id}
        data-part="settings-page.group"
        data-group={g.id}
        role="group"
        aria-labelledby={`${groupId}-${g.id}`}
      >
        <span data-part="settings-page.group.label" id={`${groupId}-${g.id}`}>
          {label(groupLabelOf(g))}
        </span>
        <ul data-part="settings-page.group.list">
          {g.sections.map((s) => (
            <li key={s.id} data-part="settings-page.nav.entry">
              <button
                type="button"
                data-part="settings-page.nav.item"
                data-section={s.id}
                aria-current={s.id === sectionId ? "page" : undefined}
                onClick={() => go(s.id)}
              >
                <Icon name={s.icon} part="settings-page.nav.icon" />
                <span data-part="settings-page.nav.label">{sectionText(s)}</span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    ))
  );

  return (
    <div data-part="settings-page" data-node-id={node.id ?? undefined}>
      <nav data-part="settings-page.nav" aria-label={t("settings-page-nav")}>
        {showSearch && (
          <SearchBox
            query={query}
            onQuery={(q) => settingsStore.getState().setQuery(q)}
            focusSignal={focusSearch}
            onFirst={() => {
              const first = hits[0];
              if (first) open(first.section, null);
            }}
            onDown={() => results.current?.querySelector<HTMLElement>("button")?.focus()}
          />
        )}
        {navigation}
      </nav>
      {/* Not a region: the section's widgets often are, under the same name. */}
      <div data-part="settings-page.content" data-section={section?.id}>
        <header data-part="settings-page.header">
          {group && <span data-part="settings-page.breadcrumb">{label(groupLabelOf(group))}</span>}
          <h2 data-part="settings-page.title" ref={title} tabIndex={-1}>
            {section ? sectionText(section) : t("settings-page-section-missing")}
          </h2>
        </header>
        <div data-part="settings-page.body" ref={body}>
          {section && <SectionCards key={section.id} section={section} />}
        </div>
      </div>
    </div>
  );
}
