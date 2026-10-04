/**
 * The `contact-card` widget: the contact chosen in a `contact-list` (or
 * set with `contacts.show`): photo, names, organization, every email,
 * phone, address, web page and custom field, dates, notes, groups, the
 * contacts it is linked with in other address books (merged persons; each
 * can be unlinked), its sync state (a conflict can be resolved), and the
 * upcoming meetings with this person (design: "A contact view shows recent
 * mail and upcoming meetings with that person").
 */
import { colorSlot } from "../../theme/skin";
import type { CalendarDate } from "@internationalized/date";
import { useState } from "react";
import type {
  Contact,
  ContactDate,
  LabeledValue,
  LocalizedMessage,
  PostalAddress,
} from "../../api/generated";
import { useDateFormat, type DateFormatter } from "../../i18n/format";
import { useT } from "../../i18n/I18nProvider";
import { Problem, problemOf, useChangeContext } from "./common";
import { initialsOf } from "./ContactList";
import { formatDay, formatPimTime } from "./fmt";
import { CONTACT_DIALOG } from "./names";
import {
  pimWrite,
  useAddressBooks,
  useContact,
  useContactGroups,
  useContactPhoto,
  usePimSearch,
} from "./queries";
import { usePim } from "./store";
import { localZone } from "./time";
import { CalendarDate as CalendarDateValue } from "@internationalized/date";
import { hasText } from "../../text/empty";

/** A contact date for people (`10 December 1815`, or without the year). */
export function formatContactDate(format: DateFormatter, date: ContactDate): string {
  try {
    const day: CalendarDate = new CalendarDateValue(date.year ?? 2000, date.month, date.day);
    return date.year === null
      ? formatDay(format, day, { day: "numeric", month: "long" })
      : formatDay(format, day, { dateStyle: "long" });
  } catch {
    return `${String(date.month)}/${String(date.day)}`;
  }
}

/** The lines of a postal address (empty parts left out). */
export function addressLines(address: PostalAddress): string[] {
  return [
    address.po_box,
    address.extended,
    address.street,
    [address.postal_code, address.locality].filter(Boolean).join(" "),
    address.region,
    address.country,
  ].filter((line): line is string => !!line && line.trim() !== "");
}

function KindLabel({ kinds, label }: { kinds: readonly string[]; label: string | null }) {
  const t = useT();
  const text = label ?? (kinds[0] ? t("pim-kind", { kind: kinds[0].toLowerCase() }) : "");
  if (!text) return null;
  return <span data-part="contact-card.kind">{text}</span>;
}

