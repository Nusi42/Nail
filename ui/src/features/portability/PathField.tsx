/**
 * A file or folder path: typed, or chosen with the system dialog
 * (`PickPath` through the desktop host). Without a host (`nail serve`, the
 * remote UI) the core answers `unsupported`; the Browse button then goes
 * away and the field says to type the full path. Shared by the wizards,
 * the PDF export and the recovery kit.
 */
import { useId, useState } from "react";
import { NailError } from "../../api/client";
import type { PathFilter, PathPickKind } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { isAbsolutePath } from "./model";
import { portabilityStoreFor, usePortabilityState } from "./store";

export interface PathFieldProps {
  /** Fluent id of the label. */
  labelId: string;
  value: string;
  onChange(path: string): void;
  /** Which system dialog Browse opens. */
  kind: PathPickKind;
  /** Fluent id of the dialog's title. */
  titleId: string;
  filters?: PathFilter[];
  /** Suggested file name (save) or start folder. */
  defaultPath?: string | null;
  /** `data-field` of the field (tests, themes). */
  field: string;
  disabled?: boolean;
}

export function PathField({
  labelId,
  value,
  onChange,
  kind,
  titleId,
  filters = [],
  defaultPath = null,
  field,
  disabled = false,
}: PathFieldProps) {
  const t = useT();
  const client = useClient();
  const ui = useUiStoreApi();
  const unavailable = usePortabilityState((s) => s.pickerUnavailable);
  const [busy, setBusy] = useState(false);
  const id = useId();
  const hintId = `${id}-hint`;
  const relative = value.trim() !== "" && !isAbsolutePath(value);

  const browse = async () => {
    setBusy(true);
    try {
      const answer = await client.request(
        { type: "PickPath", pick: { kind, title_id: titleId, filters, default_path: defaultPath } },
        "PathPicked",
      );
      if (answer.path !== null) onChange(answer.path);
    } catch (error) {
      if (error instanceof NailError && error.code === "unsupported") {
        portabilityStoreFor(ui).getState().setPickerUnavailable();
      }
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-part="form.field" data-field={field}>
      <label data-part="form.label" htmlFor={id}>
        {t(labelId)}
      </label>
      <div data-part="path-field">
        <input
          id={id}
          data-part="form.input"
          data-field={field}
          type="text"
          spellCheck={false}
          autoComplete="off"
          value={value}
          disabled={disabled}
          aria-describedby={hintId}
          aria-invalid={relative || undefined}
          onChange={(e) => onChange(e.target.value)}
        />
        {!unavailable && (
          <button
            type="button"
            data-part="form.button"
            data-action="browse"
            disabled={disabled || busy}
            onClick={() => void browse()}
          >
            {t(kind === "open-folder" ? "portability-browse-folder" : "portability-browse")}
          </button>
        )}
      </div>
      <p data-part="form.hint" id={hintId} data-tone={relative ? "danger" : undefined}>
        {t(
          relative
            ? "portability-path-not-absolute"
            : unavailable
              ? "portability-path-type-hint"
              : "portability-path-hint",
        )}
      </p>
    </div>
  );
}
