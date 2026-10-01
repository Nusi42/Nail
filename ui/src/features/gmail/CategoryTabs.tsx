/**
 * Gmail's inbox categories as a filter (widget `category-tabs`): "All" and
 * Primary / Social / Promotions / Updates / Forums with their unread
 * counts; choosing one shows the inbox filtered by `category:<name>` (the
 * query language's category filter, ADR 0065). Renders nothing for
 * accounts that are not Gmail accounts.
 */
import type { GmailCategory } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, optionList, type WidgetProps } from "../../layout/widgetRegistry";
import { useFolders } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { CATEGORIES, categoryLabel, categoryOfQuery } from "./model";
import { showCategory, useGmailAccount, useGmailLabels } from "./queries";

export function CategoryTabs({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const store = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const activeFolderId = useUi((s) => s.activeFolderId);
  const search = useUi((s) => s.search);
  const account = useGmailAccount(accountId);
  const folders = useFolders(account?.id ?? null);
  const labels = useGmailLabels(account?.id ?? null);
  const showAll = optionBool(node, "show-all", true);
  const showCounts = optionBool(node, "show-counts", true);
  const wanted = optionList(node, "categories", CATEGORIES);
  const categories = CATEGORIES.filter((c) => wanted.includes(c));
  const inbox = folders.data?.find((f) => f.role === "inbox") ?? null;
  if (!account || !inbox) return null;

  const active = search?.accountId === account.id ? categoryOfQuery(search.text) : null;
  const allActive = active === null && search === null && activeFolderId === inbox.id;
  const unread = (c: GmailCategory) =>
    labels.data?.find((l) => l.id === categoryLabel(c))?.unread ?? 0;

  return (
    <nav
      data-part="category-tabs"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("gmail-categories-label")}
    >
      <ul data-part="category-tabs.list">
        {showAll && (
          <li data-part="category-tabs.item">
            <button
              type="button"
              data-part="category-tabs.tab"
              data-category="all"
              data-state={allActive ? "selected" : undefined}
              aria-pressed={allActive}
              onClick={() => showCategory(store, account.id, inbox.id, null)}
            >
              {t("gmail-category-all")}
            </button>
          </li>
        )}
        {categories.map((c) => {
          const count = showCounts ? unread(c) : 0;
          return (
            <li key={c} data-part="category-tabs.item">
              <button
                type="button"
                data-part="category-tabs.tab"
                data-category={c}
                data-state={active === c ? "selected" : undefined}
                aria-pressed={active === c}
                aria-label={t("gmail-category-tab", {
                  name: t(`gmail-category-${c}`),
                  unread: count,
                })}
                onClick={() => showCategory(store, account.id, inbox.id, c)}
              >
                <span data-part="category-tabs.name">{t(`gmail-category-${c}`)}</span>
                {count > 0 && (
                  <span data-part="category-tabs.unread">
                    {t("gmail-category-unread", { count })}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
