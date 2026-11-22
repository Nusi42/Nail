/**
 * Rate limits of one MCP client (M4a, ADR 0079): calls per minute, write
 * tool calls per minute and messages sent per hour, each either a number
 * (1–10 000) or "no limit". Calls over a limit fail and are audited as
 * denied.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, McpClient, McpRateLimits } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { CheckField, NumberField } from "../../widgets/forms";
import { useSettingsForm } from "../settings/draft";
import { DEFAULT_RATE_LIMITS, MAX_RATE_LIMIT, mcpKeys, rateLimitIssues } from "./queries";

/** The limits in display order. */
export const RATE_LIMIT_KEYS = [
  "requests_per_minute",
  "writes_per_minute",
  "sends_per_hour",
] as const;
type Key = (typeof RATE_LIMIT_KEYS)[number];

const FIELD_ID = {
  requests_per_minute: "mcp-rate-requests",
  writes_per_minute: "mcp-rate-writes",
  sends_per_hour: "mcp-rate-sends",
} as const satisfies Record<Key, string>;

function LimitField({
  name,
  value,
  disabled,
  onChange,
}: {
  name: Key;
  value: number | null;
  disabled: boolean;
  onChange: (value: number | null) => void;
}) {
  const unlimited = value === null;
  return (
    <div
      data-part="mcp-rate-limits.limit"
      data-limit={name}
      data-state={unlimited ? "unlimited" : "limited"}
    >
      {!unlimited && (
        <NumberField
          labelId={FIELD_ID[name]}
          hintId={`${FIELD_ID[name]}-hint`}
          name={name}
          value={value}
          min={1}
          max={MAX_RATE_LIMIT}
          onChange={(n) => {
            onChange(n ?? 0);
          }}
        />
      )}
      <CheckField
        labelId={`${FIELD_ID[name]}-unlimited`}
        name={`${name}-unlimited`}
        checked={unlimited}
        onChange={(on) => {
          if (!disabled) onChange(on ? null : DEFAULT_RATE_LIMITS[name]);
        }}
      />
    </div>
  );
}

/** The rate limit editor of `client`; R6 (ADR 0239): the edits wait for the settings page's Save. */
export function RateLimitsEditor({ client }: { client: McpClient }) {
  const t = useT();
  const nail = useClient();
  const queryClient = useQueryClient();
  const [notice, setNotice] = useState<{ message: LocalizedMessage; error: boolean } | null>(null);
  const form = useSettingsForm<McpRateLimits>(client.rate_limits, {
    canSave: (limits) => rateLimitIssues(limits).length === 0,
    save: async (limits) => {
      await nail.request(
        { type: "UpdateMcpRateLimits", client_id: client.id, limits },
        "McpClientSaved",
      );
      await queryClient.invalidateQueries({ queryKey: mcpKeys.clients });
    },
    onSaved: () => {
      setNotice({ message: { id: "mcp-rate-saved" }, error: false });
    },
    onError: (e) => {
      setNotice({ message: errorMessageOf(e), error: true });
    },
  });
  const limits = form.value;
  const revoked = client.revoked_at !== null;
  const disabled = revoked;
  const issues = rateLimitIssues(limits);

  return (
    <form
      data-part="mcp-rate-limits"
      aria-label={t("mcp-rate-label", { name: client.name })}
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
      }}
    >
      <fieldset data-part="mcp-rate-limits.fields" disabled={disabled}>
        <legend data-part="mcp-rate-limits.title">{t("mcp-rate-title")}</legend>
        <p data-part="mcp-rate-limits.intro">{t("mcp-rate-intro")}</p>
        {RATE_LIMIT_KEYS.map((key) => (
          <LimitField
            key={key}
            name={key}
            value={limits[key]}
            disabled={disabled}
            onChange={(value) => {
              form.set((current) => ({ ...current, [key]: value }));
              setNotice(null);
            }}
          />
        ))}
      </fieldset>
      {issues.map((issue) => (
        <p key={issue} data-part="mcp-rate-limits.issue" role="alert">
          {t(`mcp-rate-limit-issue-${issue}`)}
        </p>
      ))}
      <div data-part="mcp-rate-limits.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="reset-rate-limits"
          disabled={disabled || JSON.stringify(limits) === JSON.stringify(DEFAULT_RATE_LIMITS)}
          onClick={() => {
            form.set({ ...DEFAULT_RATE_LIMITS });
            setNotice(null);
          }}
        >
          {t("mcp-rate-defaults")}
        </button>
      </div>
      {notice && (
        <div
          data-part="mcp-rate-limits.message"
          data-state={notice.error ? "error" : "ok"}
          role={notice.error ? "alert" : "status"}
        >
          {t(notice.message.id, notice.message.args)}
        </div>
      )}
    </form>
  );
}
