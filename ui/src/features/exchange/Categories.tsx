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
import { useId, useState, type SubmitEvent } from "react";
import { errorMessageOf } from "../../api/client";
import type { CategoryColor, ExchangeCategory, MessageSummary } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { TagCheckbox } from "../mail-actions/TagCheckbox";
import { useTagWindow } from "../mail-actions/tags";
import { Icon } from "../../widgets/Icon";
import { CategorySwatch, Problem, useWriteContext, DialogActions, DangerActions } from "./common";
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
import { EmptyDialog, LoadingDialog } from "../../widgets/DialogStates";
import { TextField } from "../../widgets/forms";
import { useBusyAction } from "../../state/busyAction";
import { useActiveMailView } from "../../state/hooks";

/** Widget `exchange-categories`. Renders nothing for other accounts. */
export function CategoryList({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const { store, accountId, search } = useActiveMailView();
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
        <button
          type="button"
          data-part="exchange-categories.new"
          onClick={() => {
            edit(null);
          }}
        >
          <Icon name="plus" part="exchange-categories.icon" />
          <span data-part="exchange-categories.action.text">
            {t("exchange-action-new-category")}
          </span>
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
                  onClick={() => {
                    showCategory(store, account.id, c.name);
                  }}
                >
                  <CategorySwatch color={c.color} />
                  <span data-part="exchange-categories.name">{c.name}</span>
                  {showCounts && c.unread > 0 && (
                    <span data-part="exchange-categories.unread">
                      {t("common-unread-count", { count: c.unread })}
                    </span>
                  )}
                  {c.sync === "pending" && (
                    <span data-part="exchange-categories.pending">
                      {t("common-not-synced-yet")}
                    </span>
                  )}
                </button>
                <button
                  type="button"
                  data-part="exchange-categories.edit"
                  aria-label={t("exchange-category-edit", { name: c.name })}
                  title={t("exchange-category-edit", { name: c.name })}
                  onClick={() => {
                    edit(c.id);
                  }}
                >
                  <Icon name="settings" part="exchange-categories.icon" />
                  <span data-part="exchange-categories.action.text">{t("common-edit")}</span>
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
                  <Icon name="trash" part="exchange-categories.icon" />
                  <span data-part="exchange-categories.action.text">{t("common-delete")}</span>
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
    <ul data-part="exchange.categories" aria-label={t("exchange-categories-title")}>
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
  onPick: () => void;
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
  const target = useExchangeUi((s) => s.categoryEditor);
  const categories = useCategories(target?.accountId ?? null);
  if (target === null) {
    return (
      <EmptyDialog
        part="exchange.dialog"
        textPart="exchange.hint"
        hintId="exchange-nothing-chosen"
        close={close}
      />
    );
  }
  if (!categories.data) {
    return <LoadingDialog part="exchange.dialog" textPart="exchange.hint" />;
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
  close: () => void;
}) {
  const t = useT();
  const ctx = useWriteContext();
  const ids = { colors: useId() };
  const [name, setName] = useState(category?.name ?? "");
  const [color, setColor] = useState<CategoryColor>(category?.color ?? "preset7");
  const { busy, problem, setProblem, run } = useBusyAction();
  const renaming = category !== null && name.trim() !== category.name;

  const finish = () => {
    exchangeStoreFor(ctx.store).getState().editCategory(null);
    close();
  };

  const submit = async (event: SubmitEvent) => {
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
    await run(async () => {
      await saveCategory(ctx, accountId, category?.id ?? null, settings);
      finish();
    });
  };

  return (
    <form
      data-part="exchange.dialog"
      data-dialog="category-editor"
      aria-busy={busy}
      onSubmit={(e) => void submit(e)}
    >
      <TextField
        labelId="common-name"
        name="name"
        value={name}
        maxLength={255}
        invalid={problem !== null}
        onChange={setName}
      />
      {renaming && (
        <p data-part="exchange.hint" data-hint="rename">
          {t("exchange-category-rename-hint", { count: category.total })}
        </p>
      )}
      <fieldset data-part="exchange.colors">
        <legend data-part="form.section.title">{t("common-colour")}</legend>
        {CATEGORY_COLORS.map((c) => (
          <ColorOption
            key={c}
            color={c}
            group={ids.colors}
            checked={color === c}
            onPick={() => {
              setColor(c);
            }}
          />
        ))}
      </fieldset>
      <DialogActions problem={problem} onCancel={finish}>
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t(category ? "exchange-save" : "exchange-category-create")}
        </button>
      </DialogActions>
    </form>
  );
}

/** Dialog `exchange-category-delete`. */
export function CategoryDeleteDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useWriteContext();
  const category = useExchangeUi((s) => s.deleting);
  const { busy, problem, run } = useBusyAction();
  const finish = () => {
    exchangeStoreFor(ctx.store).getState().deleteCategory(null);
    close();
  };
  if (category === null) {
    return (
      <EmptyDialog
        part="exchange.dialog"
        textPart="exchange.hint"
        hintId="exchange-nothing-chosen"
        close={close}
      />
    );
  }
  const remove = async () => {
    await run(async () => {
      await deleteCategory(ctx, category.account_id, category.id);
      finish();
    });
  };
  return (
    <div data-part="exchange.dialog" data-dialog="category-delete" aria-busy={busy}>
      <p data-part="exchange.question">
        {t("exchange-category-delete-question", { name: category.name })}
      </p>
      <p data-part="exchange.hint">
        {t("exchange-category-delete-hint", { count: category.total })}
      </p>
      <DangerActions
        problem={problem}
        busy={busy}
        onCancel={finish}
        onConfirm={() => void remove()}
        label={t("exchange-category-delete")}
      />
    </div>
  );
}

/**
 * Dialog `exchange-apply-categories`: the categories as check boxes over
 * what the window opened for (the conversation, the picked rows or a card):
 * checked when every message has the category, mixed when some have it.
 */
export function ApplyCategoriesDialog({ close }: DialogProps) {
  const t = useT();
  const tags = useTagWindow("category", close);
  const { summary } = tags;
  const categories = useCategories(summary?.exchange ? summary.account_id : null);
  if (!summary?.exchange) {
    return (
      <EmptyDialog
        part="exchange.dialog"
        textPart="exchange.hint"
        hintId="exchange-apply-nothing"
        close={close}
      />
    );
  }
  const list = categories.data ?? [];

  return (
    <form
      data-part="exchange.dialog"
      data-dialog="apply-categories"
      aria-busy={tags.busy}
      onSubmit={(e) => {
        e.preventDefault();
        void tags.apply(list.map((c) => c.name));
      }}
    >
      <p data-part="exchange.intro">{t("exchange-apply-intro", { subject: summary.subject })}</p>
      <fieldset data-part="exchange.colors">
        <legend data-part="form.section.title">{t("exchange-categories-title")}</legend>
        {list.length === 0 && <p data-part="exchange.hint">{t("exchange-categories-none")}</p>}
        {list.map((c) => (
          <label key={c.id} data-part="form.check" data-color={c.color}>
            <TagCheckbox
              mark={tags.mark(c.name)}
              decided={tags.decision(c.name)}
              onDecide={(checked) => {
                tags.decide(c.name, checked);
              }}
            />
            <CategorySwatch color={c.color} />
            <span data-part="form.check.label">{c.name}</span>
          </label>
        ))}
      </fieldset>
      <Problem message={tags.problem} />
      <div data-part="exchange.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={tags.busy}>
          {t("common-apply")}
        </button>
        <button type="button" data-part="form.button" onClick={close}>
          {t("common-cancel")}
        </button>
      </div>
    </form>
  );
}
