/**
 * Gmail labels in the folder pane (widget `label-list`): the user labels of
 * the selected Gmail account as a tree (`Work/Projects` nests), with their
 * colour and unread count; choosing one shows its messages across folders
 * (a label view = a search on its tag, ADR 0065); edit / delete per label,
 * "new label". Plus `LabelSwatch` and the `MessageLabels` mount point
 * (label chips and the category of one message, for list rows and the
 * reader).
 */
import { useId } from "react";
import type { GmailLabel, MessageSummary } from "../../api/generated";
import { errorMessageOf } from "../../api/client";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { labelQuery, labelTree, pruneTree, type LabelNode } from "./model";
import { LABEL_DELETE_DIALOG, LABEL_EDITOR_DIALOG } from "./names";
import { showLabel, useGmailAccount, useGmailLabels } from "./queries";
import { gmailStoreFor } from "./store";

/** A label's colour dot or chip background (theme geometry, never inline styles). */
export function LabelSwatch({
  label,
  part = "gmail.swatch",
}: {
  label: GmailLabel;
  part?: string;
}) {
  const geom = `gmail-swatch-${useId().replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  useGeometry(
    geom,
    label.color ? { "background-color": label.color.background, color: label.color.text } : {},
  );
  return (
    <span
      data-part={part}
      data-geom={geom}
      data-color={label.color ? "custom" : "none"}
      aria-hidden="true"
    />
  );
}

/** Widget `label-list`. Renders nothing for accounts that are not Gmail accounts. */
export function LabelList({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const store = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const search = useUi((s) => s.search);
  const account = useGmailAccount(accountId);
  const labels = useGmailLabels(account?.id ?? null);
  const showHidden = optionBool(node, "show-hidden", false);
  const showCounts = optionBool(node, "show-counts", true);
  if (!account) return null;
  const tree = pruneTree(labelTree(labels.data ?? []), showHidden);
  const activeQuery = search?.accountId === account.id ? search.text : null;
  const gmail = gmailStoreFor(store).getState();

  const renderNodes = (nodes: LabelNode[], depth: number) => (
    <ul data-part="label-list.items" data-depth={depth}>
      {nodes.map((n) => {
        const own = n.label;
        const active = own !== null && activeQuery === labelQuery(own.tag ?? own.name);
        return (
          <li
            key={n.path}
            data-part="label-list.item"
            data-depth={depth}
            data-state={active ? "selected" : undefined}
            data-sync={own?.sync}
          >
            {own ? (
              <div data-part="label-list.row">
                <button
                  type="button"
                  data-part="label-list.view"
                  aria-current={active ? "true" : undefined}
                  aria-label={t("gmail-label-view", {
                    name: n.name,
                    unread: showCounts ? own.unread : 0,
                    pending: own.sync === "pending" ? "yes" : "no",
                  })}
                  onClick={() => showLabel(store, account.id, own.tag ?? own.name)}
                >
                  <LabelSwatch label={own} />
                  <span data-part="label-list.name">{n.name}</span>
                  {showCounts && own.unread > 0 && (
                    <span data-part="label-list.unread">
                      {t("gmail-label-unread", { count: own.unread })}
                    </span>
                  )}
                  {own.sync === "pending" && (
                    <span data-part="label-list.pending">{t("gmail-label-pending")}</span>
                  )}
                </button>
                <button
                  type="button"
                  data-part="label-list.edit"
                  aria-label={t("gmail-label-edit", { name: own.name })}
                  title={t("gmail-label-edit", { name: own.name })}
                  onClick={() => {
                    gmail.editLabel({ accountId: account.id, labelId: own.id, parent: null });
                    store.getState().openWindow(LABEL_EDITOR_DIALOG);
                  }}
                >
                  <Icon name="settings" part="label-list.icon" />
                  <span data-part="label-list.action.text">{t("gmail-edit-short")}</span>
                </button>
                <button
                  type="button"
                  data-part="label-list.delete"
                  aria-label={t("gmail-label-remove", { name: own.name })}
                  title={t("gmail-label-remove", { name: own.name })}
                  onClick={() => {
                    gmail.deleteLabel(own);
                    store.getState().openWindow(LABEL_DELETE_DIALOG);
                  }}
                >
                  <Icon name="trash" part="label-list.icon" />
                  <span data-part="label-list.action.text">{t("gmail-delete-short")}</span>
                </button>
              </div>
            ) : (
              <span data-part="label-list.group">{n.name}</span>
            )}
            {n.children.length > 0 && renderNodes(n.children, depth + 1)}
          </li>
        );
      })}
    </ul>
  );

  return (
    <nav
      data-part="label-list"
      data-node-id={node.id ?? undefined}
      aria-label={
        node.label ? label(node.label) : t("gmail-labels-label", { account: account.name })
      }
      aria-busy={labels.isFetching}
    >
      <div data-part="label-list.header">
        <h2 data-part="label-list.title">{t("gmail-labels-title")}</h2>
        <button
          type="button"
          data-part="label-list.new"
          onClick={() => {
            gmail.editLabel({ accountId: account.id, labelId: null, parent: null });
            store.getState().openWindow(LABEL_EDITOR_DIALOG);
          }}
        >
          <Icon name="plus" part="label-list.icon" />
          <span data-part="label-list.action.text">{t("gmail-label-new")}</span>
        </button>
      </div>
      {labels.isError && (
        <p data-part="label-list.error" role="alert">
          {t(errorMessageOf(labels.error).id, errorMessageOf(labels.error).args)}
        </p>
      )}
      {labels.data && tree.length === 0 && (
        <p data-part="label-list.empty">{t("gmail-labels-none")}</p>
      )}
      {tree.length > 0 && renderNodes(tree, 0)}
    </nav>
  );
}

/**
 * Mount point `MessageLabels`: the user labels (with their colours) and
 * the inbox category of one Gmail message; renders nothing for other mail.
 */
export function MessageLabels({ message }: { message: MessageSummary }) {
  const t = useT();
  const labels = useGmailLabels(message.gmail ? message.account_id : null);
  if (!message.gmail) return null;
  const tags = message.tags.map((tag) => tag.toLowerCase());
  const shown = (labels.data ?? []).filter(
    (l) =>
      l.kind === "user" &&
      l.message_visibility === "show" &&
      tags.includes((l.tag ?? l.name).toLowerCase()),
  );
  const category = message.gmail.category;
  if (shown.length === 0 && category === null) return null;
  return (
    <ul data-part="gmail.labels" aria-label={t("gmail-message-labels")}>
      {category && (
        <li data-part="gmail.category" data-category={category}>
          {t(`gmail-category-${category}`)}
        </li>
      )}
      {shown.map((l) => (
        <li key={l.id} data-part="gmail.chip" data-label={l.id}>
          <LabelSwatch label={l} part="gmail.chip.swatch" />
          <span data-part="gmail.chip.name">{l.name}</span>
        </li>
      ))}
    </ul>
  );
}
