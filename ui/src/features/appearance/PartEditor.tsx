/**
 * The theme editor's per-part overrides (M5d WP183): every part selector a
 * rule of the theme names (base rules, rules of the theme it extends, its
 * own), searchable; the chosen part's rules with their properties — the
 * theme's own properties are edited in place, inherited ones can be
 * overridden — a new property, and "Reset part" (the theme's own rule
 * goes). The inspector's "Edit style" opens this tab on the part it
 * picked, even when no rule names it yet.
 */
import { useId, useState, type SubmitEvent } from "react";
import type { ThemePartRule } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import { Tag } from "../../widgets/primitives";
import type { ThemeEdits } from "./useThemeEdits";
import {
  isPartSelector,
  isPropertyName,
  partEntries,
  selectorMatches,
  selectorPart,
  type PartEntry,
} from "./tokens";

/** Properties offered when adding one (any property the core accepts can be typed). */
export const COMMON_PROPERTIES = [
  "background-color",
  "color",
  "border",
  "border-color",
  "border-radius",
  "box-shadow",
  "font-family",
  "font-size",
  "font-weight",
  "gap",
  "letter-spacing",
  "line-height",
  "margin",
  "opacity",
  "padding",
  "text-transform",
] as const;

/** At most this many parts are listed at once (the search narrows the rest). */
export const PART_LIST_LIMIT = 200;

const draftOf = (selector: string, property: string) => `part:${selector}|${property}`;

function PropertyValue({
  selector,
  property,
  value,
  edits,
}: {
  selector: string;
  property: string;
  value: string;
  edits: ThemeEdits;
}) {
  const t = useT();
  const [text, setText] = useState(value);
  const commit = () => {
    const next = text.trim();
    if (next === value || next === "") {
      setText(value);
      return;
    }
    edits.change(
      draftOf(selector, property),
      { op: "set-part-property", selector, property, value: next },
      next,
      true,
    );
  };
  return (
    <input
      type="text"
      data-part="form.input"
      data-field="property-value"
      aria-label={t("theme-editor-property-value", { property })}
      spellCheck={false}
      value={text}
      onChange={(event) => {
        setText(event.target.value);
      }}
      onBlur={commit}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          event.preventDefault();
          commit();
        }
      }}
    />
  );
}

function RuleView({
  rule,
  selector,
  editable,
  edits,
  overridden,
}: {
  rule: ThemePartRule;
  selector: string;
  editable: boolean;
  edits: ThemeEdits;
  /** Properties the theme's own rule sets (they win over this rule). */
  overridden: ReadonlySet<string>;
}) {
  const t = useT();
  const own = rule.origin === "own";
  const entries = Object.entries(rule.properties).filter(
    ([property]) => edits.drafts.get(draftOf(selector, property)) !== null || !own,
  );
  return (
    <section data-part="theme-editor.rule" data-origin={rule.origin}>
      <Tag text={t(`theme-editor-origin-${rule.origin}`)} kind="origin" />
      {entries.length === 0 ? (
        <p data-part="theme-editor.empty">{t("theme-editor-rule-empty")}</p>
      ) : (
        <dl data-part="theme-editor.properties">
          {entries.map(([property, value]) => {
            const draft = edits.drafts.get(draftOf(selector, property));
            const shown = draft ?? value;
            return (
              <div
                key={property}
                data-part="theme-editor.property"
                data-state={!own && overridden.has(property) ? "overridden" : undefined}
              >
                <dt data-part="theme-editor.property.name">{property}</dt>
                <dd data-part="theme-editor.property.value">
                  {own && editable ? (
                    <>
                      <PropertyValue
                        key={shown}
                        selector={selector}
                        property={property}
                        value={shown}
                        edits={edits}
                      />
                      <button
                        type="button"
                        data-part="form.button"
                        data-variant="icon"
                        aria-label={t("theme-editor-property-remove", { property })}
                        title={t("theme-editor-property-remove", { property })}
                        onClick={() => {
                          edits.change(
                            draftOf(selector, property),
                            { op: "set-part-property", selector, property, value: null },
                            null,
                            true,
                          );
                        }}
                      >
                        <Icon name="x" part="form.button.icon" />
                      </button>
                    </>
                  ) : (
                    <>
                      <code data-part="theme-editor.property.text">{shown}</code>
                      {editable && !overridden.has(property) && (
                        <button
                          type="button"
                          data-part="form.button"
                          data-variant="ghost"
                          aria-label={t("theme-editor-property-override-label", { property })}
                          onClick={() => {
                            edits.change(
                              draftOf(selector, property),
                              { op: "set-part-property", selector, property, value: shown },
                              shown,
                              true,
                            );
                          }}
                        >
                          {t("theme-editor-property-override")}
                        </button>
                      )}
                    </>
                  )}
                </dd>
              </div>
            );
          })}
        </dl>
      )}
    </section>
  );
}

