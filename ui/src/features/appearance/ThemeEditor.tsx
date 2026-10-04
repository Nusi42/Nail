/**
 * The full theme editor (M5d WP183; widget and window `theme-editor`): a
 * theme's tokens in groups (colour, type, spacing, radius, motion, more),
 * its per-part overrides, and a live preview; "Use this theme", Duplicate,
 * Rename and Delete for user themes; Undo / Redo of the appearance
 * history. Built-in themes are shown read-only with "Duplicate as new
 * theme". Every change is an `EditTheme` patch the core writes into the
 * theme's file with comments kept (`./useThemeEdits.ts`).
 */
import { useId, useRef, useState, type KeyboardEvent } from "react";
import { errorMessageOf, NailError } from "../../api/client";
import type { Appearance, ThemeInfo } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { Tag } from "../../widgets/primitives";
import {
  invalidateThemeDetails,
  useDuplicateTheme,
  useThemeDetails,
  useUseTheme,
} from "./editorQueries";
import {
  THEME_DELETE_DIALOG,
  THEME_RENAME_DIALOG,
  openNewTheme,
  openThemeAction,
  themeEditorStoreFor,
  useThemeEditorState,
} from "./editorStore";
import { HistoryButtons } from "./HistoryButtons";
import { PartEditor } from "./PartEditor";
import { contrastProblems } from "./contrast";
import { preferencesOf, useAppearance, useSetThemePreferences } from "./queries";
import { ThemePreview } from "./ThemePreview";
import { TokenList } from "./TokenList";
import {
  OVERRIDE_TOKENS,
  TOKEN_GROUPS,
  TOKEN_MODES,
  overrideTokens,
  resolveTokenValues,
  withOverrides,
  type OverriddenToken,
  type TokenGroup,
} from "./tokens";
import { useThemeEdits } from "./useThemeEdits";
import { useQueryClient } from "@tanstack/react-query";

/** The editor's tabs: the token groups, then the part overrides. */
const TABS = [...TOKEN_GROUPS, "parts"] as const;
type Tab = TokenGroup | "parts";

/** The display name of a theme: the built-in's Fluent label, else its own name. */
export function useThemeName(): (theme: Pick<ThemeInfo, "id" | "name" | "label">) => string {
  const t = useT();
  return (theme) => (theme.label ? t(theme.label) : theme.name || theme.id);
}

function Tabs({ tab, onTab, idPrefix }: { tab: Tab; onTab: (tab: Tab) => void; idPrefix: string }) {
  const t = useT();
  const refs = useRef(new Map<Tab, HTMLButtonElement>());
  const onKeyDown = (event: KeyboardEvent) => {
    const index = TABS.indexOf(tab);
    const delta =
      event.key === "ArrowRight" || event.key === "ArrowDown"
        ? 1
        : event.key === "ArrowLeft" || event.key === "ArrowUp"
          ? -1
          : 0;
    let next: Tab | undefined;
    if (delta !== 0) next = TABS[(index + delta + TABS.length) % TABS.length];
    else if (event.key === "Home") next = TABS[0];
    else if (event.key === "End") next = TABS[TABS.length - 1];
    if (next === undefined) return;
    event.preventDefault();
    onTab(next);
    refs.current.get(next)?.focus();
  };
  return (
    // eslint-disable-next-line jsx-a11y/interactive-supports-focus -- the tabs take the focus (roving tabindex), not the tab list itself.
    <div
      data-part="tabs.tab-list"
      role="tablist"
      aria-label={t("theme-editor-sections")}
      aria-orientation="vertical"
      onKeyDown={onKeyDown}
    >
      {TABS.map((id) => (
        <button
          key={id}
          ref={(el) => {
            if (el) refs.current.set(id, el);
            else refs.current.delete(id);
          }}
          type="button"
          role="tab"
          id={`${idPrefix}-tab-${id}`}
          aria-selected={id === tab}
          aria-controls={`${idPrefix}-panel`}
          tabIndex={id === tab ? 0 : -1}
          data-part="tabs.tab"
          data-tab={id}
          data-state={id === tab ? "selected" : undefined}
          onClick={() => {
            onTab(id);
          }}
        >
          {t(id === "parts" ? "theme-editor-parts" : `theme-editor-group-${id}`)}
        </button>
      ))}
    </div>
  );
}

