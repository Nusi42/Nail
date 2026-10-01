/**
 * The contact dialogs:
 *
 * - `pim-contact-groups`: the groups of every address book (vCard 4 group
 *   cards or Apple-style groups); create, rename, delete, choose members
 *   (contacts of the same book) and extra addresses.
 * - `pim-merge-contacts`: merge the selected contacts, or pick one of the
 *   duplicates the core found (same email, phone or name); choose the
 *   primary contact and which values the merged card keeps. Contacts in
 *   the primary's address book are folded into it, the others stay in their
 *   books linked to it.
 */
import { useQueries } from "@tanstack/react-query";
import { useId, useMemo, useState, type FormEvent } from "react";
import type {
  AddressBook,
  Contact,
  ContactGroup,
  DuplicateGroup,
  LocalizedMessage,
} from "../../api/generated";
import { useDateFormat } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { Problem, problemOf, useChangeContext } from "./common";
import { addressLines, formatContactDate } from "./ContactCard";
import {
  LIST_FIELDS,
  SINGLE_FIELDS,
  addressEntries,
  candidates,
  defaultChoice,
  entryKey,
  listEntries,
  mergedFields,
  type ListField,
  type MergeChoice,
  type SingleField,
} from "./merge";
import {
  pimKeys,
  pimWrite,
  useAddressBooks,
  useContactGroups,
  useContacts,
  useDuplicates,
} from "./queries";
import { usePim } from "./store";

function bookLabel(
  t: (id: string) => string,
  book: AddressBook | undefined,
  fallback: string,
): string {
  if (!book) return fallback;
  return book.role === "collected" ? t("pim-book-collected") : book.settings.name;
}

// ---------------------------------------------------------------------------
// Groups

