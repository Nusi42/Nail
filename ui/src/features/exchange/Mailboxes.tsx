/**
 * Shared mailboxes and delegate access (ADR 0072): other mailboxes appear
 * as extra folder trees under the account (`@<address>/…` in the folder
 * tree), their calendars / contacts / tasks as collections.
 *
 * - widget `exchange-mailboxes` (folder pane): the other mailboxes of the
 *   selected Exchange account with kind (shared / delegate), origin
 *   (automapped by the server or added), what they show and their state
 *   (not synced yet, no access, not found, failed); choosing a synced one
 *   opens its folder tree; "Add shared mailbox…", edit, remove (an
 *   automapped one is hidden, not deleted).
 * - dialogs `exchange-mailbox-editor` and `exchange-mailbox-remove`.
 * - mount point `MailboxFolderBadge`: on a folder-tree row of another
 *   mailbox's root, its kind and (when not synced) its state.
 */
import { useId, useState, type SubmitEvent } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  ExchangeMailbox,
  ExchangeMailboxKind,
  ExchangeMailboxSettings,
  Folder,
  LocalizedMessage,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useAccounts } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { Problem, useWriteContext } from "./common";
import {
  mailboxIssues,
  mailboxName,
  mailboxOfFolder,
  mailboxShows,
  newMailboxSettings,
} from "./model";
import { MAILBOX_EDITOR_DIALOG, MAILBOX_REMOVE_DIALOG } from "./names";
import { removeMailbox, saveMailbox, useExchangeAccount, useMailboxes } from "./queries";
import { exchangeStoreFor, useExchangeUi } from "./store";

const KINDS: readonly ExchangeMailboxKind[] = ["shared", "delegate"];
const SHOWS = ["mail", "calendar", "contacts", "tasks"] as const;