/** The body for one theme (remounted per theme, so drafts never leak across). */
function EditorBody({
  theme,
  active,
  focus,
  appearance,
}: {
  theme: ThemeInfo;
  active: boolean;
  /** The user's appearance (its overrides win over every theme). */
  appearance: Appearance;
  /** The part "Edit style" asked for, else `null`. */
  focus: string | null;
}) {
  const t = useT();
  const idPrefix = useId();
  const ui = useUiStoreApi();
  const queryClient = useQueryClient();
  const nameOf = useThemeName();
  const details = useThemeDetails(theme.id);
  const edits = useThemeEdits(theme.id);
  const pick = useUseTheme();
  const duplicate = useDuplicateTheme();
  const setPreferences = useSetThemePreferences();
  const [tab, setTab] = useState<Tab>(focus === null ? "color" : "parts");
  const [mode, setMode] = useState<string | null>(null);
  const name = nameOf(theme);

  if (details.isPending) {
    return <p data-part="theme-editor.loading">{t("theme-editor-loading")}</p>;
  }
  if (details.isError) {
    const message = errorMessageOf(details.error);
    return (
      <p data-part="theme-editor.unavailable" role="status">
        {t(message.id, message.args)}
      </p>
    );
  }
  const d = details.data;
  const editable = d.editable;
  const failure = edits.failure;
  const conflict = failure instanceof NailError && failure.code === "conflict";
  const busy = pick.isPending || duplicate.isPending;
  // The Appearance overrides win over the theme: the preview and the
  // contrast check show what the app will really wear (ADR 0138).
  const overrides = overrideTokens(appearance.settings.overrides);
  const shownDrafts = withOverrides(edits.drafts, overrides, mode);
  const problem = contrastProblems(resolveTokenValues(d.tokens, mode, shownDrafts))[0];
  const clearOverride = (token: string) => {
    if (!Object.hasOwn(OVERRIDE_TOKENS, token)) return;
    const field = OVERRIDE_TOKENS[token as OverriddenToken];
    const next = Object.fromEntries(
      Object.entries(appearance.settings.overrides).filter(([key]) => key !== field),
    );
    setPreferences.mutate({ ...preferencesOf(appearance), overrides: next });
  };

  const duplicateAsNew = () => {
    duplicate.mutate(
      { themeId: theme.id, name: t("theme-editor-copy-name", { name }) },
      {
        onSuccess: (newId) => {
          // The copy replaces the original where it was in use.
          if (active) pick.mutate(newId);
          themeEditorStoreFor(ui).getState().show(newId, focus);
        },
      },
    );
  };

  return (
    <>
      <div data-part="theme-editor.actions">
        {active ? (
          <Tag text={t("theme-editor-in-use")} icon="check" kind="in-use" />
        ) : (
          <button
            type="button"
            data-part="form.button"
            disabled={busy}
            onClick={() => {
              pick.mutate(theme.id);
            }}
          >
            {t("theme-editor-use")}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          data-variant={editable ? undefined : "primary"}
          disabled={busy}
          onClick={duplicateAsNew}
        >
          <Icon name="copy" part="form.button.icon" />
          {t("theme-editor-duplicate")}
        </button>
        {editable && (
          <>
            <button
              type="button"
              data-part="form.button"
              onClick={() => {
                openThemeAction(ui, THEME_RENAME_DIALOG, theme.id);
              }}
            >
              <Icon name="pencil" part="form.button.icon" />
              {t("theme-editor-rename")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="danger"
              onClick={() => {
                openThemeAction(ui, THEME_DELETE_DIALOG, theme.id);
              }}
            >
              <Icon name="trash" part="form.button.icon" />
              {t("theme-editor-delete")}
            </button>
          </>
        )}
        <span data-part="theme-editor.status" role="status">
          {edits.saving ? t("theme-editor-saving") : ""}
        </span>
      </div>
      {!editable && (
        <p data-part="theme-editor.notice" data-tone="info">
          {t("theme-editor-builtin-notice")}
        </p>
      )}
      {problem && (
        <p data-part="theme-editor.notice" data-tone="warning" role="status">
          {t("theme-editor-contrast-warning", {
            fg: problem.fg,
            bg: problem.bg,
            ratio: problem.ratio.toFixed(1),
            needed: problem.needed.toFixed(1),
          })}
        </p>
      )}
      {setPreferences.error && (
        <p data-part="form.error" role="alert">
          {t(errorMessageOf(setPreferences.error).id, errorMessageOf(setPreferences.error).args)}
        </p>
      )}
      {duplicate.error && (
        <p data-part="form.error" role="alert">
          {t(errorMessageOf(duplicate.error).id, errorMessageOf(duplicate.error).args)}
        </p>
      )}
      {failure !== null && failure !== undefined && (
        <p data-part="form.error" role="alert">
          {t(errorMessageOf(failure).id, errorMessageOf(failure).args)}
          {conflict && (
            <button
              type="button"
              data-part="form.button"
              onClick={() => {
                edits.clearFailure();
                void invalidateThemeDetails(queryClient);
              }}
            >
              {t("theme-editor-reload")}
            </button>
          )}
        </p>
      )}
      <div data-part="theme-editor.body">
        <Tabs tab={tab} onTab={setTab} idPrefix={idPrefix} />
        <div
          data-part="theme-editor.panel"
          role="tabpanel"
          id={`${idPrefix}-panel`}
          aria-labelledby={`${idPrefix}-tab-${tab}`}
        >
          {tab === "parts" ? (
            <PartEditor parts={d.parts} editable={editable} edits={edits} focus={focus} />
          ) : (
            <>
              <div data-part="form.field" data-field="mode">
                <label data-part="form.label" htmlFor={`${idPrefix}-mode`}>
                  {t("theme-editor-mode")}
                </label>
                <select
                  id={`${idPrefix}-mode`}
                  data-part="form.select"
                  value={mode ?? ""}
                  onChange={(event) => {
                    setMode(event.target.value === "" ? null : event.target.value);
                  }}
                >
                  <option value="">{t("theme-editor-mode-base")}</option>
                  {TOKEN_MODES.map((m) => (
                    <option key={m} value={m}>
                      {t(`theme-editor-mode-${m}`)}
                    </option>
                  ))}
                </select>
              </div>
              <TokenList
                key={tab}
                tokens={d.tokens}
                group={tab}
                mode={mode}
                editable={editable}
                edits={edits}
                overrides={overrides}
                onClearOverride={clearOverride}
              />
            </>
          )}
        </div>
        <ThemePreview tokens={d.tokens} mode={mode} drafts={shownDrafts} name={name} />
      </div>
    </>
  );
}

/** The editor: theme chooser, history, and the body of the chosen theme. */
export function ThemeEditorView() {
  const t = useT();
  const id = useId();
  const ui = useUiStoreApi();
  const nameOf = useThemeName();
  const appearance = useAppearance();
  const themeId = useThemeEditorState((s) => s.themeId);
  const part = useThemeEditorState((s) => s.part);
  const jump = useThemeEditorState((s) => s.jump);

  if (appearance.isPending) {
    return <p data-part="theme-editor.loading">{t("theme-editor-loading")}</p>;
  }
  if (appearance.isError) {
    const message = errorMessageOf(appearance.error);
    return (
      <p data-part="theme-editor.unavailable" role="status">
        {t(message.id, message.args)}
      </p>
    );
  }
  const a = appearance.data;
  const theme =
    a.themes.find((th) => th.id === (themeId ?? a.active)) ??
    a.themes.find((th) => th.id === a.active);
  return (
    <div data-part="theme-editor" aria-labelledby={`${id}-title`} role="region">
      <div data-part="theme-editor.header">
        <h2 data-part="form.section.title" id={`${id}-title`}>
          {t("theme-editor-title")}
        </h2>
        <div data-part="form.field" data-field="theme">
          <label data-part="form.label" htmlFor={`${id}-theme`}>
            {t("theme-editor-theme")}
          </label>
          <select
            id={`${id}-theme`}
            data-part="form.select"
            value={theme?.id ?? ""}
            onChange={(event) => {
              themeEditorStoreFor(ui).getState().show(event.target.value);
            }}
          >
            {a.themes.map((th) => (
              <option key={th.id} value={th.id}>
                {th.source === "user"
                  ? t("theme-editor-theme-user", { name: nameOf(th) })
                  : nameOf(th)}
              </option>
            ))}
          </select>
        </div>
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            openNewTheme(ui, theme?.id ?? null);
          }}
        >
          <Icon name="plus" part="form.button.icon" />
          {t("appearance-new-theme")}
        </button>
        <HistoryButtons />
      </div>
      {theme ? (
        <EditorBody
          key={`${theme.id}:${String(jump)}`}
          theme={theme}
          active={theme.id === a.active}
          focus={part}
          appearance={a}
        />
      ) : (
        <p data-part="theme-editor.empty">{t("theme-editor-no-theme")}</p>
      )}
    </div>
  );
}

/** Layout widget `theme-editor`. */
export function ThemeEditorWidget({ node }: WidgetProps) {
  return (
    <div data-part="theme-editor.host" data-node-id={node.id ?? undefined}>
      <ThemeEditorView />
    </div>
  );
}

/** Window `theme-editor`. */
export function ThemeEditorDialog() {
  return <ThemeEditorView />;
}
