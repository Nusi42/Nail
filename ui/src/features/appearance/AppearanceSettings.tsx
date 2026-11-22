/**
 * Settings → Appearance (M5a WP153, M5d WP183; widget `appearance-settings`):
 * the theme gallery with live previews (built-in and the user's own
 * themes; a user theme's card shows its own colours as soon as the editor
 * changes them) and "New theme", Follow Windows with its light / dark pair,
 * Comfortable / Compact density, the accent / text-size / corner-radius
 * overrides (the accent is off with a note while a high-contrast theme is
 * active, ADR 0161), "Edit theme" for the full theme editor, and Undo /
 * Redo of the appearance history.
 * Everything is written by the core into the user's `theme.kdl` (or the
 * theme's file); this page only asks for it.
 *
 * R6 (ADR 0239): a settings section — nothing applies until Save. A picked
 * theme, Follow Windows, the pair, the density and the overrides are one
 * settings draft (`useSettingsDraft`): the section's "Unsaved changes" bar
 * saves them (the theme first, then the preferences) or discards them, and
 * leaving the section asks. "Use theme defaults" empties the overrides in
 * the draft.
 */
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import {
  DENSITY_MESSAGE_IDS,
  DENSITY_VALUES,
  PREVIEW_TOKENS,
  THEME_KIND_MESSAGE_IDS,
  THEME_OVERRIDES_ISSUE_MESSAGE_IDS,
} from "../../api/generated";
import type {
  Appearance,
  LocalizedMessage,
  ThemeInfo,
  ThemeKind,
  ThemeOverrides,
  ThemePreferences,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { isUnsupported } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { useGeometry, useTheme } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { Segmented, Switch } from "../../widgets/primitives";
import { openNewTheme, openThemeEditor } from "./editorStore";
import { HistoryButtons } from "./HistoryButtons";
import { FONT_SIZE_RANGE, RADIUS_RANGE, accentIgnored, overrideIssues } from "./model";
import { preferencesOf, useAppearance, useSetTheme, useSetThemePreferences } from "./queries";
import { useThemeName } from "./ThemeEditor";
import { previewColors } from "./themes";
import { pickerColor, pixels } from "./tokens";
import { useSettingsDraft } from "../../widgets/dialog";

function Swatch({ geom, color }: { geom: string; color: string }) {
  useGeometry(geom, { "background-color": color });
  return <span data-part="theme-card.swatch" data-geom={geom} />;
}

function ThemeCard({
  theme,
  themes,
  selected,
  active,
  onPick,
}: {
  theme: ThemeInfo;
  /** Every listed theme (a user theme previews with the built-in it extends). */
  themes: readonly ThemeInfo[];
  selected: boolean;
  active: boolean;
  onPick: () => void;
}) {
  const t = useT();
  const name = useThemeName()(theme);
  const geom = useId();
  const colors = previewColors(theme, themes);
  const usable = theme.problem === undefined;
  return (
    <button
      type="button"
      role="radio"
      data-part="theme-card"
      data-theme-id={theme.id}
      data-kind={theme.kind}
      data-state={
        [active ? "active" : "", usable ? "" : "invalid"].filter(Boolean).join(" ") || undefined
      }
      aria-checked={selected}
      aria-describedby={usable ? undefined : `${geom}-problem`}
      disabled={!usable}
      onClick={onPick}
    >
      <span data-part="theme-card.preview" aria-hidden="true">
        {PREVIEW_TOKENS.map((token) => {
          const color = colors[token];
          return color ? <Swatch key={token} geom={`${geom}-${token}`} color={color} /> : null;
        })}
      </span>
      <span data-part="theme-card.name">{name}</span>
      <span data-part="theme-card.kind">
        {t(theme.source === "user" ? "appearance-theme-meta-user" : "appearance-theme-meta", {
          kind: t(THEME_KIND_MESSAGE_IDS[theme.kind]),
        })}
      </span>
      {active && <Icon name="check" part="theme-card.check" />}
      {theme.problem && (
        <span data-part="theme-card.problem" id={`${geom}-problem`}>
          {t(theme.problem.id, theme.problem.args)}
        </span>
      )}
    </button>
  );
}

function PairSelect({
  label,
  value,
  themes,
  kind,
  name,
  onChange,
}: {
  label: string;
  value: string;
  themes: readonly ThemeInfo[];
  kind: ThemeKind;
  name: string;
  onChange: (id: string) => void;
}) {
  const id = useId();
  const nameOf = useThemeName();
  const options = themes.filter((th) => th.kind === kind && th.problem === undefined);
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {label}
      </label>
      <select
        id={id}
        data-part="form.select"
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      >
        {options.map((th) => (
          <option key={th.id} value={th.id}>
            {nameOf(th)}
          </option>
        ))}
      </select>
    </div>
  );
}

