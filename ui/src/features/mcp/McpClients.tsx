/**
 * Settings → MCP clients (layout widget `mcp-clients`): register agents,
 * show each new token once, edit grants (reading and writing scopes), rate
 * limits and the confirmation policy (M4a), revoke, read the audit log.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { McpClient, McpClientId } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { CheckField, TextField } from "../../widgets/forms";
import { useUi } from "../../state/uiStore";
import { CENTER_DIALOG } from "../agents/names";
import { PolicyEditor } from "../agents/PolicyEditor";
import { pendingOf, usePendingConfirmations } from "../agents/queries";
import { AuditLog, formatAuditTime } from "./AuditLog";
import { RateLimitsEditor } from "./RateLimits";
import { GrantEditor } from "./GrantEditor";
import { emptyGrant, grantIsEmpty, mcpKeys, useMcpClients, useMcpServerInfo } from "./queries";

/** State of a client for the list (`data-state`). */
export function clientState(client: McpClient): "revoked" | "no-access" | "granted" {
  if (client.revoked_at !== null) return "revoked";
  return grantIsEmpty(client.grant) ? "no-access" : "granted";
}

/** The token of a client just created: shown once, then gone. */
function TokenPanel({ name, token, onDone }: { name: string; token: string; onDone: () => void }) {
  const t = useT();
  const [copied, setCopied] = useState<boolean | null>(null);
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(token);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };
  return (
    <section data-part="mcp-token" role="region" aria-label={t("mcp-token-title", { name })}>
      <h3 data-part="mcp-token.title">{t("mcp-token-title", { name })}</h3>
      <p data-part="mcp-token.warning" role="alert">
        {t("mcp-token-once")}
      </p>
      <code data-part="mcp-token.value">{token}</code>
      <div data-part="mcp-token.actions">
        <button
          type="button"
          data-part="form.button"
          data-action="copy-token"
          onClick={() => void copy()}
        >
          {t(copied ? "mcp-token-copied" : "mcp-token-copy")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="token-done"
          data-variant="primary"
          onClick={onDone}
        >
          {t("mcp-token-done")}
        </button>
      </div>
      {copied === false && (
        <p data-part="mcp-token.copy-failed" role="alert">
          {t("mcp-token-copy-failed")}
        </p>
      )}
      <p data-part="mcp-token.usage">{t("mcp-token-usage", { name })}</p>
      <p data-part="mcp-token.next">{t("mcp-token-next")}</p>
    </section>
  );
}

function CreateForm({ onCreated }: { onCreated: (client: McpClient, token: string) => void }) {
  const t = useT();
  const nail = useClient();
  const queryClient = useQueryClient();
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (name.trim() === "" || busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await nail.request(
        { type: "CreateMcpClient", name: name.trim(), grant: emptyGrant() },
        "McpClientCreated",
      );
      setName("");
      await queryClient.invalidateQueries({ queryKey: mcpKeys.clients });
      onCreated(created.client, created.token);
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form data-part="mcp-clients.create" noValidate onSubmit={(e) => void create(e)}>
      <TextField labelId="mcp-client-name" name="client-name" value={name} onChange={setName} />
      <button
        type="submit"
        data-part="form.button"
        data-action="create-client"
        data-variant="primary"
        disabled={busy || name.trim() === ""}
      >
        {t("mcp-client-create")}
      </button>
      {error && (
        <div data-part="mcp-clients.create.error" role="alert">
          {t(error === "error-conflict" ? "mcp-client-name-taken" : error)}
        </div>
      )}
    </form>
  );
}

/** "N writes of this client wait for you" with a way to the confirmation center. */
function PendingNote({ client }: { client: McpClient }) {
  const t = useT();
  const pending = usePendingConfirmations();
  const openWindow = useUi((s) => s.openWindow);
  const count = pendingOf(pending.data, { kind: "mcp-client", client_id: client.id }).length;
  if (count === 0) return null;
  return (
    <p data-part="mcp-clients.pending" role="note">
      <span data-part="mcp-clients.pending.text">
        {t("mcp-client-pending", { name: client.name, count })}
      </span>
      <button
        type="button"
        data-part="form.button"
        data-action="review-confirmations"
        onClick={() => openWindow(CENTER_DIALOG)}
      >
        {t("mcp-client-pending-review")}
      </button>
    </p>
  );
}

