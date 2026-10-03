/**
 * The `contact-list` widget: search, address book and group filters, and
 * the contacts as a list box (several can be selected for merging or
 * deleting). The chosen contact is shown by every `contact-card` widget.
 *
 * Keyboard: Up / Down / Home / End move (and show) the contact, Shift +
 * arrows extend the selection, Space toggles a contact in the selection,
 * Enter edits it, Delete deletes the selection.
 * Options: `address-books` (ids), `page-size` (default 200).
 */
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type MouseEvent,
} from "react";
import type { ContactSummary } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { optionList, optionNumber, type WidgetProps } from "../../layout/widgetRegistry";
import { Icon } from "../../widgets/Icon";
import { Problem, problemOf, useChangeContext } from "./common";
import { CONTACT_DIALOG, GROUPS_DIALOG, MERGE_DIALOG } from "./names";
import {
  pimWrite,
  useAddressBooks,
  useContactGroups,
  useContactSearch,
  useContacts,
} from "./queries";
import { usePim, usePimStore } from "./store";

/** Initials for the avatar placeholder. */
export function initialsOf(name: string): string {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const first = words[0]?.[0] ?? "";
  const last = words.length > 1 ? (words[words.length - 1]?.[0] ?? "") : "";
  return (first + last).toUpperCase();
}

