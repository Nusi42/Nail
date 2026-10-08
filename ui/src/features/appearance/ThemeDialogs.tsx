/**
 * The theme windows besides the editor (M5d WP183): "New theme" (a name and
 * the theme to start from → `DuplicateTheme`, optionally used at once, then
 * opened in the editor), "Rename theme" (`RenameTheme`, optionally with a
 * new file name) and the confirmation "Delete theme?" (`DeleteTheme`; a
 * selection naming the theme falls back to the default of its kind).
 */
import { useEffect, useId, useRef, useState, type SubmitEvent } from "react";
import { errorMessageOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { useDeleteTheme, useDuplicateTheme, useRenameTheme, useUseTheme } from "./editorQueries";
import { openThemeEditor, themeEditorStoreFor, useThemeEditorState } from "./editorStore";
import { THEME_NAME_MAX, isThemeName, themeIdFromName } from "./model";
import { useAppearance } from "./queries";
import { useThemeName } from "./ThemeEditor";
import { CloseButton } from "../../widgets/CloseButton";

function NameField({
  id,
  value,
  onChange,
  invalid,
}: {
  id: string;
  value: string;
  onChange: (value: string) => void;
  invalid: boolean;
}) {
  const t = useT();
  return (
    <div data-part="form.field" data-field="name">
      <label data-part="form.label" htmlFor={id}>
        {t("common-name")}
      </label>
      <input
        id={id}
        type="text"
        data-part="form.input"
        maxLength={THEME_NAME_MAX}
        aria-invalid={invalid || undefined}
        aria-describedby={`${id}-hint`}
        value={value}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
      <span data-part="form.hint" id={`${id}-hint`}>
        {t("theme-dialog-file", { file: `themes/${themeIdFromName(value)}.kdl` })}
      </span>
    </div>
  );
}

/** Window `theme-new`. */
export function NewThemeDialog({ close }: DialogProps) {
  const t = useT();
  const id = useId();
  const ui = useUiStoreApi();
  const nameOf = useThemeName();
  const appearance = useAppearance();
  const requested = useThemeEditorState((s) => s.newBase);
  const duplicate = useDuplicateTheme();
  const pick = useUseTheme();
  const [name, setName] = useState("");
  const [base, setBase] = useState<string | null>(null);
  const [use, setUse] = useState(true);
  const [invalid, setInvalid] = useState(false);
  const themes = (appearance.data?.themes ?? []).filter((th) => th.problem === undefined);
  const from = base ?? requested ?? appearance.data?.active ?? "";
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!isThemeName(name)) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    duplicate.mutate(
      { themeId: from, name: name.trim() },
      {
        onSuccess: (newId) => {
          if (use) pick.mutate(newId);
          close();
          openThemeEditor(ui, newId);
        },
      },
    );
  };
  const failure = duplicate.error;
  return (
    <form data-part="theme-dialog" data-dialog="new" noValidate onSubmit={submit}>
      <p data-part="form.hint">{t("theme-dialog-new-hint")}</p>
      <NameField id={`${id}-name`} value={name} onChange={setName} invalid={invalid} />
      <div data-part="form.field" data-field="base">
        <label data-part="form.label" htmlFor={`${id}-base`}>
          {t("theme-dialog-base")}
        </label>
        <select
          id={`${id}-base`}
          data-part="form.select"
          value={from}
          onChange={(event) => {
            setBase(event.target.value);
          }}
        >
          {themes.map((th) => (
            <option key={th.id} value={th.id}>
              {nameOf(th)}
            </option>
          ))}
        </select>
      </div>
      <label data-part="form.check" data-field="use">
        <input
          type="checkbox"
          checked={use}
          onChange={(event) => {
            setUse(event.target.checked);
          }}
        />
        <span data-part="form.check.label">{t("theme-dialog-use")}</span>
      </label>
      <NameDialogFooter
        invalid={invalid}
        failure={failure}
        close={close}
        submitLabel={t("common-create")}
        disabled={duplicate.isPending || from === ""}
      />
    </form>
  );
}