function ValueList({
  title,
  values,
  field,
  href,
}: {
  title: string;
  values: readonly LabeledValue[];
  field: string;
  href?: (value: string) => string | null;
}) {
  if (values.length === 0) return null;
  return (
    <div data-part="contact-card.section" data-field={field}>
      <h3 data-part="contact-card.heading">{title}</h3>
      <ul data-part="contact-card.values">
        {values.map((v, index) => {
          const link = href?.(v.value) ?? null;
          return (
            <li
              key={`${v.value}-${String(index)}`}
              data-part="contact-card.value"
              data-state={v.preferred ? "preferred" : undefined}
            >
              <KindLabel kinds={v.kinds} label={v.label} />
              {link ? (
                <a
                  data-part="contact-card.link"
                  href={link}
                  target="_blank"
                  rel="noreferrer noopener"
                >
                  {v.value}
                </a>
              ) : (
                <span data-part="contact-card.text">{v.value}</span>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

export function ContactCard() {
  const t = useT();
  const id = usePim((s) => s.contactId);
  const contact = useContact(id);
  if (id === null) {
    return (
      <section data-part="contact-card" data-state="empty" aria-label={t("pim-contact-card-label")}>
        <p data-part="contact-card.empty">{t("pim-contact-none")}</p>
      </section>
    );
  }
  if (contact.isPending) {
    return (
      <section
        data-part="contact-card"
        data-state="loading"
        aria-busy="true"
        aria-label={t("pim-contact-card-label")}
      >
        <p data-part="contact-card.loading">{t("pim-loading")}</p>
      </section>
    );
  }
  if (contact.isError) {
    return (
      <section data-part="contact-card" data-state="error" aria-label={t("pim-contact-card-label")}>
        <Problem message={problemOf(contact.error)} />
      </section>
    );
  }
  return <ContactDetails contact={contact.data} />;
}

function ContactDetails({ contact }: { contact: Contact }) {
  const t = useT();
  const format = useDateFormat();
  const ctx = useChangeContext();
  const f = contact.fields;
  const photo = useContactPhoto(contact.id, f.photo !== null);
  const books = useAddressBooks();
  const groups = useContactGroups();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const primaryEmail = f.emails.find((e) => e.preferred)?.value ?? f.emails[0]?.value ?? "";
  const meetings = usePimSearch(primaryEmail);
  const book = books.data?.find((b) => b.id === contact.address_book_id);
  const name = f.display_name || primaryEmail || t("pim-contact-unnamed");
  const tz = localZone();

  const run = async (
    request: Parameters<typeof pimWrite>[1],
    expected: Parameters<typeof pimWrite>[2],
  ) => {
    setProblem(null);
    try {
      await pimWrite(ctx, request, expected);
    } catch (error) {
      setProblem(problemOf(error));
    }
  };

  return (
    <section
      data-part="contact-card"
      data-contact={contact.id}
      aria-label={t("pim-contact-card-of", { name })}
    >
      <header data-part="contact-card.header">
        {photo.data ? (
          <img
            data-part="contact-card.photo"
            src={photo.data}
            alt={t("pim-contact-photo-of", { name })}
          />
        ) : (
          <span
            data-part="contact-card.avatar"
            data-color={colorSlot(primaryEmail || name)}
            aria-hidden="true"
          >
            {initialsOf(name)}
          </span>
        )}
        <div data-part="contact-card.names">
          <h2 data-part="contact-card.name">{name}</h2>
          {f.nicknames.length > 0 && (
            <p data-part="contact-card.nicknames">{f.nicknames.join(", ")}</p>
          )}
          {(hasText(f.title) || f.organization !== null) && (
            <p data-part="contact-card.organization">
              {[f.title, f.role, f.organization?.name, ...(f.organization?.units ?? [])]
                .filter(Boolean)
                .join(" · ")}
            </p>
          )}
          <p data-part="contact-card.book">
            {t("pim-contact-in-book", {
              book:
                book?.role === "collected"
                  ? t("pim-book-collected")
                  : (book?.settings.name ?? contact.address_book_id),
              version: contact.version,
            })}
          </p>
        </div>
        <div data-part="contact-card.actions" role="group" aria-label={t("pim-contact-actions")}>
          {!contact.read_only && (
            <button
              type="button"
              data-part="form.button"
              data-action="edit"
              onClick={() => {
                ctx.pim.getState().editContact({ mode: "edit", contactId: contact.id });
                ctx.store.getState().openWindow(CONTACT_DIALOG);
              }}
            >
              {t("pim-edit")}
            </button>
          )}
          {!contact.read_only && (
            <button
              type="button"
              data-part="form.button"
              data-variant="danger"
              data-action="delete"
              onClick={() => {
                void run({ type: "DeleteContacts", contact_ids: [contact.id] }, "PimDeleted").then(
                  () => {
                    ctx.pim.getState().showContact(null);
                  },
                );
              }}
            >
              {t("pim-delete")}
            </button>
          )}
        </div>
      </header>

      {contact.sync !== "synced" && (
        <div data-part="contact-card.sync" data-state={contact.sync}>
          <p data-part="contact-card.sync.text">{t(`pim-sync-${contact.sync}`)}</p>
          {contact.sync === "conflict" && (
            <>
              <button
                type="button"
                data-part="form.button"
                onClick={() =>
                  void run(
                    {
                      type: "ResolvePimConflict",
                      item: { type: "contact", contact_id: contact.id },
                      keep: "server",
                    },
                    "Done",
                  )
                }
              >
                {t("pim-conflict-keep-server")}
              </button>
              <button
                type="button"
                data-part="form.button"
                onClick={() =>
                  void run(
                    {
                      type: "ResolvePimConflict",
                      item: { type: "contact", contact_id: contact.id },
                      keep: "local",
                    },
                    "Done",
                  )
                }
              >
                {t("pim-conflict-keep-local")}
              </button>
            </>
          )}
        </div>
      )}
      <Problem message={problem} />

      <ValueList title={t("pim-contact-emails")} values={f.emails} field="emails" />
      <ValueList title={t("pim-contact-phones")} values={f.phones} field="phones" />
      {f.addresses.length > 0 && (
        <div data-part="contact-card.section" data-field="addresses">
          <h3 data-part="contact-card.heading">{t("pim-contact-addresses")}</h3>
          <ul data-part="contact-card.values">
            {f.addresses.map((address, index) => (
              <li
                key={index}
                data-part="contact-card.value"
                data-state={address.preferred ? "preferred" : undefined}
              >
                <KindLabel kinds={address.kinds} label={address.label} />
                <address data-part="contact-card.address">
                  {addressLines(address).map((line, i) => (
                    <span key={i} data-part="contact-card.address.line">
                      {line}
                    </span>
                  ))}
                </address>
              </li>
            ))}
          </ul>
        </div>
      )}
      <ValueList
        title={t("pim-contact-urls")}
        values={f.urls}
        field="urls"
        href={(value) => (/^https?:\/\//i.test(value) ? value : null)}
      />
      <ValueList title={t("pim-contact-impp")} values={f.impp} field="impp" />
      <ValueList title={t("pim-contact-related")} values={f.related} field="related" />
      {(f.birthday !== null || f.anniversary !== null) && (
        <div data-part="contact-card.section" data-field="dates">
          <h3 data-part="contact-card.heading">{t("pim-contact-dates")}</h3>
          <dl data-part="contact-card.dates">
            {f.birthday && (
              <>
                <dt data-part="contact-card.term">{t("pim-contact-birthday")}</dt>
                <dd data-part="contact-card.detail">{formatContactDate(format, f.birthday)}</dd>
              </>
            )}
            {f.anniversary && (
              <>
                <dt data-part="contact-card.term">{t("pim-contact-anniversary")}</dt>
                <dd data-part="contact-card.detail">{formatContactDate(format, f.anniversary)}</dd>
              </>
            )}
          </dl>
        </div>
      )}
      {f.notes && (
        <div data-part="contact-card.section" data-field="notes">
          <h3 data-part="contact-card.heading">{t("pim-contact-notes")}</h3>
          <p data-part="contact-card.notes">{f.notes}</p>
        </div>
      )}
      {f.categories.length > 0 && (
        <p data-part="contact-card.categories">
          {t("pim-categories-list", { list: f.categories.join(", ") })}
        </p>
      )}
      {f.custom.length > 0 && (
        <div data-part="contact-card.section" data-field="custom">
          <h3 data-part="contact-card.heading">{t("pim-contact-custom")}</h3>
          <dl data-part="contact-card.custom">
            {f.custom.map((field, index) => (
              <div key={index} data-part="contact-card.custom.item">
                <dt data-part="contact-card.term">{field.name}</dt>
                <dd data-part="contact-card.detail">{field.value}</dd>
              </div>
            ))}
          </dl>
        </div>
      )}
      {contact.group_ids.length > 0 && (
        <p data-part="contact-card.groups">
          {t("pim-contact-groups-list", {
            list: contact.group_ids
              .map((g) => groups.data?.find((x) => x.id === g)?.name ?? g)
              .join(", "),
          })}
        </p>
      )}
      {contact.linked_ids.length > 0 && (
        <LinkedContacts
          contact={contact}
          onUnlink={(target) =>
            void run({ type: "UnlinkContact", contact_id: target }, "ContactSaved")
          }
        />
      )}
      {primaryEmail && (meetings.data ?? []).length > 0 && (
        <div data-part="contact-card.section" data-field="meetings">
          <h3 data-part="contact-card.heading">{t("pim-contact-meetings")}</h3>
          <ul data-part="contact-card.values">
            {(meetings.data ?? []).map((hit) => (
              <li key={JSON.stringify(hit.item)} data-part="contact-card.value">
                <span data-part="contact-card.text">{hit.title}</span>
                {hit.start && (
                  <span data-part="contact-card.kind">{formatPimTime(format, hit.start, tz)}</span>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}
    </section>
  );
}

function LinkedContacts({
  contact,
  onUnlink,
}: {
  contact: Contact;
  onUnlink: (id: string) => void;
}) {
  const t = useT();
  const books = useAddressBooks();
  return (
    <div data-part="contact-card.section" data-field="linked">
      <h3 data-part="contact-card.heading">{t("pim-contact-linked")}</h3>
      <ul data-part="contact-card.values">
        {contact.linked_ids.map((linked) => (
          <LinkedItem
            key={linked}
            id={linked}
            books={books.data ?? []}
            onUnlink={() => {
              onUnlink(linked);
            }}
          />
        ))}
      </ul>
    </div>
  );
}

function LinkedItem({
  id,
  books,
  onUnlink,
}: {
  id: string;
  books: readonly { id: string; settings: { name: string } }[];
  onUnlink: () => void;
}) {
  const t = useT();
  const linked = useContact(id);
  const name = linked.data?.fields.display_name ?? id;
  const book = books.find((b) => b.id === linked.data?.address_book_id)?.settings.name ?? "";
  return (
    <li data-part="contact-card.value" data-contact={id}>
      <span data-part="contact-card.text">{t("pim-contact-linked-item", { name, book })}</span>
      <button
        type="button"
        data-part="form.button"
        onClick={onUnlink}
        aria-label={t("pim-contact-unlink-label", { name })}
      >
        {t("pim-contact-unlink")}
      </button>
    </li>
  );
}
