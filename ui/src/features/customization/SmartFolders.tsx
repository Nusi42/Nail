/**
 * Smart folders: saved searches placed in the layout like folders
 * (`smart-folders` widget). Choosing one runs its query in the message list;
 * the current search can be saved under a name; saved searches can be
 * removed. The core stores them (`ListSavedSearches`, `SaveSavedSearch`,
 * `DeleteSavedSearch`) and announces changes with `SavedSearchesChanged`.
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type SubmitEvent } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { SavedSearch } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";

/** Query key of the saved searches. */
export const savedSearchesKey = ["saved-searches"] as const;

export function useSavedSearches() {
  const client = useClient();
  return useQuery({
    queryKey: savedSearchesKey,
    queryFn: async () =>
      (await client.request({ type: "ListSavedSearches" }, "SavedSearches")).searches,
  });
}

/** Whether a saved search is the search the message list shows. */
export function isActiveSearch(
  saved: SavedSearch,
  search: { text: string; accountId: string | null; folderId: string | null } | null,
): boolean {
  return (
    search !== null &&
    search.text === saved.query &&
    search.accountId === saved.account_id &&
    search.folderId === saved.folder_id
  );
}

export function SmartFolders({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const client = useClient();
  const queryClient = useQueryClient();
  const saved = useSavedSearches();
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const editable = optionBool(node, "editable", true);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);

  const refresh = () => queryClient.invalidateQueries({ queryKey: savedSearchesKey });
  const run = async (work: () => Promise<unknown>) => {
    setError(null);
    try {
      await work();
      await refresh();
      return true;
    } catch (e) {
      setError(errorMessageIdOf(e));
      return false;
    }
  };

  const open = (s: SavedSearch) => {
    setSearch({ text: s.query, accountId: s.account_id, folderId: s.folder_id, error: null });
  };

  const save = async (event: SubmitEvent) => {
    event.preventDefault();
    if (!search || name.trim() === "") return;
    const ok = await run(() =>
      client.request(
        {
          type: "SaveSavedSearch",
          search: {
            id: null,
            name: name.trim(),
            query: search.text,
            account_id: search.accountId,
            folder_id: search.folderId,
            sort: "date",
            position: 0,
          },
        },
        "SavedSearchSaved",
      ),
    );
    if (ok) {
      setNaming(false);
      setName("");
    }
  };

  const remove = (s: SavedSearch) => {
    const id = s.id;
    return id === null
      ? Promise.resolve(false)
      : run(() => client.request({ type: "DeleteSavedSearch", search_id: id }, "Done"));
  };

  const list = saved.data ?? [];
  return (
    <nav
      data-part="smart-folders"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("common-smart-folders")}
      aria-busy={saved.isFetching}
    >
      {saved.isError && (
        <div data-part="smart-folders.error" role="alert">
          {t(errorMessageIdOf(saved.error))}
        </div>
      )}
      {error && (
        <div data-part="smart-folders.error" role="alert">
          {t(error)}
        </div>
      )}
      {saved.isSuccess && list.length === 0 && (
        <p data-part="smart-folders.empty">{t("saved-search-empty")}</p>
      )}
      <ul data-part="smart-folders.list">
        {list.map((s) => {
          const active = isActiveSearch(s, search);
          return (
            <li key={s.id ?? s.name} data-part="smart-folders.entry">
              <button
                type="button"
                data-part="smart-folders.item"
                data-state={active ? "selected" : undefined}
                aria-current={active ? "true" : undefined}
                title={s.query}
                onClick={() => {
                  open(s);
                }}
              >
                <Icon name="search" part="smart-folders.item.icon" />
                <span data-part="smart-folders.item.name">{s.name}</span>
              </button>
              {editable && (
                <button
                  type="button"
                  data-part="smart-folders.remove"
                  aria-label={t("saved-search-remove", { name: s.name })}
                  title={t("saved-search-remove", { name: s.name })}
                  onClick={() => void remove(s)}
                >
                  <Icon name="trash" part="smart-folders.remove.icon" />
                </button>
              )}
            </li>
          );
        })}
      </ul>
      {editable &&
        (naming ? (
          <form data-part="smart-folders.form" onSubmit={(e) => void save(e)}>
            <input
              data-part="smart-folders.name"
              value={name}
              aria-label={t("saved-search-name")}
              placeholder={t("saved-search-name")}
              onChange={(e) => {
                setName(e.target.value);
              }}
              // eslint-disable-next-line jsx-a11y/no-autofocus -- the name field opens on request (Save search) and takes the focus.
              autoFocus
            />
            <button
              type="submit"
              data-part="form.button"
              data-action="save"
              disabled={name.trim() === ""}
            >
              {t("common-save")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="cancel"
              onClick={() => {
                setNaming(false);
                setName("");
              }}
            >
              {t("common-cancel")}
            </button>
          </form>
        ) : (
          <button
            type="button"
            data-part="smart-folders.add"
            disabled={search === null}
            title={search === null ? t("saved-search-add-hint") : undefined}
            onClick={() => {
              setNaming(true);
            }}
          >
            {t("saved-search-add")}
          </button>
        ))}
    </nav>
  );
}