/** Window `theme-rename`. */
export function RenameThemeDialog({ close }: DialogProps) {
  const t = useT();
  const id = useId();
  const ui = useUiStoreApi();
  const appearance = useAppearance();
  const subject = useThemeEditorState((s) => s.subject);
  const theme = appearance.data?.themes.find((th) => th.id === subject);
  const rename = useRenameTheme();
  const [name, setName] = useState<string | null>(null);
  const [moveFile, setMoveFile] = useState(false);
  const [invalid, setInvalid] = useState(false);
  const shown = name ?? theme?.name ?? "";
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (!theme) return;
    if (!isThemeName(shown)) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    const newId = moveFile ? themeIdFromName(shown) : undefined;
    rename.mutate(
      {
        themeId: theme.id,
        name: shown.trim(),
        ...(newId === undefined || newId === theme.id ? {} : { newId }),
      },
      {
        onSuccess: (savedId) => {
          const editor = themeEditorStoreFor(ui).getState();
          if (editor.themeId === theme.id) editor.show(savedId);
          close();
        },
      },
    );
  };
  const failure = rename.error;
  if (!theme) {
    return (
      <div data-part="theme-dialog" data-dialog="rename">
        <p data-part="theme-dialog.gone">{t("edit-issue-theme-not-found")}</p>
        <div data-part="theme-dialog.actions">
          <CloseButton onClose={close} />
        </div>
      </div>
    );
  }
  return (
    <form data-part="theme-dialog" data-dialog="rename" noValidate onSubmit={submit}>
      <NameField id={`${id}-name`} value={shown} onChange={setName} invalid={invalid} />
      <label data-part="form.check" data-field="rename-file">
        <input
          type="checkbox"
          checked={moveFile}
          onChange={(event) => {
            setMoveFile(event.target.checked);
          }}
        />
        <span data-part="form.check.label">
          {t("theme-dialog-rename-file", { file: theme.file })}
        </span>
      </label>
      <NameDialogFooter
        invalid={invalid}
        failure={failure}
        close={close}
        submitLabel={t("common-rename")}
        disabled={rename.isPending}
      />
    </form>
  );
}

/** Window `theme-delete` (a confirmation). */
export function DeleteThemeDialog({ close }: DialogProps) {
  const t = useT();
  const ui = useUiStoreApi();
  const nameOf = useThemeName();
  const appearance = useAppearance();
  const subject = useThemeEditorState((s) => s.subject);
  const theme = appearance.data?.themes.find((th) => th.id === subject);
  const remove = useDeleteTheme();
  const cancel = useRef<HTMLButtonElement>(null);
  // The safe choice has the focus (after the frame focused its first button).
  useEffect(() => {
    queueMicrotask(() => cancel.current?.focus());
  }, []);
  const failure = remove.error;
  return (
    <div data-part="theme-dialog" data-dialog="delete" aria-busy={remove.isPending}>
      <p data-part="theme-dialog.question">
        {theme
          ? t("theme-dialog-delete-question", { name: nameOf(theme) })
          : t("edit-issue-theme-not-found")}
      </p>
      {theme && (
        <p data-part="theme-dialog.note">{t("theme-dialog-delete-note", { file: theme.file })}</p>
      )}
      {failure && (
        <p data-part="form.error" role="alert">
          {t(errorMessageOf(failure).id, errorMessageOf(failure).args)}
        </p>
      )}
      <div data-part="theme-dialog.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          disabled={!theme || remove.isPending}
          onClick={() => {
            if (!theme) return;
            remove.mutate(theme.id, {
              onSuccess: () => {
                const editor = themeEditorStoreFor(ui).getState();
                if (editor.themeId === theme.id) editor.show(null);
                close();
              },
            });
          }}
        >
          {t("common-delete")}
        </button>
        <button ref={cancel} type="button" data-part="form.button" onClick={close}>
          {t("common-cancel")}
        </button>
      </div>
    </div>
  );
}

/** The name problem (or the core's refusal) and Cancel / submit of a theme dialog. */
function NameDialogFooter({
  invalid,
  failure,
  close,
  submitLabel,
  disabled,
}: {
  invalid: boolean;
  failure: unknown;
  close: () => void;
  submitLabel: string;
  disabled: boolean;
}) {
  const t = useT();
  const refusal = failure ? errorMessageOf(failure) : null;
  return (
    <>
      {(invalid || refusal) && (
        <p data-part="form.error" role="alert">
          {invalid || refusal === null
            ? t("edit-issue-invalid-theme-name")
            : t(refusal.id, refusal.args)}
        </p>
      )}
      <div data-part="theme-dialog.actions">
        <button type="button" data-part="form.button" onClick={close}>
          {t("common-cancel")}
        </button>
        <button type="submit" data-part="form.button" data-variant="primary" disabled={disabled}>
          {submitLabel}
        </button>
      </div>
    </>
  );
}
