/**
 * Settings → Appearance (M5a WP153; widget `appearance-settings`): the theme
 * gallery (built-in and the user's own themes), Follow Windows with its
 * light / dark pair, Comfortable / Compact density, and the text-size /
 * corner-radius / accent overrides. Everything is written by the core into
 * the user's `theme.kdl`; this page only asks for it.
 */
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  Appearance,
  Density,
  LocalizedMessage,
  ThemeInfo,
  ThemeKind,
  ThemeOverrides,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { isUnsupported } from "../../state/queries";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { Segmented, Switch } from "../../widgets/primitives";
import { FONT_SIZE_RANGE, RADIUS_RANGE, isHexColor, overrideIssues } from "./mock";
import { preferencesOf, useAppearance, useSetTheme, useSetThemePreferences } from "./queries";
import { previewColors } from "./themes";

/** The display name of a theme: the built-in's Fluent label, else its own name. */
function useThemeName(): (theme: ThemeInfo) => string {
  const t = useT();
  return (theme) => (theme.label ? t(theme.label) : theme.name || theme.id);
}

/** Colours of a theme's preview (rail, list, accent, text). */
const SWATCH_TOKENS = ["bg-subtle", "bg", "accent", "fg"] as const;

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
  onPick(): void;
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
        {SWATCH_TOKENS.map((token) => {
          const color = colors[token];
          return color ? <Swatch key={token} geom={`${geom}-${token}`} color={color} /> : null;
        })}
      </span>
      <span data-part="theme-card.name">{name}</span>
      <span data-part="theme-card.kind">
        {t(theme.source === "user" ? "appearance-theme-meta-user" : "appearance-theme-meta", {
          kind: t(`theme-kind-${theme.kind}`),
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
  onChange(id: string): void;
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
        onChange={(event) => onChange(event.target.value)}
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

function OverridesForm({
  appearance,
  busy,
  onApply,
}: {
  appearance: Appearance;
  busy: boolean;
  onApply(overrides: ThemeOverrides): void;
}) {
  const t = useT();
  const id = useId();
  const saved = appearance.settings.overrides;
  const [draft, setDraft] = useState<OverrideDraft>(() => draftOf(saved));
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const apply = (overrides: ThemeOverrides | null) => {
    if (overrides === null) {
      setProblem({ id: "appearance-overrides-not-a-number" });
      return;
    }
    const issue = overrideIssues(overrides)[0];
    if (issue) {
      const range = issue === "font-size-range" ? FONT_SIZE_RANGE : RADIUS_RANGE;
      setProblem({
        id: `theme-overrides-issue-${issue}`,
        args: { min: range[0], max: range[1] },
      });
      return;
    }
    setProblem(null);
    onApply(overrides);
  };
  const field = (key: keyof OverrideDraft) => (value: string) =>
    setDraft((d) => ({ ...d, [key]: value }));
  return (
    <form
      data-part="appearance.overrides"
      // Nail's own checks (with the localized texts) instead of the browser's.
      noValidate
      aria-labelledby={`${id}-title`}
      onSubmit={(event) => {
        event.preventDefault();
        apply(overridesOf(draft));
      }}
    >
      <h3 data-part="card.title" id={`${id}-title`}>
        {t("appearance-overrides-title")}
      </h3>
      <p data-part="form.hint">{t("appearance-overrides-hint")}</p>
      <div data-part="form.field" data-field="font-size">
        <label data-part="form.label" htmlFor={`${id}-size`}>
          {t("appearance-font-size", { min: FONT_SIZE_RANGE[0], max: FONT_SIZE_RANGE[1] })}
        </label>
        <input
          id={`${id}-size`}
          data-part="form.input"
          type="number"
          min={FONT_SIZE_RANGE[0]}
          max={FONT_SIZE_RANGE[1]}
          placeholder={t("appearance-override-default")}
          value={draft.fontSize}
          onChange={(event) => field("fontSize")(event.target.value)}
        />
      </div>
      <div data-part="form.field" data-field="radius">
        <label data-part="form.label" htmlFor={`${id}-radius`}>
          {t("appearance-radius", { min: RADIUS_RANGE[0], max: RADIUS_RANGE[1] })}
        </label>
        <input
          id={`${id}-radius`}
          data-part="form.input"
          type="number"
          min={RADIUS_RANGE[0]}
          max={RADIUS_RANGE[1]}
          placeholder={t("appearance-override-default")}
          value={draft.radius}
          onChange={(event) => field("radius")(event.target.value)}
        />
      </div>
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
            onChange={(event) => field("accent")(event.target.value)}
          />
          <input
            data-part="appearance.accent-picker"
            type="color"
            aria-label={t("appearance-accent-pick")}
            value={isHexColor(draft.accent) && draft.accent.length === 7 ? draft.accent : "#3b5bdb"}
            onChange={(event) => field("accent")(event.target.value)}
          />
        </span>
      </div>
      {problem && (
        <p data-part="form.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      <div data-part="appearance.actions">
        <button
          type="button"
          data-part="form.button"
          disabled={busy}
          onClick={() => {
            setDraft(draftOf({}));
            apply({});
          }}
        >
          {t("appearance-overrides-reset")}
        </button>
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("appearance-overrides-apply")}
        </button>
      </div>
    </form>
  );
}

export function AppearanceSettings({ node }: WidgetProps) {
  const t = useT();
  const titleId = useId();
  const appearance = useAppearance();
  const setTheme = useSetTheme();
  const setPreferences = useSetThemePreferences();
  const busy = setTheme.isPending || setPreferences.isPending;
  const failure = setTheme.error ?? setPreferences.error;

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
    const s = a.settings;
    const prefs = preferencesOf(a);
    const update = (change: Partial<typeof prefs>) =>
      setPreferences.mutate({ ...prefs, ...change });
    const selected = (theme: ThemeInfo) =>
      s.follow_system ? theme.id === a.active : theme.id === s.theme;
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
            onChange={(on) => update({ follow_system: on })}
          />
          {s.follow_system && (
            <div data-part="appearance.pair">
              <PairSelect
                name="light"
                label={t("appearance-light-theme")}
                value={s.light}
                themes={a.themes}
                kind="light"
                onChange={(light) => update({ light })}
              />
              <PairSelect
                name="dark"
                label={t("appearance-dark-theme")}
                value={s.dark}
                themes={a.themes}
                kind="dark"
                onChange={(dark) => update({ dark })}
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
                onPick={() => setTheme.mutate(theme.id)}
              />
            ))}
          </div>
          {s.follow_system && <p data-part="form.hint">{t("appearance-gallery-hint")}</p>}
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
            options={(["comfortable", "compact"] as const satisfies readonly Density[]).map(
              (d) => ({ value: d, label: t(`density-${d}`) }),
            )}
            onChange={(value) =>
              update({ density: value === "compact" ? "compact" : "comfortable" })
            }
          />
          <p data-part="form.hint">{t("appearance-density-hint")}</p>
        </section>
        <section data-part="card" data-section="overrides">
          <OverridesForm
            key={JSON.stringify(s.overrides)}
            appearance={a}
            busy={busy}
            onApply={(overrides) => update({ overrides })}
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
      <h2 data-part="form.section.title" id={titleId}>
        {t("appearance-title")}
      </h2>
      {content}
    </div>
  );
}