function AddProperty({ selector, edits }: { selector: string; edits: ThemeEdits }) {
  const t = useT();
  const id = useId();
  const [property, setProperty] = useState("");
  const [value, setValue] = useState("");
  const [problem, setProblem] = useState(false);
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!isPropertyName(property) || value.trim() === "") {
      setProblem(true);
      return;
    }
    setProblem(false);
    const name = property.trim();
    edits.change(
      draftOf(selector, name),
      { op: "set-part-property", selector, property: name, value: value.trim() },
      value.trim(),
      true,
    );
    setProperty("");
    setValue("");
  };
  return (
    <form
      data-part="theme-editor.add-property"
      noValidate
      aria-label={t("theme-editor-add-property")}
      onSubmit={submit}
    >
      <div data-part="form.field" data-field="property">
        <label data-part="form.label" htmlFor={`${id}-property`}>
          {t("theme-editor-property")}
        </label>
        <input
          id={`${id}-property`}
          type="text"
          data-part="form.input"
          list={`${id}-properties`}
          spellCheck={false}
          value={property}
          onChange={(event) => {
            setProperty(event.target.value);
          }}
        />
        <datalist id={`${id}-properties`}>
          {COMMON_PROPERTIES.map((p) => (
            <option key={p} value={p} />
          ))}
        </datalist>
      </div>
      <div data-part="form.field" data-field="value">
        <label data-part="form.label" htmlFor={`${id}-value`}>
          {t("theme-editor-value")}
        </label>
        <input
          id={`${id}-value`}
          type="text"
          data-part="form.input"
          spellCheck={false}
          value={value}
          onChange={(event) => {
            setValue(event.target.value);
          }}
        />
      </div>
      <button type="submit" data-part="form.button">
        {t("theme-editor-add")}
      </button>
      {problem && (
        <p data-part="form.error" role="alert">
          {t("theme-editor-property-invalid")}
        </p>
      )}
    </form>
  );
}

function PartView({
  entry,
  editable,
  edits,
}: {
  entry: PartEntry;
  editable: boolean;
  edits: ThemeEdits;
}) {
  const t = useT();
  const own = entry.rules.find((r) => r.origin === "own");
  const overridden = new Set(Object.keys(own?.properties ?? {}));
  for (const [key, value] of edits.drafts) {
    const prefix = `part:${entry.selector}|`;
    if (key.startsWith(prefix) && value !== null) overridden.add(key.slice(prefix.length));
  }
  // Properties added in this session before the theme answered.
  const added = [...edits.drafts].filter(
    ([key, value]) =>
      key.startsWith(`part:${entry.selector}|`) &&
      value !== null &&
      !Object.hasOwn(own?.properties ?? {}, key.slice(`part:${entry.selector}|`.length)),
  );
  const rules: ThemePartRule[] =
    added.length > 0 && !own
      ? [...entry.rules, { selector: entry.selector, properties: {}, origin: "own" }]
      : entry.rules;
  return (
    <section data-part="theme-editor.part" aria-label={entry.selector}>
      <div data-part="theme-editor.part.header">
        <code data-part="theme-editor.part.selector">{entry.selector}</code>
        {editable && (own !== undefined || added.length > 0) && (
          <button
            type="button"
            data-part="form.button"
            onClick={() => {
              edits.change(
                `reset:${entry.selector}`,
                { op: "reset-part", selector: entry.selector },
                null,
                true,
              );
            }}
          >
            {t("theme-editor-reset-part")}
          </button>
        )}
      </div>
      {rules.length === 0 && (
        <p data-part="theme-editor.empty">{t("theme-editor-part-no-rules")}</p>
      )}
      {rules.map((rule, index) => (
        <RuleView
          key={`${rule.origin}-${String(index)}`}
          rule={
            rule.origin === "own"
              ? {
                  ...rule,
                  properties: {
                    ...rule.properties,
                    ...Object.fromEntries(
                      added.map(([key, value]) => [
                        key.slice(`part:${entry.selector}|`.length),
                        value ?? "",
                      ]),
                    ),
                  },
                }
              : rule
          }
          selector={entry.selector}
          editable={editable}
          edits={edits}
          overridden={rule.origin === "own" ? new Set() : overridden}
        />
      ))}
      {editable && <AddProperty selector={entry.selector} edits={edits} />}
    </section>
  );
}