function ClientDetail({ client }: { client: McpClient }) {
  const t = useT();
  const nail = useClient();
  const queryClient = useQueryClient();
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const revoked = client.revoked_at !== null;

  const revoke = async () => {
    setError(null);
    try {
      await nail.request({ type: "RevokeMcpClient", client_id: client.id }, "Done");
      setConfirm(false);
      await queryClient.invalidateQueries({ queryKey: mcpKeys.all });
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  return (
    <div data-part="mcp-clients.detail" data-client={client.id}>
      <PendingNote client={client} />
      <GrantEditor key={`${client.id}:${client.revoked_at ?? ""}`} client={client} />
      <RateLimitsEditor key={`rate:${client.id}:${client.revoked_at ?? ""}`} client={client} />
      <PolicyEditor
        actor={{ kind: "mcp-client", client_id: client.id }}
        name={client.name}
        disabled={revoked}
      />
      {!revoked && (
        <div data-part="mcp-clients.revoke">
          {!confirm ? (
            <button
              type="button"
              data-part="form.button"
              data-action="revoke"
              data-variant="danger"
              onClick={() => setConfirm(true)}
            >
              {t("mcp-client-revoke")}
            </button>
          ) : (
            <span data-part="mcp-clients.confirm" role="group">
              <span data-part="mcp-clients.confirm.text">
                {t("mcp-client-revoke-confirm", { name: client.name })}
              </span>
              <button
                type="button"
                data-part="form.button"
                data-action="revoke"
                data-variant="danger"
                onClick={() => void revoke()}
              >
                {t("mcp-client-revoke-yes")}
              </button>
              <button
                type="button"
                data-part="form.button"
                data-action="cancel"
                onClick={() => setConfirm(false)}
              >
                {t("mcp-client-cancel")}
              </button>
            </span>
          )}
        </div>
      )}
      {error && (
        <div data-part="mcp-clients.error" role="alert">
          {t(error)}
        </div>
      )}
    </div>
  );
}

/** The `mcp-clients` widget. */
export function McpClients({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const locale = useLocale();
  const clients = useMcpClients();
  const server = useMcpServerInfo();
  const [selected, setSelected] = useState<McpClientId | null>(null);
  const [created, setCreated] = useState<{ name: string; token: string } | null>(null);
  const [allActivity, setAllActivity] = useState(false);
  const list = clients.data ?? [];
  const current = list.find((c) => c.id === selected) ?? list[0] ?? null;

  return (
    <section
      data-part="mcp-clients"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("mcp-clients-label")}
    >
      <p data-part="mcp-clients.intro">{t("mcp-clients-intro")}</p>
      {server.data && (
        <div
          data-part="mcp-clients.server"
          data-state={server.data.running ? "running" : "stopped"}
        >
          {server.data.running && server.data.url
            ? t("mcp-server-running", { url: server.data.url })
            : t("mcp-server-stopped")}
        </div>
      )}
      <CreateForm
        onCreated={(client, token) => {
          setSelected(client.id);
          setCreated({ name: client.name, token });
        }}
      />
      {created && (
        <TokenPanel name={created.name} token={created.token} onDone={() => setCreated(null)} />
      )}
      {clients.isError && (
        <div data-part="mcp-clients.error" role="alert">
          {t(errorMessageIdOf(clients.error))}
        </div>
      )}
      {clients.isSuccess && list.length === 0 && (
        <div data-part="mcp-clients.empty">{t("mcp-clients-empty")}</div>
      )}
      {list.length > 0 && (
        <ul data-part="mcp-clients.list" aria-label={t("mcp-clients-label")}>
          {list.map((client) => {
            const active = client.id === current?.id;
            return (
              <li key={client.id} data-part="mcp-clients.list.item">
                <button
                  type="button"
                  data-part="mcp-clients.client"
                  data-state={active ? "selected" : undefined}
                  data-access={clientState(client)}
                  aria-pressed={active}
                  onClick={() => setSelected(client.id)}
                >
                  <span data-part="mcp-clients.client.name">{client.name}</span>
                  <span data-part="mcp-clients.client.status" data-access={clientState(client)}>
                    {t(`mcp-client-state-${clientState(client)}`)}
                  </span>
                  <span data-part="mcp-clients.client.hint">
                    {t("mcp-client-token-hint", { hint: client.token_hint })}
                  </span>
                  <span data-part="mcp-clients.client.used">
                    {client.last_used_at
                      ? t("mcp-client-last-used", {
                          time: formatAuditTime(client.last_used_at, locale),
                        })
                      : t("mcp-client-never-used")}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
      {current && <ClientDetail key={current.id} client={current} />}
      <div data-part="mcp-clients.activity">
        <CheckField
          labelId="mcp-audit-show-all"
          name="audit-all"
          checked={allActivity || current === null}
          onChange={setAllActivity}
        />
        <AuditLog clientId={allActivity || current === null ? null : current.id} />
      </div>
    </section>
  );
}