export function ContactList({ node }: WidgetProps) {
  const t = useT();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const ids = { search: useId(), book: useId(), group: useId(), list: useId() };
  const fixedBooks = optionList(node, "address-books", []);
  const pageSize = Math.max(20, Math.trunc(optionNumber(node, "page-size", 200)));
  const [query, setQuery] = useState("");
  const [debounced, setDebounced] = useState("");
  const [book, setBook] = useState<string>("");
  const [group, setGroup] = useState<string>("");
  const [limit, setLimit] = useState(pageSize);
  const [problem, setProblem] = useState<ReturnType<typeof problemOf> | null>(null);
  const books = useAddressBooks();
  const groups = useContactGroups();
  const shownBooks = book ? [book] : fixedBooks.length > 0 ? fixedBooks : null;
  const listed = useContacts(shownBooks, group || null, limit);
  const searched = useContactSearch(debounced, shownBooks);
  const current = usePim((s) => s.contactId);
  const selection = usePim((s) => s.selectedContacts);
  const anchorRef = useRef<string | null>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const focusNext = useRef(false);

  useEffect(() => {
    const next = query.trim();
    if (next === debounced) return;
    const timer = setTimeout(() => setDebounced(next), 150);
    return () => clearTimeout(timer);
  }, [query, debounced]);

  const contacts: ContactSummary[] = useMemo(() => {
    if (debounced) {
      const groupMembers = group ? groups.data?.find((g) => g.id === group)?.member_ids : null;
      return (searched.data ?? [])
        .map((m) => m.contact)
        .filter((c) => !groupMembers || groupMembers.includes(c.id));
    }
    return listed.data?.contacts ?? [];
  }, [debounced, searched.data, listed.data, group, groups.data]);

  const total = debounced ? contacts.length : (listed.data?.total ?? 0);
  const order = contacts.map((c) => c.id);
  const active = current && order.includes(current) ? current : (order[0] ?? null);
  const bookName = (id: string) => books.data?.find((b) => b.id === id)?.settings.name ?? id;
  const visibleGroups = (groups.data ?? []).filter((g) => !book || g.address_book_id === book);

  useEffect(() => {
    if (!focusNext.current || !active) return;
    focusNext.current = false;
    listRef.current
      ?.querySelector<HTMLElement>(`[data-contact="${active.replace(/["\\]/g, "")}"]`)
      ?.focus();
  }, [active]);

  const show = (id: string, mode: "single" | "toggle" | "range") => {
    const state = pim.getState();
    if (mode === "toggle") {
      const next = selection.includes(id) ? selection.filter((s) => s !== id) : [...selection, id];
      state.setSelectedContacts(next);
      anchorRef.current = id;
    } else if (mode === "range" && anchorRef.current && order.includes(anchorRef.current)) {
      const a = order.indexOf(anchorRef.current);
      const b = order.indexOf(id);
      state.setSelectedContacts(order.slice(Math.min(a, b), Math.max(a, b) + 1));
    } else {
      state.setSelectedContacts([id]);
      anchorRef.current = id;
    }
    state.showContact(id);
  };

  const onClick = (event: MouseEvent, id: string) => {
    show(id, event.shiftKey ? "range" : event.ctrlKey || event.metaKey ? "toggle" : "single");
  };

  const removeSelection = async () => {
    const targets = selection.length > 0 ? selection : active ? [active] : [];
    if (targets.length === 0) return;
    setProblem(null);
    try {
      await pimWrite(ctx, { type: "DeleteContacts", contact_ids: targets }, "PimDeleted");
      pim.getState().setSelectedContacts([]);
      pim.getState().showContact(null);
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  const onKeyDown = (event: KeyboardEvent<HTMLUListElement>) => {
    if (!active) return;
    const index = order.indexOf(active);
    let next: string | undefined;
    switch (event.key) {
      case "ArrowDown":
        next = order[Math.min(order.length - 1, index + 1)];
        break;
      case "ArrowUp":
        next = order[Math.max(0, index - 1)];
        break;
      case "Home":
        next = order[0];
        break;
      case "End":
        next = order[order.length - 1];
        break;
      case " ":
        event.preventDefault();
        show(active, "toggle");
        return;
      case "Enter":
        event.preventDefault();
        pim.getState().editContact({ mode: "edit", contactId: active });
        ctx.store.getState().openWindow(CONTACT_DIALOG);
        return;
      case "Delete":
        event.preventDefault();
        void removeSelection();
        return;
      default:
        return;
    }
    event.preventDefault();
    if (next) {
      focusNext.current = true;
      show(next, event.shiftKey ? "range" : "single");
    }
  };

  return (
    <section
      data-part="contact-list"
      data-key-context="contacts"
      aria-label={t("pim-contacts-label")}
    >
      <div data-part="contact-list.toolbar">
        <div data-part="form.field" data-field="search">
          <label data-part="form.label" htmlFor={ids.search}>
            {t("pim-contacts-search")}
          </label>
          <input
            id={ids.search}
            data-part="form.input"
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        {fixedBooks.length !== 1 && (
          <div data-part="form.field" data-field="address-book">
            <label data-part="form.label" htmlFor={ids.book}>
              {t("pim-contacts-book")}
            </label>
            <select
              id={ids.book}
              data-part="form.select"
              value={book}
              onChange={(e) => {
                setBook(e.target.value);
                setGroup("");
              }}
            >
              <option value="">{t("pim-contacts-all-books")}</option>
              {(books.data ?? [])
                .filter((b) => fixedBooks.length === 0 || fixedBooks.includes(b.id))
                .map((b) => (
                  <option key={b.id} value={b.id}>
                    {b.role === "collected" ? t("pim-book-collected") : b.settings.name}
                  </option>
                ))}
            </select>
          </div>
        )}
        <div data-part="form.field" data-field="group">
          <label data-part="form.label" htmlFor={ids.group}>
            {t("pim-contacts-group")}
          </label>
          <select
            id={ids.group}
            data-part="form.select"
            value={group}
            onChange={(e) => setGroup(e.target.value)}
          >
            <option value="">{t("pim-contacts-all-groups")}</option>
            {visibleGroups.map((g) => (
              <option key={g.id} value={g.id}>
                {g.name}
              </option>
            ))}
          </select>
        </div>
        <div data-part="contact-list.actions" role="group" aria-label={t("pim-contacts-actions")}>
          <button
            type="button"
            data-part="form.button"
            data-action="new-contact"
            onClick={() => {
              pim
                .getState()
                .editContact({ mode: "new", addressBookId: book || null, fields: null });
              ctx.store.getState().openWindow(CONTACT_DIALOG);
            }}
          >
            <Icon name="plus" part="form.button.icon" />
            {t("pim-contact-new")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="groups"
            onClick={() => ctx.store.getState().openWindow(GROUPS_DIALOG)}
          >
            {t("pim-groups-open")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-action="merge"
            onClick={() => {
              pim.getState().setMergeIds(selection.length >= 2 ? selection : []);
              ctx.store.getState().openWindow(MERGE_DIALOG);
            }}
          >
            {selection.length >= 2
              ? t("pim-merge-selected", { count: selection.length })
              : t("pim-merge-find")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            data-action="delete"
            disabled={selection.length === 0 && !current}
            onClick={() => void removeSelection()}
          >
            {t("pim-contacts-delete", { count: Math.max(1, selection.length) })}
          </button>
        </div>
      </div>
      <Problem message={problem} />
      {(listed.isError || searched.isError) && (
        <Problem message={problemOf(listed.error ?? searched.error)} />
      )}
      <p data-part="contact-list.count" aria-live="polite">
        {t("pim-contacts-count", { count: total })}
      </p>
      {contacts.length === 0 ? (
        <p data-part="contact-list.empty">
          {debounced ? t("pim-contacts-no-match") : t("pim-contacts-empty")}
        </p>
      ) : (
        <ul
          id={ids.list}
          ref={listRef}
          role="listbox"
          aria-multiselectable="true"
          aria-label={t("pim-contacts-list")}
          data-part="contact-list.items"
          onKeyDown={onKeyDown}
        >
          {contacts.map((c) => {
            const selected = selection.includes(c.id);
            const states = [
              selected ? "selected" : null,
              c.id === current ? "current" : null,
              c.read_only ? "read-only" : null,
            ].filter(Boolean);
            return (
              <li
                key={c.id}
                role="option"
                data-part="contact-list.item"
                data-contact={c.id}
                data-state={states.join(" ") || undefined}
                aria-selected={selected}
                tabIndex={c.id === active ? 0 : -1}
                onClick={(e) => onClick(e, c.id)}
                onDoubleClick={() => {
                  pim.getState().editContact({ mode: "edit", contactId: c.id });
                  ctx.store.getState().openWindow(CONTACT_DIALOG);
                }}
              >
                <span data-part="contact-list.avatar" aria-hidden="true">
                  {initialsOf(c.display_name || c.email || "")}
                </span>
                <span data-part="contact-list.name">
                  {c.display_name || c.email || t("pim-contact-unnamed")}
                </span>
                {c.email && <span data-part="contact-list.email">{c.email}</span>}
                {c.organization && (
                  <span data-part="contact-list.organization">{c.organization}</span>
                )}
                {!book && <span data-part="contact-list.book">{bookName(c.address_book_id)}</span>}
                {c.linked_ids.length > 0 && (
                  <span data-part="contact-list.linked">
                    {t("pim-contact-linked-count", { count: c.linked_ids.length })}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
      {!debounced && total > contacts.length && (
        <button
          type="button"
          data-part="form.button"
          data-action="more"
          onClick={() => setLimit((l) => l + pageSize)}
        >
          {t("pim-contacts-more", { count: total - contacts.length })}
        </button>
      )}
    </section>
  );
}