/** The Parts tab. */
export function PartEditor({
  parts,
  editable,
  edits,
  focus,
}: {
  parts: readonly ThemePartRule[];
  editable: boolean;
  edits: ThemeEdits;
  /** The part to show first (the inspector's "Edit style"). */
  focus: string | null;
}) {
  const t = useT();
  const id = useId();
  const [added, setAdded] = useState<string[]>([]);
  const [filter, setFilter] = useState(() => (focus === null ? "" : selectorPart(focus)));
  const [selected, setSelected] = useState<string | null>(() => {
    if (focus === null) return null;
    // The theme's own rule for the part, else any rule naming it, else the part itself.
    const named = parts.filter((p) => selectorMatches(p.selector, selectorPart(focus)));
    return (
      named.find((p) => p.origin === "own" && p.selector === focus)?.selector ??
      named.find((p) => p.selector === focus)?.selector ??
      focus
    );
  });
  const [newSelector, setNewSelector] = useState("");
  const [problem, setProblem] = useState(false);
  const extra = [...added, ...(selected === null ? [] : [selected])];
  const entries = partEntries(parts, extra);
  const needle = filter.trim().toLowerCase();
  const matching = entries.filter((e) => needle === "" || e.selector.includes(needle));
  const listed = matching.slice(0, PART_LIST_LIMIT);
  const current = entries.find((e) => e.selector === selected) ?? null;
  return (
    <div data-part="theme-editor.parts">
      <div data-part="theme-editor.part-picker">
        <div data-part="form.field" data-field="part-filter">
          <label data-part="form.label" htmlFor={`${id}-filter`}>
            {t("theme-editor-part-filter")}
          </label>
          <input
            id={`${id}-filter`}
            type="search"
            data-part="form.input"
            spellCheck={false}
            value={filter}
            onChange={(event) => {
              setFilter(event.target.value);
            }}
          />
        </div>
        <ul data-part="theme-editor.part-list" aria-label={t("theme-editor-parts-label")}>
          {listed.map((entry) => (
            <li key={entry.selector} data-part="theme-editor.part-list.item">
              <button
                type="button"
                data-part="theme-editor.part-button"
                data-state={entry.own ? "own" : undefined}
                aria-pressed={entry.selector === selected}
                onClick={() => {
                  setSelected(entry.selector);
                }}
              >
                {entry.selector}
              </button>
            </li>
          ))}
        </ul>
        {matching.length > listed.length && (
          <p data-part="form.hint">
            {t("theme-editor-parts-more", { count: matching.length - listed.length })}
          </p>
        )}
        {editable && (
          <form
            data-part="theme-editor.add-part"
            noValidate
            aria-label={t("theme-editor-add-part")}
            onSubmit={(event) => {
              event.preventDefault();
              const selector = newSelector.trim();
              if (!isPartSelector(selector)) {
                setProblem(true);
                return;
              }
              setProblem(false);
              setAdded((a) => [...a, selector]);
              setSelected(selector);
              setNewSelector("");
            }}
          >
            <div data-part="form.field" data-field="selector">
              <label data-part="form.label" htmlFor={`${id}-selector`}>
                {t("theme-editor-selector")}
              </label>
              <input
                id={`${id}-selector`}
                type="text"
                data-part="form.input"
                spellCheck={false}
                value={newSelector}
                onChange={(event) => {
                  setNewSelector(event.target.value);
                }}
              />
            </div>
            <button type="submit" data-part="form.button">
              {t("theme-editor-add")}
            </button>
            {problem && (
              <p data-part="form.error" role="alert">
                {t("theme-editor-selector-invalid")}
              </p>
            )}
          </form>
        )}
      </div>
      {current ? (
        <PartView key={current.selector} entry={current} editable={editable} edits={edits} />
      ) : (
        <p data-part="theme-editor.empty">{t("theme-editor-pick-part")}</p>
      )}
    </div>
  );
}