export function GroupsDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useChangeContext();
  const books = useAddressBooks();
  const groups = useContactGroups();
  const ids = { name: useId(), book: useId(), newName: useId(), emails: useId() };
  const [selected, setSelected] = useState<string | null>(null);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [newName, setNewName] = useState("");
  const writable = (books.data ?? []).filter((b) => !b.read_only && b.role === null);
  const [chosenBook, setNewBook] = useState<string>("");
  const newBook = chosenBook || writable[0]?.id || "";
  const group = groups.data?.find((g) => g.id === selected) ?? null;

  const create = async (event: FormEvent) => {
    event.preventDefault();
    if (!newName.trim()) {
      setProblem({ id: "pim-issue-missing-name" });
      return;
    }
    setProblem(null);
    try {
      const saved = await pimWrite(
        ctx,
        {
          type: "CreateContactGroup",
          address_book_id: newBook,
          name: newName.trim(),
          member_ids: [],
          member_emails: [],
        },
        "ContactGroupSaved",
      );
      setNewName("");
      setSelected(saved.group.id);
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  return (
    <div data-part="pim.groups">
      <div data-part="pim.groups.list">
        {(books.data ?? []).map((book) => {
          const own = (groups.data ?? []).filter((g) => g.address_book_id === book.id);
          if (own.length === 0) return null;
          return (
            <section
              key={book.id}
              data-part="pim.groups.book"
              aria-label={bookLabel(t, book, book.id)}
            >
              <h3 data-part="pim.groups.book.name">{bookLabel(t, book, book.id)}</h3>
              <ul data-part="pim.groups.items">
                {own.map((g) => (
                  <li key={g.id} data-part="pim.groups.item">
                    <button
                      type="button"
                      data-part="pim.groups.choose"
                      data-group={g.id}
                      aria-pressed={g.id === selected}
                      data-state={g.id === selected ? "selected" : undefined}
                      onClick={() => setSelected(g.id)}
                    >
                      {t("pim-group-entry", {
                        name: g.name,
                        count: g.member_ids.length + g.member_emails.length,
                      })}
                    </button>
                  </li>
                ))}
              </ul>
            </section>
          );
        })}
        {(groups.data ?? []).length === 0 && (
          <p data-part="pim.groups.empty">{t("pim-groups-empty")}</p>
        )}
      </div>
      {group && <GroupEditor key={group.id} group={group} onDeleted={() => setSelected(null)} />}
      <form data-part="pim.groups.new" onSubmit={(e) => void create(e)}>
        <h3 data-part="form.section.title">{t("pim-group-new")}</h3>
        <div data-part="form.field" data-field="book">
          <label data-part="form.label" htmlFor={ids.book}>
            {t("pim-contact-book")}
          </label>
          <select
            id={ids.book}
            data-part="form.select"
            value={newBook}
            onChange={(e) => setNewBook(e.target.value)}
          >
            {writable.map((b) => (
              <option key={b.id} value={b.id}>
                {b.settings.name}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="name">
          <label data-part="form.label" htmlFor={ids.newName}>
            {t("pim-group-name")}
          </label>
          <input
            id={ids.newName}
            data-part="form.input"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
          />
        </div>
        <button type="submit" data-part="form.button" disabled={!newBook}>
          {t("pim-group-create")}
        </button>
      </form>
      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    </div>
  );
}

function GroupEditor({ group, onDeleted }: { group: ContactGroup; onDeleted(): void }) {
  const t = useT();
  const ctx = useChangeContext();
  const ids = { name: useId(), emails: useId() };
  const members = useContacts([group.address_book_id], null, 1000);
  const [name, setName] = useState(group.name);
  const [memberIds, setMemberIds] = useState<string[]>(group.member_ids);
  const [emails, setEmails] = useState(group.member_emails.join("\n"));
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [saved, setSaved] = useState(false);

  const save = async (event: FormEvent) => {
    event.preventDefault();
    setProblem(null);
    setSaved(false);
    try {
      await pimWrite(
        ctx,
        {
          type: "UpdateContactGroup",
          group_id: group.id,
          name: name.trim(),
          member_ids: memberIds,
          member_emails: emails
            .split(/[\s,;]+/)
            .map((e) => e.trim())
            .filter(Boolean),
        },
        "ContactGroupSaved",
      );
      setSaved(true);
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  const remove = async () => {
    setProblem(null);
    try {
      await pimWrite(ctx, { type: "DeleteContactGroup", group_id: group.id }, "PimDeleted");
      onDeleted();
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  return (
    <form
      data-part="pim.groups.editor"
      onSubmit={(e) => void save(e)}
      aria-label={t("pim-group-edit", { name: group.name })}
    >
      <fieldset data-part="pim.editor.main" disabled={group.read_only}>
        <div data-part="form.field" data-field="name">
          <label data-part="form.label" htmlFor={ids.name}>
            {t("pim-group-name")}
          </label>
          <input
            id={ids.name}
            data-part="form.input"
            value={name}
            onChange={(e) => setName(e.target.value)}
          />
        </div>
        <fieldset data-part="pim.groups.members">
          <legend data-part="form.section.title">{t("pim-group-members")}</legend>
          {(members.data?.contacts ?? []).map((c) => (
            <label key={c.id} data-part="form.check" data-contact={c.id}>
              <input
                type="checkbox"
                checked={memberIds.includes(c.id)}
                onChange={(e) =>
                  setMemberIds((ids) =>
                    e.target.checked ? [...ids, c.id] : ids.filter((id) => id !== c.id),
                  )
                }
              />
              <span>{c.display_name || c.email || c.id}</span>
            </label>
          ))}
        </fieldset>
        <div data-part="form.field" data-field="member-emails">
          <label data-part="form.label" htmlFor={ids.emails}>
            {t("pim-group-member-emails")}
          </label>
          <textarea
            id={ids.emails}
            data-part="form.textarea"
            value={emails}
            onChange={(e) => setEmails(e.target.value)}
          />
        </div>
      </fieldset>
      <Problem message={problem} />
      {saved && (
        <p data-part="pim.groups.saved" role="status">
          {t("pim-group-saved")}
        </p>
      )}
      {!group.read_only && (
        <div data-part="pim.editor.actions">
          <button type="submit" data-part="form.button" data-variant="primary">
            {t("pim-save")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            onClick={() => void remove()}
          >
            {t("pim-group-delete")}
          </button>
        </div>
      )}
    </form>
  );
}

// ---------------------------------------------------------------------------
// Merge

function useFullContacts(ids: readonly string[]) {
  const client = useClient();
  const results = useQueries({
    queries: ids.map((id) => ({
      queryKey: pimKeys.contact(id),
      queryFn: async () =>
        (await client.request({ type: "GetContact", contact_id: id }, "PimContact")).contact,
    })),
  });
  return {
    contacts: results.flatMap((r) => (r.data ? [r.data] : [])),
    pending: results.some((r) => r.isPending),
    error: results.find((r) => r.isError)?.error ?? null,
  };
}

export function MergeDialog({ close }: DialogProps) {
  const t = useT();
  const preset = usePim((s) => s.mergeIds);
  const [chosen, setChosen] = useState<string[]>(preset.length >= 2 ? preset : []);
  const duplicates = useDuplicates(chosen.length < 2);
  if (chosen.length >= 2) {
    return (
      <MergeForm
        ids={chosen}
        close={close}
        onBack={preset.length >= 2 ? null : () => setChosen([])}
      />
    );
  }
  return (
    <div data-part="pim.merge" data-state="duplicates">
      <p data-part="pim.merge.intro">{t("pim-merge-duplicates-intro")}</p>
      {duplicates.isPending && <p data-part="pim.merge.loading">{t("pim-loading")}</p>}
      {duplicates.isError && <Problem message={problemOf(duplicates.error)} />}
      {duplicates.data && duplicates.data.length === 0 && (
        <p data-part="pim.merge.none">{t("pim-merge-no-duplicates")}</p>
      )}
      {duplicates.data && duplicates.data.length > 0 && (
        <DuplicateList groups={duplicates.data} onChoose={setChosen} />
      )}
      <div data-part="pim.editor.actions">
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    </div>
  );
}

function DuplicateList({
  groups,
  onChoose,
}: {
  groups: DuplicateGroup[];
  onChoose(ids: string[]): void;
}) {
  const t = useT();
  const all = useMemo(() => [...new Set(groups.flatMap((g) => g.contact_ids))], [groups]);
  const { contacts } = useFullContacts(all);
  const nameOf = (id: string) => contacts.find((c) => c.id === id)?.fields.display_name ?? id;
  return (
    <ul data-part="pim.merge.groups" aria-label={t("pim-merge-duplicates")}>
      {groups.map((g) => (
        <li key={g.contact_ids.join(",")} data-part="pim.merge.group">
          <span data-part="pim.merge.group.names">{g.contact_ids.map(nameOf).join(", ")}</span>
          <span data-part="pim.merge.group.reasons">
            {t("pim-merge-reasons", {
              list: g.reasons.map((r) => t(`pim-duplicate-${r}`)).join(", "),
            })}
          </span>
          <button type="button" data-part="form.button" onClick={() => onChoose(g.contact_ids)}>
            {t("pim-merge-choose")}
          </button>
        </li>
      ))}
    </ul>
  );
}

function MergeForm({
  ids,
  close,
  onBack,
}: {
  ids: string[];
  close(): void;
  onBack: (() => void) | null;
}) {
  const t = useT();
  const { contacts, pending, error } = useFullContacts(ids);
  if (pending) return <p data-part="pim.merge.loading">{t("pim-loading")}</p>;
  if (error || contacts.length < 2) {
    return (
      <div data-part="pim.merge" data-state="error">
        <Problem message={error ? problemOf(error) : { id: "pim-merge-too-few" }} />
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-close")}
        </button>
      </div>
    );
  }
  return <MergeChoices contacts={contacts} close={close} onBack={onBack} />;
}

function MergeChoices({
  contacts,
  close,
  onBack,
}: {
  contacts: Contact[];
  close(): void;
  onBack: (() => void) | null;
}) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const books = useAddressBooks();
  const name = useId();
  const [choice, setChoice] = useState<MergeChoice>(() =>
    defaultChoice(contacts, contacts[0]?.id ?? ""),
  );
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const primary = contacts.find((c) => c.id === choice.primaryId) ?? contacts[0];
  const folded = contacts.filter(
    (c) => c.id !== primary?.id && c.address_book_id === primary?.address_book_id,
  );
  const linked = contacts.filter((c) => c.address_book_id !== primary?.address_book_id);
  const book = (id: string) =>
    bookLabel(
      t,
      books.data?.find((b) => b.id === id),
      id,
    );

  const show = (field: SingleField, value: unknown): string => {
    if (value === null || value === undefined) return "";
    switch (field) {
      case "organization":
        return (value as { name: string }).name;
      case "birthday":
      case "anniversary":
        return formatContactDate(
          format,
          value as { year: number | null; month: number; day: number },
        );
      case "photo":
        return t("pim-merge-photo");
      default:
        return String(value);
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!primary) return;
    setBusy(true);
    setProblem(null);
    try {
      await pimWrite(
        ctx,
        {
          type: "MergeContacts",
          primary_id: primary.id,
          contact_ids: contacts.map((c) => c.id).filter((id) => id !== primary.id),
          fields: mergedFields(contacts, choice),
        },
        "ContactSaved",
      );
      const pim = ctx.pim.getState();
      pim.showContact(primary.id);
      pim.setSelectedContacts([primary.id]);
      pim.setMergeIds([]);
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (field: ListField | "addresses", key: string, keep: boolean) =>
    setChoice((c) => {
      const excluded = new Set(c.excluded[field] ?? []);
      if (keep) excluded.delete(key);
      else excluded.add(key);
      return { ...c, excluded: { ...c.excluded, [field]: [...excluded] } };
    });

  return (
    <form data-part="pim.merge" onSubmit={(e) => void submit(e)} aria-busy={busy}>
      <fieldset data-part="pim.merge.primary">
        <legend data-part="form.section.title">{t("pim-merge-primary")}</legend>
        {contacts.map((c) => (
          <label key={c.id} data-part="form.check" data-contact={c.id}>
            <input
              type="radio"
              name={`${name}-primary`}
              checked={c.id === primary?.id}
              onChange={() =>
                setChoice((prev) => ({ ...defaultChoice(contacts, c.id), excluded: prev.excluded }))
              }
            />
            <span>
              {t("pim-merge-contact", {
                name: c.fields.display_name,
                book: book(c.address_book_id),
              })}
            </span>
          </label>
        ))}
      </fieldset>
      <p data-part="pim.merge.effect">
        {t("pim-merge-effect", { folded: folded.length, linked: linked.length })}
      </p>

      {SINGLE_FIELDS.map((field) => {
        const options = candidates(contacts, field);
        if (options.length < 2) return null;
        return (
          <fieldset key={field} data-part="pim.merge.field" data-field={field}>
            <legend data-part="form.section.title">{t(`pim-merge-field-${field}`)}</legend>
            {options.map((option) => (
              <label key={option.key} data-part="form.check">
                <input
                  type="radio"
                  name={`${name}-${field}`}
                  checked={choice.single[field] === option.key}
                  onChange={() =>
                    setChoice((c) => ({ ...c, single: { ...c.single, [field]: option.key } }))
                  }
                />
                <span>{show(field, option.value)}</span>
              </label>
            ))}
          </fieldset>
        );
      })}

      {LIST_FIELDS.map((field) => {
        const entries = listEntries(contacts, field);
        if (entries.length === 0) return null;
        return (
          <fieldset key={field} data-part="pim.merge.field" data-field={field}>
            <legend data-part="form.section.title">{t(`pim-merge-field-${field}`)}</legend>
            {entries.map((entry) => {
              const key = entryKey(field, entry);
              return (
                <label key={key} data-part="form.check">
                  <input
                    type="checkbox"
                    checked={!(choice.excluded[field] ?? []).includes(key)}
                    onChange={(e) => toggle(field, key, e.target.checked)}
                  />
                  <span>{entry.value}</span>
                </label>
              );
            })}
          </fieldset>
        );
      })}
      {addressEntries(contacts).length > 0 && (
        <fieldset data-part="pim.merge.field" data-field="addresses">
          <legend data-part="form.section.title">{t("pim-merge-field-addresses")}</legend>
          {addressEntries(contacts).map((address) => {
            const key = entryKey("addresses", address);
            return (
              <label key={key} data-part="form.check">
                <input
                  type="checkbox"
                  checked={!(choice.excluded.addresses ?? []).includes(key)}
                  onChange={(e) => toggle("addresses", key, e.target.checked)}
                />
                <span>{addressLines(address).join(", ")}</span>
              </label>
            );
          })}
        </fieldset>
      )}

      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {t("pim-merge-submit", { count: contacts.length })}
        </button>
        {onBack && (
          <button type="button" data-part="form.button" onClick={onBack}>
            {t("pim-merge-back")}
          </button>
        )}
        <button type="button" data-part="form.button" onClick={close}>
          {t("pim-cancel")}
        </button>
      </div>
    </form>
  );
}
