/**
 * Rate limits of one MCP client (M4a, ADR 0079): calls per minute, write
 * tool calls per minute and messages sent per hour, each either a number
 * (1–10 000) or "no limit". Calls over a limit fail and are audited as
 * denied.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, McpClient, McpRateLimits } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { CheckField, NumberField } from "../../widgets/forms";
import { DEFAULT_RATE_LIMITS, mcpKeys, rateLimitIssues } from "./queries";

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
  onChange(value: number | null): void;
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
          max={10_000}
          onChange={(n) => onChange(n ?? 0)}
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

/** The rate limit editor of `client`. */
export function RateLimitsEditor({ client }: { client: McpClient }) {
  const t = useT();
  const nail = useClient();
  const queryClient = useQueryClient();
  const [limits, setLimits] = useState<McpRateLimits>(client.rate_limits);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ message: LocalizedMessage; error: boolean } | null>(null);
  const revoked = client.revoked_at !== null;
  const disabled = revoked || busy;
  const issues = rateLimitIssues(limits);
  const dirty = JSON.stringify(limits) !== JSON.stringify(client.rate_limits);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (disabled || issues.length > 0) return;
    setBusy(true);
    setNotice(null);
    try {
      await nail.request(
        { type: "UpdateMcpRateLimits", client_id: client.id, limits },
        "McpClientSaved",
      );
      await queryClient.invalidateQueries({ queryKey: mcpKeys.clients });
      setNotice({ message: { id: "mcp-rate-saved" }, error: false });
    } catch (e) {
      setNotice({ message: errorMessageOf(e), error: true });
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      data-part="mcp-rate-limits"
      aria-label={t("mcp-rate-label", { name: client.name })}
      noValidate
      onSubmit={(e) => void save(e)}
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
              setLimits({ ...limits, [key]: value });
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
          type="submit"
          data-part="form.button"
          data-action="save-rate-limits"
          data-variant="primary"
          disabled={disabled || !dirty || issues.length > 0}
        >
          {t("mcp-rate-save")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="reset-rate-limits"
          disabled={disabled || JSON.stringify(limits) === JSON.stringify(DEFAULT_RATE_LIMITS)}
          onClick={() => setLimits({ ...DEFAULT_RATE_LIMITS })}
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
