/**
 * Merging contacts (the merge dialog's model): the merged card starts as
 * the primary's fields; single values (display name, organization, title,
 * birthday, …) can be taken from any of the contacts; lists (emails,
 * phones, addresses, web pages, …) are the union without duplicates, each
 * entry can be left out. The core folds contacts of the primary's address
 * book into it and links the others (ADR 0058).
 */
import type { Contact, ContactFields, LabeledValue, PostalAddress } from "../../api/generated";

/** Single-valued fields the dialog lets the user choose. */
export const SINGLE_FIELDS = [
  "display_name",
  "organization",
  "title",
  "role",
  "birthday",
  "anniversary",
  "notes",
  "photo",
] as const;
export type SingleField = (typeof SINGLE_FIELDS)[number];

/** List fields merged as a union. */
export const LIST_FIELDS = ["emails", "phones", "urls", "impp", "related"] as const;
export type ListField = (typeof LIST_FIELDS)[number];

function normalize(field: ListField, value: string): string {
  const v = value.trim().toLowerCase();
  return field === "phones" ? v.replace(/[^\d+]/g, "") : v;
}

function addressKey(a: PostalAddress): string {
  return [a.street, a.locality, a.postal_code, a.country]
    .map((p) => (p ?? "").trim().toLowerCase())
    .join("|");
}

/** The distinct candidate values of a single field (contact ids that have each). */
export function candidates(
  contacts: readonly Contact[],
  field: SingleField,
): { key: string; from: string; value: ContactFields[SingleField] }[] {
  const out: { key: string; from: string; value: ContactFields[SingleField] }[] = [];
  for (const contact of contacts) {
    const value = contact.fields[field];
    if (
      value === null ||
      value === "" ||
      (field === "organization" && !contact.fields.organization?.name)
    )
      continue;
    const key = JSON.stringify(value);
    if (!out.some((c) => c.key === key)) out.push({ key, from: contact.id, value });
  }
  return out;
}

/** Every distinct entry of a list field across the contacts. */
export function listEntries(contacts: readonly Contact[], field: ListField): LabeledValue[] {
  const seen = new Set<string>();
  const out: LabeledValue[] = [];
  for (const contact of contacts) {
    for (const entry of contact.fields[field]) {
      const key = normalize(field, entry.value);
      if (!key || seen.has(key)) continue;
      seen.add(key);
      out.push({ ...entry });
    }
  }
  return out;
}

export function addressEntries(contacts: readonly Contact[]): PostalAddress[] {
  const seen = new Set<string>();
  const out: PostalAddress[] = [];
  for (const contact of contacts) {
    for (const address of contact.fields.addresses) {
      const key = addressKey(address);
      if (seen.has(key)) continue;
      seen.add(key);
      out.push({ ...address });
    }
  }
  return out;
}

/** What the user chose in the dialog. */
export interface MergeChoice {
  primaryId: string;
  /** Single field → key of the chosen candidate (`candidates().key`). */
  single: Partial<Record<SingleField, string>>;
  /** Left-out list entries: field → normalized values. */
  excluded: Partial<Record<ListField | "addresses", string[]>>;
}

export function defaultChoice(contacts: readonly Contact[], primaryId: string): MergeChoice {
  const primary = contacts.find((c) => c.id === primaryId) ?? contacts[0];
  const single: MergeChoice["single"] = {};
  for (const field of SINGLE_FIELDS) {
    const options = candidates(contacts, field);
    const own = options.find((o) => o.from === primary?.id);
    const pick = own ?? options[0];
    if (pick) single[field] = pick.key;
  }
  return { primaryId: primary?.id ?? primaryId, single, excluded: {} };
}

export function entryKey(
  field: ListField | "addresses",
  entry: LabeledValue | PostalAddress,
): string {
  return field === "addresses"
    ? addressKey(entry as PostalAddress)
    : normalize(field, (entry as LabeledValue).value);
}

/** The merged fields for a choice. */
export function mergedFields(contacts: readonly Contact[], choice: MergeChoice): ContactFields {
  const primary = contacts.find((c) => c.id === choice.primaryId) ?? contacts[0];
  if (!primary) throw new Error("nothing to merge");
  const fields: ContactFields = structuredClone(primary.fields);
  const assign = <K extends SingleField>(field: K, value: ContactFields[K]) => {
    fields[field] = value;
  };
  for (const field of SINGLE_FIELDS) {
    const key = choice.single[field];
    const pick = candidates(contacts, field).find((c) => c.key === key);
    if (pick) assign(field, structuredClone<ContactFields[typeof field]>(pick.value));
  }
  for (const field of LIST_FIELDS) {
    const excluded = new Set(choice.excluded[field] ?? []);
    fields[field] = listEntries(contacts, field).filter((e) => !excluded.has(entryKey(field, e)));
  }
  const excludedAddresses = new Set(choice.excluded.addresses ?? []);
  fields.addresses = addressEntries(contacts).filter(
    (a) => !excludedAddresses.has(entryKey("addresses", a)),
  );
  fields.nicknames = [...new Set(contacts.flatMap((c) => c.fields.nicknames))];
  fields.categories = [...new Set(contacts.flatMap((c) => c.fields.categories))];
  const customKeys = new Set<string>();
  fields.custom = contacts
    .flatMap((c) => c.fields.custom)
    .filter((f) => {
      const key = `${f.name}\u0000${f.value}`;
      if (customKeys.has(key)) return false;
      customKeys.add(key);
      return true;
    });
  fields.name ??= contacts.find((c) => c.fields.name)?.fields.name ?? null;
  // Only one preferred entry per list.
  for (const field of LIST_FIELDS) {
    let seen = false;
    fields[field] = fields[field].map((e) => {
      const preferred = e.preferred && !seen;
      if (preferred) seen = true;
      return { ...e, preferred };
    });
  }
  return fields;
}