/** Text in the override form (empty = no override). */
interface OverrideDraft {
  fontSize: string;
  radius: string;
  accent: string;
}

function draftOf(o: ThemeOverrides): OverrideDraft {
  return {
    fontSize: o.font_size === undefined ? "" : String(o.font_size),
    radius: o.radius === undefined ? "" : String(o.radius),
    accent: o.accent ?? "",
  };
}

/** The overrides a draft stands for (`null` = a number field is not a number). */
export function overridesOf(draft: OverrideDraft): ThemeOverrides | null {
  const out: ThemeOverrides = {};
  const num = (text: string) => (text.trim() === "" ? undefined : Number(text));
  const fontSize = num(draft.fontSize);
  const radius = num(draft.radius);
  if (Number.isNaN(fontSize) || Number.isNaN(radius)) return null;
  if (fontSize !== undefined) out.font_size = fontSize;
  if (radius !== undefined) out.radius = radius;
  if (draft.accent.trim() !== "") out.accent = draft.accent.trim();
  return out;
}

/** A pixel slider with its value; "theme default" while there is no override. */
function PixelSlider({
  name,
  label,
  value,
  fallback,
  range,
  disabled,
  onChange,
}: {
  name: string;
  label: string;
  /** The override as typed (empty = none). */
  value: string;
  /** The theme's own value (px) shown while there is no override. */
  fallback: number;
  range: readonly [number, number];
  disabled: boolean;
  onChange: (value: string) => void;
}) {
  const t = useT();
  const id = useId();
  const own = value.trim() !== "" && !Number.isNaN(Number(value));
  const current = own ? Number(value) : Math.min(range[1], Math.max(range[0], fallback));
  const text = own
    ? t("appearance-pixels", { value: current })
    : t("appearance-pixels-default", { value: current });
  return (
    <div data-part="form.field" data-field={name}>
      <label data-part="form.label" htmlFor={id}>
        {label}
      </label>
      <span data-part="appearance.slider">
        <input
          id={id}
          type="range"
          data-part="appearance.range"
          min={range[0]}
          max={range[1]}
          step={1}
          value={current}
          disabled={disabled}
          aria-valuetext={text}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
        <output data-part="appearance.range-value" htmlFor={id}>
          {text}
        </output>
      </span>
    </div>
  );
}

