/**
 * Remote content in the reader (ADR 0036, design "Privacy"): images,
 * fonts and style sheets from the internet are blocked by default. The
 * bar says so and offers to load them for this message, always for the
 * sender or always for the sender's domain; once loaded (through the
 * core's proxy, so the sender never sees your address) it says why and
 * can block them again. Mounted by the layout (`remote-content`, the
 * selected message) or by the reader itself (`RemoteContentBar`).
 *
 * Settings → Privacy (`remote-content-allows`) lists and removes the
 * allow-list entries.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { MessageBody, RemoteContentAllow, RemoteContentScope } from "../../api/generated";
import { useLabel, useLocale, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { queryKeys, useClient, useMessage } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { CONTENT_SCOPES, formatTime } from "./model";
import { remoteKeys, useRemoteContentAllows } from "./queries";

/** What the bar knows about a message's remote content. */
export interface RemoteContentView {
  /** Blocked right now. */
  blocked: boolean;
  /** Number of resources, `null` = unknown (a core before WP45). */
  resources: number | null;
  /** Why they load, `null` = blocked. */
  allowedBy: RemoteContentScope | null;
  sender: string | null;
  domain: string | null;
}

function senderOf(message: MessageBody): string | null {
  const email = message.headers.from[0]?.email.trim().toLowerCase() ?? "";
  const at = email.lastIndexOf("@");
  return at > 0 && at < email.length - 1 ? email : null;
}

/**
 * The bar's view of a message, or `null` when it has no remote content.
 * Uses `remote_content` when the core sends it, else the older
 * `remote_content_blocked` flag and the `From` address.
 */
export function remoteContentView(message: MessageBody): RemoteContentView | null {
  const info = message.remote_content;
  if (info !== undefined) {
    if (info.resources === 0) return null;
    return {
      blocked: message.remote_content_blocked,
      resources: info.resources,
      allowedBy: info.allowed_by,
      sender: info.sender,
      domain: info.domain,
    };
  }
  if (!message.remote_content_blocked) return null;
  const sender = senderOf(message);
  return {
    blocked: true,
    resources: null,
    allowedBy: null,
    sender,
    domain: sender === null ? null : sender.slice(sender.lastIndexOf("@") + 1),
  };
}

/** The allow-list value an entry of `scope` has for this message. */
export function allowValueFor(
  messageId: string,
  view: RemoteContentView,
  scope: RemoteContentScope,
): string | null {
  switch (scope) {
    case "message":
      return messageId;
    case "sender":
      return view.sender;
    case "domain":
      return view.domain;
    case "once":
      return messageId;
  }
}

