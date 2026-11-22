/**
 * Editor of one MCP client's grant: reading and (M4a) writing scopes,
 * accounts (or all accounts) and hidden folders (shared multi-value
 * pickers, R3). Nothing is granted until the user saves a grant here.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { McpClient, McpGrant, McpScope } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useAccounts, useClient } from "../../state/queries";
import { CheckField, Section } from "../../widgets/forms";
import { PickerListField } from "../../widgets/pickers";
import { useSettingsForm } from "../settings/draft";
import { ALL_SCOPES, READ_SCOPES, WRITE_SCOPES, grantIsEmpty, mcpKeys, toggled } from "./queries";

/**
 * The grant of `client`, editable unless the client is revoked. R6 (ADR
 * 0239): the edits wait for the settings page's Save.
 */
export function GrantEditor({ client }: { client: McpClient }) {
  const t = useT();
  const nail = useClient();
  const queryClient = useQueryClient();
  const accounts = useAccounts();
  const [notice, setNotice] = useState<{ id: string; error: boolean } | null>(null);
  const form = useSettingsForm<McpGrant>(client.grant, {
    save: async (grant) => {
      await nail.request(
        { type: "UpdateMcpGrants", client_id: client.id, grant },
        "McpClientSaved",
      );
      await queryClient.invalidateQueries({ queryKey: mcpKeys.clients });
    },
    onSaved: () => {
      setNotice({ id: "mcp-grant-saved", error: false });
    },
    onError: (e) => {
      setNotice({ id: errorMessageIdOf(e), error: true });
    },
  });
  const grant = form.value;
  const revoked = client.revoked_at !== null;
  const disabled = revoked;
  const all = accounts.data ?? [];
  const inScope = grant.all_accounts ? all : all.filter((a) => grant.accounts.includes(a.id));

  const change = (next: McpGrant) => {
    if (disabled) return;
    form.set(next);
    setNotice(null);
  };

  return (
    <form
      data-part="mcp-grants"
      data-client={client.id}
      data-state={revoked ? "revoked" : grantIsEmpty(grant) ? "empty" : "granted"}
      aria-label={t("mcp-grant-label", { name: client.name })}
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
      }}
    >
      {grantIsEmpty(grant) && !revoked && (
        <div data-part="mcp-grants.empty" role="note">
          {t("mcp-grant-empty")}
        </div>
      )}
      <fieldset data-part="mcp-grants.fields" disabled={disabled}>
        <Section titleId="mcp-grant-scopes">
          <ScopeChecks scopes={READ_SCOPES} write={false} grant={grant} change={change} />
        </Section>
        <Section titleId="mcp-grant-write-scopes">
          <p data-part="mcp-grants.write-intro">{t("mcp-grant-write-intro")}</p>
          <ScopeChecks scopes={WRITE_SCOPES} write={true} grant={grant} change={change} />
        </Section>
        <Section titleId="mcp-grant-accounts">
          <CheckField
            labelId="mcp-grant-all-accounts"
            name="all-accounts"
            checked={grant.all_accounts}
            onChange={(on) => {
              change({ ...grant, all_accounts: on });
            }}
          />
          {!grant.all_accounts && (
            <PickerListField
              kind="account"
              virtualAccounts
              labelId="mcp-grant-chosen-accounts"
              name="accounts"
              disabled={disabled}
              values={grant.accounts}
              onChange={(accounts) => {
                change({ ...grant, accounts });
              }}
            />
          )}
        </Section>
        {inScope.length > 0 && (
          <Section titleId="mcp-grant-folders">
            <PickerListField
              kind="folder"
              folderValue="id"
              labelId="mcp-grant-hidden-folders"
              hintId="mcp-grant-hidden-folders-hint"
              name="hidden-folders"
              disabled={disabled}
              accounts={inScope.filter((a) => a.kind !== "virtual").map((a) => a.id)}
              values={grant.excluded_folders}
              onChange={(excluded_folders) => {
                change({ ...grant, excluded_folders });
              }}
            />
          </Section>
        )}
      </fieldset>

      {notice && (
        <div
          data-part="mcp-grants.message"
          data-state={notice.error ? "error" : "ok"}
          role={notice.error ? "alert" : "status"}
        >
          {t(notice.id)}
        </div>
      )}
    </form>
  );
}

/** The scopes' check boxes, each with what it allows. */
function ScopeChecks({
  scopes,
  write,
  grant,
  change,
}: {
  scopes: readonly McpScope[];
  write: boolean;
  grant: McpGrant;
  change: (next: McpGrant) => void;
}) {
  const t = useT();
  return (
    <>
      {scopes.map((scope) => (
        <div
          key={scope}
          data-part="mcp-grants.scope"
          data-scope={scope}
          data-write={write ? "" : undefined}
        >
          <CheckField
            labelId={`mcp-scope-${scope}`}
            name={`scope-${scope}`}
            checked={grant.scopes.includes(scope)}
            onChange={(on) => {
              change({ ...grant, scopes: toggled(grant.scopes, scope, on, ALL_SCOPES) });
            }}
          />
          <span data-part="mcp-grants.scope.hint">{t(`mcp-scope-${scope}-hint`)}</span>
        </div>
      ))}
    </>
  );
}
