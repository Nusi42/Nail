/**
 * A plug-in's settings page, generated from its manifest's settings schema
 * (`GetPluginSettings` / `UpdatePluginSettings`; the core checks values).
 * Nothing is written before the settings page's Save (R6).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, type ReactNode } from "react";
import type { JsonValue, PluginSettingDef } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { sameJson } from "../../text/json";
import { useFormNotice, useSettingsForm } from "../settings/draft";
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

/** No values yet (the settings are loading). */
const NO_VALUES: Readonly<Record<string, JsonValue>> = {};

/**
 * The settings of plug-in `pluginId` (R6, ADR 0239): a settings draft,
 * written with the section's Save (`UpdatePluginSettings`, only the changed
 * keys); the core's refusal shows above the fields and the bar stays.
 */
export function Settings({ pluginId }: { pluginId: string }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const settings = usePluginSettings(pluginId);
  const values = settings.data?.values ?? NO_VALUES;
  const { notice, outcome, clear, problems } = useFormNotice();
  const form = useSettingsForm<Record<string, JsonValue>>(values, {
    save: async (edited) => {
      const changed = Object.fromEntries(
        Object.entries(edited).filter(([key, value]) => !sameJson(value, values[key])),
      );
      const r = await client.request(
        { type: "UpdatePluginSettings", plugin_id: pluginId, values: changed },
        "PluginSettings",
      );
      queryClient.setQueryData(pluginKeys.settings(pluginId), r);
    },
    ...outcome("common-settings-saved"),
  });
  if (!settings.data) return null;
  const { schema } = settings.data;
  if (schema.length === 0) {
    return <p data-part="plugin-settings.none">{t("plugin-settings-none")}</p>;
  }
  const failure = problems([])[0];
  return (
    <div data-part="plugin-settings" role="group" aria-label={t("plugin-settings-label")}>
      {failure && (
        <p data-part="plugin-settings.error" role="alert">
          {t(failure.id, failure.args)}
        </p>
      )}
      {notice !== null && !form.dirty && (
        <p data-part="plugin-settings.status" role="status">
          {t(notice)}
        </p>
      )}
      {schema.map((def) => (
        <SettingField
          key={def.key}
          def={def}
          value={form.value[def.key]}
          onChange={(value) => {
            clear();
            form.set((current) => ({ ...current, [def.key]: value }));
          }}
        />
      ))}
    </div>
  );
}