/** The bar for one message; nothing when it has no remote content. */
export function RemoteContentBar({ message }: { message: MessageBody }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const view = remoteContentView(message);
  if (view === null) return null;

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.message(message.id) }),
      queryClient.invalidateQueries({ queryKey: remoteKeys.allows }),
    ]);
  };

  const allow = async (scope: RemoteContentScope) => {
    setBusy(true);
    setError(null);
    try {
      await client.request(
        { type: "AllowRemoteContent", message_id: message.id, scope },
        "RemoteContentAllowed",
      );
      await refresh();
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const block = async () => {
    const scope = view.allowedBy;
    const value = scope === null ? null : allowValueFor(message.id, view, scope);
    if (scope === null || value === null) return;
    setBusy(true);
    setError(null);
    try {
      await client.request({ type: "RemoveRemoteContentAllow", scope, value }, "Done");
      await refresh();
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const allowed = !view.blocked && view.allowedBy !== null;

  return (
    <section
      data-part="remote-content"
      data-state={allowed ? "allowed" : "blocked"}
      data-scope={view.allowedBy ?? undefined}
      aria-label={t("remote-content-label")}
    >
      <p data-part="remote-content.text">
        {allowed
          ? t(`remote-content-allowed-${view.allowedBy ?? "message"}`, {
              sender: view.sender ?? "",
              domain: view.domain ?? "",
            })
          : view.resources === null
            ? t("remote-content-blocked-unknown")
            : t("remote-content-blocked", { count: view.resources })}
      </p>
      <div
        data-part="remote-content.actions"
        role="toolbar"
        aria-label={t("remote-action-load-content")}
      >
        {allowed ? (
          <button
            type="button"
            data-part="form.button"
            data-action="block-remote-content"
            disabled={busy}
            onClick={() => void block()}
          >
            {t(`remote-content-block-${view.allowedBy ?? "message"}`)}
          </button>
        ) : (
          <>
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="allow-message"
              disabled={busy}
              onClick={() => void allow("message")}
            >
              {t("remote-content-allow-message")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="allow-once"
              disabled={busy}
              onClick={() => void allow("once")}
            >
              {t("remote-content-allow-once")}
            </button>
            {view.sender !== null && (
              <button
                type="button"
                data-part="form.button"
                data-action="allow-sender"
                disabled={busy}
                onClick={() => void allow("sender")}
              >
                {t("remote-content-allow-sender", { sender: view.sender })}
              </button>
            )}
            {view.domain !== null && (
              <button
                type="button"
                data-part="form.button"
                data-action="allow-domain"
                disabled={busy}
                onClick={() => void allow("domain")}
              >
                {t("remote-content-allow-domain", { domain: view.domain })}
              </button>
            )}
          </>
        )}
      </div>
      {!allowed && <p data-part="form.hint">{t("remote-content-privacy")}</p>}
      {error && (
        <p data-part="remote-content.error" role="alert">
          {t(error)}
        </p>
      )}
    </section>
  );
}

/** Layout widget `remote-content`: the bar of the selected message. */
export function RemoteContentWidget({ node }: WidgetProps) {
  const label = useLabel();
  const selectedId = useUi((s) => s.selectedMessageId);
  const message = useMessage(selectedId).data;
  if (message?.id !== selectedId || remoteContentView(message) === null) return null;
  return (
    <div
      data-part="remote-content.slot"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : undefined}
    >
      <RemoteContentBar message={message} />
    </div>
  );
}

function AllowRow({ entry }: { entry: RemoteContentAllow }) {
  const t = useT();
  const locale = useLocale();
  const client = useClient();
  const queryClient = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  const shown = entry.scope === "message" ? (entry.label ?? entry.value) : entry.value;

  const remove = async () => {
    setError(null);
    try {
      await client.request(
        { type: "RemoveRemoteContentAllow", scope: entry.scope, value: entry.value },
        "Done",
      );
      await queryClient.invalidateQueries({ queryKey: remoteKeys.allows });
      await queryClient.invalidateQueries({ queryKey: ["message"] });
    } catch (e) {
      setError(errorMessageIdOf(e));
    }
  };

  return (
    <li data-part="remote-allows.item" data-scope={entry.scope} data-value={entry.value}>
      <span data-part="remote-allows.value">{shown}</span>
      <span data-part="remote-allows.time">
        {t("remote-allows-since", { time: formatTime(entry.created_at, locale) })}
      </span>
      <button
        type="button"
        data-part="form.button"
        data-action="remove-allow"
        aria-label={t("remote-allows-remove-label", { value: shown })}
        onClick={() => void remove()}
      >
        {t("common-remove")}
      </button>
      {error && (
        <span data-part="remote-allows.error" role="alert">
          {t(error)}
        </span>
      )}
    </li>
  );
}

/** Layout widget `remote-content-allows`: Settings → Privacy. */
export function RemoteContentAllowsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const allows = useRemoteContentAllows();
  const entries = allows.data ?? [];
  return (
    <section
      data-part="remote-allows"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("remote-allows-title")}
    >
      <h3 data-part="remote-allows.title">{t("remote-allows-title")}</h3>
      <p data-part="remote-allows.intro">{t("remote-allows-intro")}</p>
      {allows.isError && (
        <p data-part="remote-allows.error" role="alert">
          {t(errorMessageIdOf(allows.error))}
        </p>
      )}
      {allows.isSuccess && entries.length === 0 && (
        <p data-part="remote-allows.empty">{t("remote-allows-empty")}</p>
      )}
      {CONTENT_SCOPES.map((scope) => {
        const of = entries.filter((e) => e.scope === scope);
        if (of.length === 0) return null;
        return (
          <section key={scope} data-part="remote-allows.group" data-scope={scope}>
            <h4 data-part="remote-allows.subtitle">{t(`remote-allows-scope-${scope}`)}</h4>
            <ul data-part="remote-allows.list" aria-label={t(`remote-allows-scope-${scope}`)}>
              {of.map((e) => (
                <AllowRow key={`${e.scope}:${e.value}`} entry={e} />
              ))}
            </ul>
          </section>
        );
      })}
    </section>
  );
}
