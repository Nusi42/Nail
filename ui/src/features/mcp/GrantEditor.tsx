/**
 * Editor of one MCP client's grant: reading and (M4a) writing scopes,
 * accounts (or all accounts) and hidden folders. Nothing is granted until the user saves a grant here.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { Account, McpClient, McpGrant } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useAccounts, useClient, useFolders } from "../../state/queries";
import { CheckField, Section } from "../../widgets/forms";
import { ALL_SCOPES, READ_SCOPES, WRITE_SCOPES, grantIsEmpty, mcpKeys, toggled } from "./queries";

function FolderExclusions({
  account,
  grant,
  disabled,
  onChange,
}: {
  account: Account;
  grant: McpGrant;
  disabled: boolean;
  onChange: (grant: McpGrant) => void;
}) {
  const folders = useFolders(account.id);
  const t = useT();
  const hidden = (folders.data ?? []).filter((f) => grant.excluded_folders.includes(f.id)).length;
  return (
    <details data-part="mcp-grants.folders.account" data-account={account.id}>
      <summary data-part="mcp-grants.folders.summary">
        {t("mcp-grant-folders-of", { account: account.name, hidden })}
      </summary>
      <div data-part="mcp-grants.folders.list">
        {(folders.data ?? []).map((folder) => (
          <CheckField
            key={folder.id}
            labelId="mcp-grant-folder-hidden"
            args={{ folder: folder.path }}
            name={`hide-${folder.id}`}
            checked={grant.excluded_folders.includes(folder.id)}
            onChange={(on) => {
              if (!disabled) {
                onChange({
                  ...grant,
                  excluded_folders: toggled(grant.excluded_folders, folder.id, on),
                });
              }
            }}
          />
        ))}
      </div>
    </details>
  );
}

/** The grant of `client`, editable unless the client is revoked. */
export function GrantEditor({ client }: { client: McpClient }) {
  const t = useT();
  const nail = useClient();
  const queryClient = useQueryClient();
  const accounts = useAccounts();
  const [grant, setGrant] = useState<McpGrant>(client.grant);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<{ id: string; error: boolean } | null>(null);
  const revoked = client.revoked_at !== null;
  const disabled = revoked || busy;
  const all = accounts.data ?? [];
  const inScope = grant.all_accounts ? all : all.filter((a) => grant.accounts.includes(a.id));
  const dirty = JSON.stringify(grant) !== JSON.stringify(client.grant);

  const change = (next: McpGrant) => {
    if (disabled) return;
    setGrant(next);
    setNotice(null);
  };

  const save = async (event: FormEvent) => {
    event.preventDefault();
    if (disabled) return;
    setBusy(true);
    setNotice(null);
    try {
      await nail.request(
        { type: "UpdateMcpGrants", client_id: client.id, grant },
        "McpClientSaved",
      );
      await queryClient.invalidateQueries({ queryKey: mcpKeys.clients });
      setNotice({ id: "mcp-grant-saved", error: false });
    } catch (e) {
      setNotice({ id: errorMessageIdOf(e), error: true });
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      data-part="mcp-grants"
      data-client={client.id}
      data-state={revoked ? "revoked" : grantIsEmpty(grant) ? "empty" : "granted"}
      aria-label={t("mcp-grant-label", { name: client.name })}
      noValidate
      onSubmit={(e) => void save(e)}
    >
      {grantIsEmpty(grant) && !revoked && (
        <div data-part="mcp-grants.empty" role="note">
          {t("mcp-grant-empty")}
        </div>
      )}
      <fieldset data-part="mcp-grants.fields" disabled={disabled}>
        <Section titleId="mcp-grant-scopes">
          {READ_SCOPES.map((scope) => (
            <div key={scope} data-part="mcp-grants.scope" data-scope={scope}>
              <CheckField
                labelId={`mcp-scope-${scope}`}
                name={`scope-${scope}`}
                checked={grant.scopes.includes(scope)}
                onChange={(on) =>
                  change({ ...grant, scopes: toggled(grant.scopes, scope, on, ALL_SCOPES) })
                }
              />
              <span data-part="mcp-grants.scope.hint">{t(`mcp-scope-${scope}-hint`)}</span>
            </div>
          ))}
        </Section>
        <Section titleId="mcp-grant-write-scopes">
          <p data-part="mcp-grants.write-intro">{t("mcp-grant-write-intro")}</p>
          {WRITE_SCOPES.map((scope) => (
            <div key={scope} data-part="mcp-grants.scope" data-scope={scope} data-write="">
              <CheckField
                labelId={`mcp-scope-${scope}`}
                name={`scope-${scope}`}
                checked={grant.scopes.includes(scope)}
                onChange={(on) =>
                  change({ ...grant, scopes: toggled(grant.scopes, scope, on, ALL_SCOPES) })
                }
              />
              <span data-part="mcp-grants.scope.hint">{t(`mcp-scope-${scope}-hint`)}</span>
            </div>
          ))}
        </Section>
        <Section titleId="mcp-grant-accounts">
          <CheckField
            labelId="mcp-grant-all-accounts"
            name="all-accounts"
            checked={grant.all_accounts}
            onChange={(on) => change({ ...grant, all_accounts: on })}
          />
          {!grant.all_accounts &&
            all.map((account) => (
              <CheckField
                key={account.id}
                labelId="mcp-grant-account"
                args={{ name: account.name, email: account.email }}
                name={`account-${account.id}`}
                checked={grant.accounts.includes(account.id)}
                onChange={(on) =>
                  change({ ...grant, accounts: toggled(grant.accounts, account.id, on) })
                }
              />
            ))}
        </Section>
        {inScope.length > 0 && (
          <Section titleId="mcp-grant-folders">
            {inScope
              .filter((a) => a.kind !== "virtual")
              .map((account) => (
                <FolderExclusions
                  key={account.id}
                  account={account}
                  grant={grant}
                  disabled={disabled}
                  onChange={change}
                />
              ))}
          </Section>
        )}
      </fieldset>
      <div data-part="mcp-grants.actions">
        <button
          type="submit"
          data-part="form.button"
          data-action="save-grant"
          data-variant="primary"
          disabled={disabled || !dirty}
        >
          {t("mcp-grant-save")}
        </button>
      </div>
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