function OverridesForm({
  appearance,
  active,
  busy,
  draft,
  onDraft,
  problem,
}: {
  appearance: Appearance;
  /** The theme the section shows as picked (saved or not). */
  active: string;
  busy: boolean;
  /** The overrides as edited (saved with the section's Save). */
  draft: OverrideDraft;
  onDraft: (draft: OverrideDraft) => void;
  /** Why the overrides cannot be saved, else `null`. */
  problem: LocalizedMessage | null;
}) {
  const t = useT();
  const id = useId();
  const theme = useTheme();
  const set = (key: keyof OverrideDraft) => (value: string) => {
    onDraft({ ...draft, [key]: value });
  };
  const fontSize = pixels(theme.tokens["font-size"]?.body) ?? FONT_SIZE_RANGE[0];
  const radius = pixels(theme.tokens.radius?.md) ?? RADIUS_RANGE[0];
  const picked =
    pickerColor(draft.accent) ?? pickerColor(theme.tokens.color?.accent ?? "") ?? "#000000";
  // A high-contrast theme keeps its own accent (ADR 0161): the stored
  // value stays and is shown, but cannot be changed here.
  const accentOff = accentIgnored(active, appearance.themes);
  const accentNote = accentOff ? `${id}-accent-note` : undefined;
  return (
    <div data-part="appearance.overrides" role="group" aria-labelledby={`${id}-title`}>
      <h3 data-part="card.title" id={`${id}-title`}>
        {t("appearance-overrides-title")}
      </h3>
      <p data-part="form.hint">{t("appearance-overrides-hint")}</p>
      <div data-part="form.field" data-field="accent">
        <label data-part="form.label" htmlFor={`${id}-accent`}>
          {t("appearance-accent")}
        </label>
        <span data-part="appearance.accent">
          <input
            id={`${id}-accent`}
            data-part="form.input"
            type="text"
            inputMode="text"
            spellCheck={false}
            placeholder={t("appearance-override-default")}
            value={draft.accent}
            disabled={accentOff}
            aria-describedby={accentNote}
            onChange={(event) => {
              set("accent")(event.target.value);
            }}
          />
          <input
            data-part="appearance.accent-picker"
            type="color"
            aria-label={t("appearance-accent-pick")}
            value={picked}
            disabled={busy || accentOff}
            aria-describedby={accentNote}
            onChange={(event) => {
              set("accent")(event.target.value);
            }}
          />
        </span>
        {accentNote && (
          <p data-part="appearance.accent-note" id={accentNote}>
            {t("appearance-accent-high-contrast")}
          </p>
        )}
      </div>
      <PixelSlider
        name="font-size"
        label={t("appearance-font-size")}
        value={draft.fontSize}
        fallback={fontSize}
        range={FONT_SIZE_RANGE}
        disabled={busy}
        onChange={set("fontSize")}
      />
      <PixelSlider
        name="radius"
        label={t("appearance-radius")}
        value={draft.radius}
        fallback={radius}
        range={RADIUS_RANGE}
        disabled={busy}
        onChange={set("radius")}
      />
      {problem && (
        <p data-part="form.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <div data-part="appearance.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="reset-overrides"
          disabled={busy}
          onClick={() => {
            onDraft(draftOf({}));
          }}
        >
          {t("appearance-overrides-reset")}
        </button>
      </div>
    </div>
  );
}

/** Why `draft` cannot be saved (`null`: it can). */
function overrideProblem(draft: OverrideDraft): LocalizedMessage | null {
  const overrides = overridesOf(draft);
  if (overrides === null) return { id: "appearance-overrides-not-a-number" };
  const issue = overrideIssues(overrides)[0];
  if (!issue) return null;
  const range = issue === "font-size-range" ? FONT_SIZE_RANGE : RADIUS_RANGE;
  return { id: THEME_OVERRIDES_ISSUE_MESSAGE_IDS[issue], args: { min: range[0], max: range[1] } };
}

/** The unsaved choices of the section (`undefined`: as saved). */
interface AppearanceDraft {
  theme?: string;
  prefs?: Partial<Omit<ThemePreferences, "overrides">>;
  overrides?: OverrideDraft;
}

/** The settings draft key of the Appearance section. */
export const APPEARANCE_DRAFT = "appearance-settings";

/**
 * The section's draft: what is shown (`view`), whether it differs from the
 * saved settings, and Save / Discard for the save bar.
 */
function useAppearanceDraft(saved: Appearance | undefined) {
  const setTheme = useSetTheme();
  const setPreferences = useSetThemePreferences();
  const [draft, setDraft] = useState<AppearanceDraft>({});
  const savedOverrides = saved ? draftOf(saved.settings.overrides) : draftOf({});
  const overrides = draft.overrides ?? savedOverrides;
  const problem = overrideProblem(overrides);
  const changedPrefs =
    saved !== undefined &&
    Object.entries(draft.prefs ?? {}).some(
      ([key, value]) => saved.settings[key as keyof typeof saved.settings] !== value,
    );
  const changedOverrides =
    draft.overrides !== undefined &&
    JSON.stringify(overridesOf(draft.overrides)) !== JSON.stringify(overridesOf(savedOverrides));
  const changedTheme =
    saved !== undefined && draft.theme !== undefined && draft.theme !== saved.settings.theme;
  const dirty = changedTheme || changedPrefs || changedOverrides;
  const save = async (): Promise<boolean> => {
    if (saved === undefined) return true;
    const own = overridesOf(overrides);
    if (own === null || problem !== null) return false;
    try {
      let after = saved;
      if (changedTheme && draft.theme !== undefined)
        after = await setTheme.mutateAsync(draft.theme);
      if (changedPrefs || changedOverrides) {
        await setPreferences.mutateAsync({
          ...preferencesOf(after),
          ...draft.prefs,
          overrides: changedOverrides ? own : after.settings.overrides,
        });
      }
      setDraft({});
      return true;
    } catch {
      // The error shows below (`setTheme.error` / `setPreferences.error`).
      return false;
    }
  };
  useSettingsDraft(APPEARANCE_DRAFT, {
    dirty,
    canSave: problem === null,
    save,
    discard: () => {
      setDraft({});
    },
  });
  return {
    draft,
    setDraft,
    overrides,
    problem: draft.overrides === undefined ? null : problem,
    busy: setTheme.isPending || setPreferences.isPending,
    failure: setTheme.error ?? setPreferences.error,
  };
}

export function AppearanceSettings({ node }: WidgetProps) {
  const t = useT();
  const titleId = useId();
  const ui = useUiStoreApi();
  const appearance = useAppearance();
  const { draft, setDraft, overrides, problem, busy, failure } = useAppearanceDraft(
    appearance.data,
  );

  let content;
  if (appearance.isPending) {
    content = <p data-part="appearance.loading">{t("appearance-loading")}</p>;
  } else if (appearance.isError) {
    // `unsupported` carries the core's reason (e.g. a core older than M5a).
    const message = errorMessageOf(appearance.error);
    content = (
      <p
        data-part="appearance.unavailable"
        data-state={isUnsupported(appearance.error) ? "unsupported" : "error"}
        role="status"
      >
        {t(message.id, message.args)}
      </p>
    );
  } else {
    const a = appearance.data;
    // What the section shows: the saved settings with the unsaved choices.
    const s = { ...a.settings, ...draft.prefs };
    const update = (change: NonNullable<AppearanceDraft["prefs"]>) => {
      setDraft((d) => ({ ...d, prefs: { ...d.prefs, ...change } }));
    };
    const picked = draft.theme ?? (s.follow_system ? a.active : s.theme);
    const selected = (theme: ThemeInfo) => theme.id === picked;
    content = (
      <>
        <section data-part="card" data-section="themes" aria-labelledby={`${titleId}-themes`}>
          <h3 data-part="card.title" id={`${titleId}-themes`}>
            {t("appearance-themes-title")}
          </h3>
          <Switch
            name="follow-system"
            checked={s.follow_system}
            label={t("appearance-follow-system")}
            hint={t("appearance-follow-system-hint")}
            disabled={busy}
            onChange={(on) => {
              update({ follow_system: on });
            }}
          />
          {s.follow_system && (
            <div data-part="appearance.pair">
              <PairSelect
                name="light"
                label={t("appearance-light-theme")}
                value={s.light}
                themes={a.themes}
                kind="light"
                onChange={(light) => {
                  update({ light });
                }}
              />
              <PairSelect
                name="dark"
                label={t("appearance-dark-theme")}
                value={s.dark}
                themes={a.themes}
                kind="dark"
                onChange={(dark) => {
                  update({ dark });
                }}
              />
            </div>
          )}
          <div
            data-part="theme-gallery"
            role="radiogroup"
            aria-label={t("appearance-gallery-label")}
          >
            {a.themes.map((theme) => (
              <ThemeCard
                key={theme.id}
                theme={theme}
                themes={a.themes}
                selected={selected(theme)}
                active={theme.id === a.active}
                onPick={() => {
                  setDraft((d) => ({ ...d, theme: theme.id }));
                }}
              />
            ))}
          </div>
          {s.follow_system && <p data-part="form.hint">{t("appearance-gallery-hint")}</p>}
          <div data-part="appearance.actions" data-actions="themes">
            <button
              type="button"
              data-part="form.button"
              data-action="new-theme"
              onClick={() => {
                openNewTheme(ui, a.active);
              }}
            >
              <Icon name="plus" part="form.button.icon" />
              {t("appearance-new-theme")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="edit-theme"
              onClick={() => {
                openThemeEditor(ui, a.active);
              }}
            >
              <Icon name="palette" part="form.button.icon" />
              {t("appearance-edit-theme")}
            </button>
          </div>
          <p data-part="form.hint">{t("appearance-editor-hint")}</p>
        </section>
        <section data-part="card" data-section="density" aria-labelledby={`${titleId}-density`}>
          <h3 data-part="card.title" id={`${titleId}-density`}>
            {t("appearance-density-title")}
          </h3>
          <Segmented
            name="density"
            label={t("appearance-density-title")}
            value={s.density}
            disabled={busy}
            options={DENSITY_VALUES.map((d) => ({ value: d, label: t(DENSITY_MESSAGE_IDS[d]) }))}
            onChange={(value) => {
              update({ density: value === "compact" ? "compact" : "comfortable" });
            }}
          />
          <p data-part="form.hint">{t("appearance-density-hint")}</p>
        </section>
        <section data-part="card" data-section="overrides">
          <OverridesForm
            appearance={a}
            active={draft.theme ?? a.active}
            busy={busy}
            draft={overrides}
            problem={problem}
            onDraft={(next) => {
              setDraft((d) => ({ ...d, overrides: next }));
            }}
          />
        </section>
        {failure && (
          <p data-part="form.error" role="alert">
            {t(errorMessageOf(failure).id, errorMessageOf(failure).args)}
          </p>
        )}
      </>
    );
  }

  return (
    <div
      data-part="appearance"
      data-node-id={node.id ?? undefined}
      aria-labelledby={titleId}
      role="region"
    >
      <div data-part="appearance.header">
        <h2 data-part="form.section.title" id={titleId}>
          {t("appearance-title")}
        </h2>
        <HistoryButtons />
      </div>
      {content}
    </div>
  );
}
