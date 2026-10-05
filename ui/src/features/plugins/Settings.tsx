/**
 * A plug-in's settings page, generated from its manifest's settings schema
 * (`GetPluginSettings` / `UpdatePluginSettings`; the core checks values).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useState, type ReactNode } from "react";
import { errorMessageOf } from "../../api/client";
import type { JsonValue, LocalizedMessage, PluginSettingDef } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { pluginKeys, usePluginSettings } from "./queries";

function SettingField({
  def,
  value,
  onChange,
}: {
  def: PluginSettingDef;
  value: JsonValue | undefined;
  onChange: (value: JsonValue) => void;
}) {
  const id = useId();
  const hint = def.description === undefined ? undefined : `${id}-hint`;
  let control: ReactNode;
  switch (def.type) {
    case "string":
      control = def.multiline ? (
        <textarea
          id={id}
          data-part="form.textarea"
          aria-describedby={hint}
          maxLength={def.maxLength}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      ) : (
        <input
          id={id}
          data-part="form.input"
          type="text"
          aria-describedby={hint}
          maxLength={def.maxLength}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        />
      );
      break;
    case "number":
      control = (
        <input
          id={id}
          data-part="form.input"
          data-width="short"
          type="number"
          step={1}
          min={def.min}
          max={def.max}
          aria-describedby={hint}
          value={typeof value === "number" ? String(value) : ""}
          onChange={(event) => {
            onChange(event.target.value === "" ? null : Number(event.target.value));
          }}
        />
      );
      break;
    case "boolean":
      control = (
        <input
          id={id}
          data-part="form.checkbox"
          type="checkbox"
          aria-describedby={hint}
          checked={value === true}
          onChange={(event) => {
            onChange(event.target.checked);
          }}
        />
      );
      break;
    case "select":
      control = (
        <select
          id={id}
          data-part="form.select"
          aria-describedby={hint}
          value={typeof value === "string" ? value : ""}
          onChange={(event) => {
            onChange(event.target.value);
          }}
        >
          {def.options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      );
      break;
  }
  return (
    <div data-part="form.field" data-field={def.key}>
      <label data-part="form.label" htmlFor={id}>
        {def.title}
      </label>
      {control}
      {def.description !== undefined && (
        <span data-part="form.hint" id={hint}>
          {def.description}
        </span>
      )}
    </div>
  );
}

export function Settings({ pluginId }: { pluginId: string }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const settings = usePluginSettings(pluginId);
  const [changed, setChanged] = useState<Record<string, JsonValue>>({});
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [saved, setSaved] = useState(false);
  const [busy, setBusy] = useState(false);
  if (!settings.data) return null;
  const { schema, values } = settings.data;
  if (schema.length === 0) {
    return <p data-part="plugin-settings.none">{t("plugin-settings-none")}</p>;
  }
  const save = async () => {
    setBusy(true);
    setProblem(null);
    setSaved(false);
    try {
      const r = await client.request(
        { type: "UpdatePluginSettings", plugin_id: pluginId, values: changed },
        "PluginSettings",
      );
      queryClient.setQueryData(pluginKeys.settings(pluginId), r);
      setChanged({});
      setSaved(true);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <form
      data-part="plugin-settings"
      noValidate
      aria-label={t("plugin-settings-label")}
      onSubmit={(event) => {
        event.preventDefault();
        void save();
      }}
    >
      {problem && (
        <p data-part="plugin-settings.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
      {saved && (
        <p data-part="plugin-settings.status" role="status">
          {t("plugin-settings-saved")}
        </p>
      )}
      {schema.map((def) => (
        <SettingField
          key={def.key}
          def={def}
          value={Object.hasOwn(changed, def.key) ? changed[def.key] : values[def.key]}
          onChange={(value) => {
            setSaved(false);
            setChanged((c) => ({ ...c, [def.key]: value }));
          }}
        />
      ))}
      <div data-part="plugin-settings.actions">
        <button
          type="submit"
          data-part="form.button"
          data-action="save"
          disabled={busy || Object.keys(changed).length === 0}
        >
          {t("plugin-settings-save")}
        </button>
      </div>
    </form>
  );
}