/** Widget `exchange-mailboxes`. Renders nothing for other accounts. */
export function MailboxList({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const store = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const account = useExchangeAccount(accountId);
  const mailboxes = useMailboxes(account?.id ?? null);
  if (!account) return null;
  const ui = exchangeStoreFor(store).getState();

  const edit = (address: string | null) => {
    ui.editMailbox({ accountId: account.id, address });
    store.getState().openWindow(MAILBOX_EDITOR_DIALOG);
  };

  return (
    <nav
      data-part="exchange-mailboxes"
      data-node-id={node.id ?? undefined}
      aria-label={
        node.label ? label(node.label) : t("exchange-mailboxes-label", { account: account.name })
      }
      aria-busy={mailboxes.isFetching}
    >
      <div data-part="exchange-mailboxes.header">
        <h2 data-part="exchange-mailboxes.title">{t("exchange-mailboxes-title")}</h2>
        <button
          type="button"
          data-part="exchange-mailboxes.new"
          onClick={() => {
            edit(null);
          }}
        >
          <Icon name="plus" part="exchange-mailboxes.icon" />
          <span data-part="exchange-mailboxes.action.text">{t("exchange-mailbox-add")}</span>
        </button>
      </div>
      {mailboxes.isError && (
        <Problem message={errorMessageOf(mailboxes.error)} part="exchange-mailboxes.error" />
      )}
      {mailboxes.data?.length === 0 && (
        <p data-part="exchange-mailboxes.empty">{t("exchange-mailboxes-none")}</p>
      )}
      {mailboxes.data && mailboxes.data.length > 0 && (
        <ul data-part="exchange-mailboxes.items">
          {mailboxes.data.map((m) => {
            const name = mailboxName(m);
            const open = m.root_folder_id !== null && m.root_folder_id === activeFolderId;
            const shows = mailboxShows(m.settings)
              .map((s) => t(`exchange-mailbox-show-${s}`))
              .join(", ");
            return (
              <li
                key={m.settings.address}
                data-part="exchange-mailboxes.item"
                data-kind={m.settings.kind}
                data-origin={m.origin}
                data-state={m.state}
              >
                {m.root_folder_id !== null ? (
                  <button
                    type="button"
                    data-part="exchange-mailboxes.open"
                    aria-current={open ? "true" : undefined}
                    onClick={() => {
                      if (m.root_folder_id) store.getState().selectFolder(m.root_folder_id);
                    }}
                  >
                    <span data-part="exchange-mailboxes.name">{name}</span>
                  </button>
                ) : (
                  <span data-part="exchange-mailboxes.name">{name}</span>
                )}
                {name !== m.settings.address && (
                  <span data-part="exchange-mailboxes.address">{m.settings.address}</span>
                )}
                <span data-part="exchange-mailboxes.kind">
                  {t(`exchange-mailbox-kind-${m.settings.kind}`)}
                  {m.origin === "automapped" ? ` · ${t("exchange-mailbox-automapped")}` : ""}
                </span>
                <span data-part="exchange-mailboxes.shows">{shows}</span>
                {m.state !== "synced" && (
                  <span
                    data-part="exchange-mailboxes.state"
                    role={m.state === "pending" ? "status" : "alert"}
                  >
                    {t(m.localized?.id ?? `exchange-mailbox-state-${m.state}`, m.localized?.args)}
                  </span>
                )}
                <button
                  type="button"
                  data-part="exchange-mailboxes.edit"
                  aria-label={t("exchange-mailbox-edit", { name })}
                  title={t("exchange-mailbox-edit", { name })}
                  onClick={() => {
                    edit(m.settings.address);
                  }}
                >
                  <Icon name="settings" part="exchange-mailboxes.icon" />
                  <span data-part="exchange-mailboxes.action.text">{t("exchange-edit-short")}</span>
                </button>
                <button
                  type="button"
                  data-part="exchange-mailboxes.remove"
                  aria-label={t("exchange-mailbox-remove", { name })}
                  title={t("exchange-mailbox-remove", { name })}
                  onClick={() => {
                    ui.removeMailbox(m);
                    store.getState().openWindow(MAILBOX_REMOVE_DIALOG);
                  }}
                >
                  <Icon name="trash" part="exchange-mailboxes.icon" />
                  <span data-part="exchange-mailboxes.action.text">
                    {t("exchange-remove-short")}
                  </span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </nav>
  );
}

/**
 * Mount point `MailboxFolderBadge`: for the root folder of another mailbox
 * in the folder tree, a badge with its kind and (unless synced) its state;
 * nothing for other folders.
 */
export function MailboxFolderBadge({ folder }: { folder: Pick<Folder, "account_id" | "path"> }) {
  const t = useT();
  const isMailboxPath = folder.path.startsWith("@");
  const account = useExchangeAccount(isMailboxPath ? folder.account_id : null);
  const mailboxes = useMailboxes(account?.id ?? null);
  if (!account || folder.path.includes("/")) return null;
  const mailbox = mailboxOfFolder(folder, mailboxes.data ?? []);
  if (!mailbox) return null;
  return (
    <span
      data-part="exchange.mailbox-badge"
      data-kind={mailbox.settings.kind}
      data-state={mailbox.state}
    >
      {t(`exchange-mailbox-kind-${mailbox.settings.kind}`)}
      {mailbox.state !== "synced" && ` · ${t(`exchange-mailbox-state-${mailbox.state}`)}`}
    </span>
  );
}

/** Dialog `exchange-mailbox-editor`. */
export function MailboxEditorDialog({ close }: DialogProps) {
  const t = useT();
  const target = useExchangeUi((s) => s.mailboxEditor);
  const accounts = useAccounts();
  const account = useExchangeAccount(target?.accountId ?? null);
  const mailboxes = useMailboxes(target?.accountId ?? null);
  if (target === null || (account === null && accounts.data !== undefined)) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-nothing-chosen")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  if (!mailboxes.data || !account) {
    return (
      <div data-part="exchange.dialog" data-state="loading" aria-busy="true">
        <p data-part="exchange.hint">{t("exchange-loading")}</p>
      </div>
    );
  }
  const mailbox = target.address
    ? (mailboxes.data.find((m) => m.settings.address === target.address) ?? null)
    : null;
  return (
    <MailboxEditor
      key={mailbox?.settings.address ?? "new"}
      accountId={account.id}
      own={account.email}
      mailbox={mailbox}
      mailboxes={mailboxes.data}
      close={close}
    />
  );
}

function MailboxEditor({
  accountId,
  own,
  mailbox,
  mailboxes,
  close,
}: {
  accountId: string;
  own: string;
  mailbox: ExchangeMailbox | null;
  mailboxes: readonly ExchangeMailbox[];
  close: () => void;
}) {
  const t = useT();
  const ctx = useWriteContext();
  const ids = { address: useId(), name: useId(), kind: useId(), hint: useId() };
  const [settings, setSettings] = useState<ExchangeMailboxSettings>(
    mailbox?.settings ?? newMailboxSettings(),
  );
  const [name, setName] = useState(mailbox?.settings.name ?? "");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const change = (patch: Partial<ExchangeMailboxSettings>) => {
    setSettings({ ...settings, ...patch });
  };

  const finish = () => {
    exchangeStoreFor(ctx.store).getState().editMailbox(null);
    close();
  };

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const next: ExchangeMailboxSettings = {
      ...settings,
      address: settings.address.trim(),
      // An empty name field = the server's name.
      name: name.trim() === "" ? null : name.trim(),
    };
    const issue = mailboxIssues(
      next,
      own,
      mailboxes.filter((m) => m !== mailbox),
    )[0];
    if (issue) {
      setProblem({ id: `exchange-mailbox-issue-${issue}` });
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      await saveMailbox(ctx, accountId, next, mailbox !== null);
      finish();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      data-part="exchange.dialog"
      data-dialog="mailbox-editor"
      aria-busy={busy}
      onSubmit={(e) => void submit(e)}
    >
      <p data-part="exchange.intro">{t("exchange-mailbox-intro")}</p>
      <div data-part="form.field" data-field="address">
        <label data-part="form.label" htmlFor={ids.address}>
          {t("exchange-mailbox-address")}
        </label>
        <input
          id={ids.address}
          data-part="form.input"
          type="email"
          value={settings.address}
          readOnly={mailbox !== null}
          aria-invalid={problem !== null}
          autoComplete="off"
          onChange={(e) => {
            change({ address: e.target.value });
          }}
        />
      </div>
      <div data-part="form.field" data-field="name">
        <label data-part="form.label" htmlFor={ids.name}>
          {t("exchange-mailbox-name")}
        </label>
        <input
          id={ids.name}
          data-part="form.input"
          value={name}
          aria-describedby={ids.hint}
          onChange={(e) => {
            setName(e.target.value);
          }}
        />
        <p id={ids.hint} data-part="form.hint">
          {t("exchange-mailbox-name-hint")}
        </p>
      </div>
      <fieldset data-part="form.section" data-section="kind">
        <legend data-part="form.section.title">{t("exchange-mailbox-kind")}</legend>
        {KINDS.map((kind) => (
          <label key={kind} data-part="form.check" data-field={`kind-${kind}`}>
            <input
              type="radio"
              name={ids.kind}
              checked={settings.kind === kind}
              onChange={() => {
                change({ kind });
              }}
            />
            <span data-part="form.check.label">{t(`exchange-mailbox-kind-${kind}-long`)}</span>
          </label>
        ))}
      </fieldset>
      <fieldset data-part="form.section" data-section="shows">
        <legend data-part="form.section.title">{t("exchange-mailbox-shows")}</legend>
        {SHOWS.map((key) => (
          <label key={key} data-part="form.check" data-field={key}>
            <input
              data-part="form.checkbox"
              type="checkbox"
              checked={settings[key]}
              onChange={(e) => {
                change({ [key]: e.target.checked });
              }}
            />
            <span data-part="form.check.label">{t(`exchange-mailbox-show-${key}`)}</span>
          </label>
        ))}
      </fieldset>
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button type="button" data-part="form.button" onClick={finish}>
          {t("exchange-cancel")}
        </button>
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t(mailbox ? "exchange-save" : "exchange-mailbox-open")}
        </button>
      </div>
    </form>
  );
}

/** Dialog `exchange-mailbox-remove`. */
export function MailboxRemoveDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useWriteContext();
  const mailbox = useExchangeUi((s) => s.removing);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const finish = () => {
    exchangeStoreFor(ctx.store).getState().removeMailbox(null);
    close();
  };
  if (mailbox === null) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-nothing-chosen")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  const automapped = mailbox.origin === "automapped";
  const remove = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await removeMailbox(ctx, mailbox.account_id, mailbox.settings.address);
      // Its folders are gone: leave them if one was open.
      const state = ctx.store.getState();
      if (state.activeFolderId?.startsWith(mailbox.root_folder_id ?? "\u0000")) {
        state.selectFolder(null);
      }
      finish();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-part="exchange.dialog" data-dialog="mailbox-remove" aria-busy={busy}>
      <p data-part="exchange.question">
        {t(automapped ? "exchange-mailbox-hide-question" : "exchange-mailbox-remove-question", {
          name: mailboxName(mailbox),
        })}
      </p>
      <p data-part="exchange.hint">
        {t(automapped ? "exchange-mailbox-hide-hint" : "exchange-mailbox-remove-hint")}
      </p>
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button type="button" data-part="form.button" onClick={finish}>
          {t("exchange-cancel")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-variant="danger"
          disabled={busy}
          onClick={() => void remove()}
        >
          {t(automapped ? "exchange-mailbox-hide" : "exchange-mailbox-remove-confirm")}
        </button>
      </div>
    </div>
  );
}
