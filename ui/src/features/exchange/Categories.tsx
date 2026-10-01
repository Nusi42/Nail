/**
 * Exchange categories (Nail tags with Outlook's colours, ADR 0072):
 *
 * - widget `exchange-categories`: the master list of the selected Exchange
 *   account with colour, message and unread counts; choosing one shows its
 *   messages across folders (a search on its tag); edit / delete; "new
 *   category". Pending changes are marked.
 * - dialogs `exchange-category-editor` (name + one of the 25 colours or
 *   none) and `exchange-category-delete` (the tag stays on messages, as in
 *   Outlook).
 * - mount point `MessageCategories`: the categories of one Exchange
 *   message as coloured chips (list rows, reader).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId, useMemo, useState, type FormEvent } from "react";
import { errorMessageOf } from "../../api/client";
import type {
  CategoryColor,
  ExchangeCategory,
  LocalizedMessage,
  MessageSummary,
} from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { findSummary, patchSummaries, queued } from "../../state/mailOps";
import { useClient } from "../../state/queries";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { CategorySwatch, Problem, useWriteContext } from "./common";
import { CATEGORY_COLORS } from "./messages";
import { categoryIssues, categoryOfTag, tagQuery } from "./model";
import { CATEGORY_DELETE_DIALOG, CATEGORY_EDITOR_DIALOG } from "./names";
import {
  deleteCategory,
  saveCategory,
  showCategory,
  useCategories,
  useExchangeAccount,
} from "./queries";
import { exchangeStoreFor, useExchangeUi } from "./store";

/** Widget `exchange-categories`. Renders nothing for other accounts. */
export function CategoryList({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const store = useUiStoreApi();
  const accountId = useUi((s) => s.activeAccountId);
  const search = useUi((s) => s.search);
  const account = useExchangeAccount(accountId);
  const categories = useCategories(account?.id ?? null);
  const showCounts = optionBool(node, "show-counts", true);
  const showEmpty = optionBool(node, "show-empty", true);
  if (!account) return null;
  const ui = exchangeStoreFor(store).getState();
  const activeQuery = search?.accountId === account.id ? search.text : null;
  const shown = (categories.data ?? []).filter((c) => showEmpty || c.total > 0);

  const edit = (categoryId: string | null) => {
    ui.editCategory({ accountId: account.id, categoryId });
    store.getState().openWindow(CATEGORY_EDITOR_DIALOG);
  };

  return (
    <nav
      data-part="exchange-categories"
      data-node-id={node.id ?? undefined}
      aria-label={
        node.label ? label(node.label) : t("exchange-categories-label", { account: account.name })
      }
      aria-busy={categories.isFetching}
    >
      <div data-part="exchange-categories.header">
        <h2 data-part="exchange-categories.title">{t("exchange-categories-title")}</h2>
        <button type="button" data-part="exchange-categories.new" onClick={() => edit(null)}>
          <Icon name="plus" part="exchange-categories.icon" />
          <span data-part="exchange-categories.action.text">{t("exchange-category-new")}</span>
        </button>
      </div>
      {categories.isError && (
        <Problem message={errorMessageOf(categories.error)} part="exchange-categories.error" />
      )}
      {categories.data && shown.length === 0 && (
        <p data-part="exchange-categories.empty">{t("exchange-categories-none")}</p>
      )}
      {shown.length > 0 && (
        <ul data-part="exchange-categories.items">
          {shown.map((c) => {
            const active = activeQuery === tagQuery(c.name);
            return (
              <li
                key={c.id}
                data-part="exchange-categories.item"
                data-state={active ? "selected" : undefined}
                data-sync={c.sync}
                data-color={c.color}
              >
                <button
                  type="button"
                  data-part="exchange-categories.view"
                  aria-current={active ? "true" : undefined}
                  aria-label={t("exchange-category-view", {
                    name: c.name,
                    color: t(`exchange-color-${c.color}`),
                    unread: showCounts ? c.unread : 0,
                    pending: c.sync === "pending" ? "yes" : "no",
                  })}
                  onClick={() => showCategory(store, account.id, c.name)}
                >
                  <CategorySwatch color={c.color} />
                  <span data-part="exchange-categories.name">{c.name}</span>
                  {showCounts && c.unread > 0 && (
                    <span data-part="exchange-categories.unread">
                      {t("exchange-category-unread", { count: c.unread })}
                    </span>
                  )}
                  {c.sync === "pending" && (
                    <span data-part="exchange-categories.pending">{t("exchange-pending")}</span>
                  )}
                </button>
                <button
                  type="button"
                  data-part="exchange-categories.edit"
                  aria-label={t("exchange-category-edit", { name: c.name })}
                  title={t("exchange-category-edit", { name: c.name })}
                  onClick={() => edit(c.id)}
                >
                  <Icon name="settings" part="exchange-categories.icon" />
                  <span data-part="exchange-categories.action.text">
                    {t("exchange-edit-short")}
                  </span>
                </button>
                <button
                  type="button"
                  data-part="exchange-categories.delete"
                  aria-label={t("exchange-category-remove", { name: c.name })}
                  title={t("exchange-category-remove", { name: c.name })}
                  onClick={() => {
                    ui.deleteCategory(c);
                    store.getState().openWindow(CATEGORY_DELETE_DIALOG);
                  }}
                >
                  <Icon name="delete" part="exchange-categories.icon" />
                  <span data-part="exchange-categories.action.text">
                    {t("exchange-delete-short")}
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
 * Mount point `MessageCategories`: the categories (tags) of one Exchange
 * message as chips in their master-list colours; renders nothing for other
 * mail or mail without categories.
 */
export function MessageCategories({ message }: { message: MessageSummary }) {
  const t = useT();
  const categories = useCategories(message.exchange ? message.account_id : null);
  if (!message.exchange) return null;
  const tags = message.tags.filter((tag) => !tag.startsWith("$"));
  if (tags.length === 0) return null;
  return (
    <ul data-part="exchange.categories" aria-label={t("exchange-message-categories")}>
      {tags.map((tag) => {
        const category = categoryOfTag(categories.data ?? [], tag);
        const color: CategoryColor = category?.color ?? "none";
        return (
          <li key={tag} data-part="exchange.chip" data-color={color}>
            <CategorySwatch color={color} part="exchange.chip.swatch" />
            <span data-part="exchange.chip.name">{category?.name ?? tag}</span>
          </li>
        );
      })}
    </ul>
  );
}

/** One colour as a radio button. */
function ColorOption({
  color,
  group,
  checked,
  onPick,
}: {
  color: CategoryColor;
  group: string;
  checked: boolean;
  onPick(): void;
}) {
  const t = useT();
  return (
    <label
      data-part="exchange.color"
      data-color={color}
      data-state={checked ? "selected" : undefined}
    >
      <input type="radio" name={group} checked={checked} onChange={onPick} />
      <CategorySwatch color={color} part="exchange.color.sample" />
      <span data-part="exchange.color.name">{t(`exchange-color-${color}`)}</span>
    </label>
  );
}

/** Dialog `exchange-category-editor`. */
export function CategoryEditorDialog({ close }: DialogProps) {
  const t = useT();
  const target = useExchangeUi((s) => s.categoryEditor);
  const categories = useCategories(target?.accountId ?? null);
  if (target === null) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-nothing-chosen")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  if (!categories.data) {
    return (
      <div data-part="exchange.dialog" data-state="loading" aria-busy="true">
        <p data-part="exchange.hint">{t("exchange-loading")}</p>
      </div>
    );
  }
  const category = target.categoryId
    ? (categories.data.find((c) => c.id === target.categoryId) ?? null)
    : null;
  return (
    <CategoryEditor
      key={category?.id ?? "new"}
      accountId={target.accountId}
      category={category}
      categories={categories.data}
      close={close}
    />
  );
}

function CategoryEditor({
  accountId,
  category,
  categories,
  close,
}: {
  accountId: string;
  category: ExchangeCategory | null;
  categories: readonly ExchangeCategory[];
  close(): void;
}) {
  const t = useT();
  const ctx = useWriteContext();
  const ids = { name: useId(), colors: useId() };
  const [name, setName] = useState(category?.name ?? "");
  const [color, setColor] = useState<CategoryColor>(category?.color ?? "preset7");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const renaming = category !== null && name.trim() !== category.name;

  const finish = () => {
    exchangeStoreFor(ctx.store).getState().editCategory(null);
    close();
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const settings = { name: name.trim(), color };
    const issue = categoryIssues(
      settings,
      categories.filter((c) => c.id !== category?.id),
    )[0];
    if (issue) {
      setProblem({ id: `exchange-category-issue-${issue}` });
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      await saveCategory(ctx, accountId, category?.id ?? null, settings);
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
      data-dialog="category-editor"
      aria-busy={busy}
      onSubmit={(e) => void submit(e)}
    >
      <div data-part="form.field" data-field="name">
        <label data-part="form.label" htmlFor={ids.name}>
          {t("exchange-category-name")}
        </label>
        <input
          id={ids.name}
          data-part="form.input"
          value={name}
          maxLength={255}
          aria-invalid={problem !== null}
          onChange={(e) => setName(e.target.value)}
        />
      </div>
      {renaming && (
        <p data-part="exchange.hint" data-hint="rename">
          {t("exchange-category-rename-hint", { count: category.total })}
        </p>
      )}
      <fieldset data-part="exchange.colors">
        <legend data-part="form.section.title">{t("exchange-category-color")}</legend>
        {CATEGORY_COLORS.map((c) => (
          <ColorOption
            key={c}
            color={c}
            group={ids.colors}
            checked={color === c}
            onPick={() => setColor(c)}
          />
        ))}
      </fieldset>
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button type="button" data-part="form.button" onClick={finish}>
          {t("exchange-cancel")}
        </button>
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t(category ? "exchange-save" : "exchange-category-create")}
        </button>
      </div>
    </form>
  );
}

/** Dialog `exchange-category-delete`. */
export function CategoryDeleteDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useWriteContext();
  const category = useExchangeUi((s) => s.deleting);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const finish = () => {
    exchangeStoreFor(ctx.store).getState().deleteCategory(null);
    close();
  };
  if (category === null) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-nothing-chosen")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  const remove = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await deleteCategory(ctx, category.account_id, category.id);
      finish();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div data-part="exchange.dialog" data-dialog="category-delete" aria-busy={busy}>
      <p data-part="exchange.question">
        {t("exchange-category-delete-question", { name: category.name })}
      </p>
      <p data-part="exchange.hint">
        {t("exchange-category-delete-hint", { count: category.total })}
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
          {t("exchange-category-delete")}
        </button>
      </div>
    </div>
  );
}

/** Dialog `exchange-apply-categories`: the categories of the selected message as check boxes. */
export function ApplyCategoriesDialog({ close }: DialogProps) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  const messageId = useUi((s) => s.selectedMessageId);
  const summary = useMemo(
    () => (messageId ? findSummary(queryClient, messageId) : undefined),
    [messageId, queryClient],
  );
  const categories = useCategories(summary?.exchange ? summary.account_id : null);
  const [chosen, setChosen] = useState<Set<string> | null>(null);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  if (!summary?.exchange) {
    return (
      <div data-part="exchange.dialog" data-state="empty">
        <p data-part="exchange.hint">{t("exchange-apply-nothing")}</p>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-close")}
        </button>
      </div>
    );
  }
  const list = categories.data ?? [];
  const has = (name: string) =>
    summary.tags.some((tag) => tag.toLowerCase() === name.toLowerCase());
  const selected = chosen ?? new Set(list.filter((c) => has(c.name)).map((c) => c.name));

  const apply = async (event: FormEvent) => {
    event.preventDefault();
    const add = list.filter((c) => selected.has(c.name) && !has(c.name)).map((c) => c.name);
    const remove = summary.tags.filter(
      (tag) =>
        list.some((c) => c.name.toLowerCase() === tag.toLowerCase()) &&
        ![...selected].some((s) => s.toLowerCase() === tag.toLowerCase()),
    );
    if (add.length === 0 && remove.length === 0) {
      close();
      return;
    }
    setBusy(true);
    setProblem(null);
    patchSummaries(queryClient, [summary.id], (m) => ({
      ...m,
      tags: [...new Set([...m.tags.filter((tag) => !remove.includes(tag)), ...add])].sort(),
    }));
    try {
      await queued(
        { client, store, queryClient },
        {
          type: "UpdateFlags",
          message_ids: [summary.id],
          change: {
            seen: null,
            flagged: null,
            answered: null,
            draft: null,
            add_tags: add,
            remove_tags: remove,
          },
        },
      );
      close();
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <form
      data-part="exchange.dialog"
      data-dialog="apply-categories"
      aria-busy={busy}
      onSubmit={(e) => void apply(e)}
    >
      <p data-part="exchange.intro">{t("exchange-apply-intro", { subject: summary.subject })}</p>
      <fieldset data-part="exchange.colors">
        <legend data-part="form.section.title">{t("exchange-categories-title")}</legend>
        {list.length === 0 && <p data-part="exchange.hint">{t("exchange-categories-none")}</p>}
        {list.map((c) => (
          <label key={c.id} data-part="form.check" data-color={c.color}>
            <input
              type="checkbox"
              checked={selected.has(c.name)}
              onChange={(e) => {
                const next = new Set(selected);
                if (e.target.checked) next.add(c.name);
                else next.delete(c.name);
                setChosen(next);
              }}
            />
            <CategorySwatch color={c.color} />
            <span data-part="form.check.label">{c.name}</span>
          </label>
        ))}
      </fieldset>
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("exchange-apply")}
        </button>
        <button type="button" data-part="form.button" onClick={close}>
          {t("exchange-cancel")}
        </button>
      </div>
    </form>
  );
}
