/**
 * The contact editor (dialog `pim-contact-editor`): address book (new
 * contacts; existing ones can be moved), kind, the name parts and the
 * display name, nicknames, organization, title and role, any number of
 * emails, phones, postal addresses, web pages, chat addresses and related
 * people (each with a kind and "preferred"), birthday and anniversary
 * (the year may be unknown), notes, categories, custom fields and the
 * photo. Unknown vCard properties stay with the card (the core keeps them).
 */
import { CalendarDate } from "@internationalized/date";
import { useId, useState, type ChangeEvent, type SubmitEvent } from "react";
import type {
  AddressBook,
  Contact,
  ContactDate,
  ContactFields,
  ContactKind,
  CustomField,
  LabeledValue,
  LocalizedMessage,
  PostalAddress,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import {
  Problem,
  problemOf,
  useChangeContext,
  EditorMain,
  CategoriesField,
  OptionalSection,
} from "./common";
import { CONTACT_KINDS } from "./constants";
import { DateTimeInput } from "./DateTimeInput";
import { emptyContactFields } from "./defaults";
import { pimWrite, useAddressBooks, useContact } from "./queries";
import { usePim, type ContactEditorTarget } from "./store";
import { localZone, todayIn } from "./time";
import { nonEmpty } from "../../text/empty";
import { EmptyDialog, LoadingDialog } from "../../widgets/DialogStates";
import { TextField, TextAreaField } from "../../widgets/forms";
import { RemoveButton } from "../../widgets/RemoveButton";
import { CloseButton } from "../../widgets/CloseButton";

export const EMAIL_KINDS = ["home", "work", "other"] as const;
export const PHONE_KINDS = ["cell", "home", "work", "fax", "other"] as const;
export const ADDRESS_KINDS = ["home", "work", "other"] as const;
export const URL_KINDS = ["home", "work", "other"] as const;
/** Largest photo the editor accepts (bytes). */
export const MAX_PHOTO_BYTES = 512 * 1024;

/** The display name from the name parts when none was typed. */
export function derivedDisplayName(fields: ContactFields): string {
  if (fields.display_name.trim()) return fields.display_name.trim();
  const n = fields.name;
  const fromName = [n?.prefix, n?.given, n?.additional, n?.family, n?.suffix]
    .filter(Boolean)
    .join(" ")
    .trim();
  return (
    nonEmpty(fromName) ??
    nonEmpty(fields.organization?.name.trim()) ??
    fields.emails[0]?.value ??
    ""
  );
}

/** The fields to save: trimmed, empty entries dropped, a display name set. */
export function cleanContact(fields: ContactFields): ContactFields {
  const values = (list: LabeledValue[]) =>
    list.filter((v) => v.value.trim() !== "").map((v) => ({ ...v, value: v.value.trim() }));
  const addresses = fields.addresses.filter((a) =>
    [a.po_box, a.extended, a.street, a.locality, a.region, a.postal_code, a.country].some(
      (p) => p && p.trim() !== "",
    ),
  );
  const cleaned: ContactFields = {
    ...fields,
    nicknames: fields.nicknames.map((n) => n.trim()).filter(Boolean),
    emails: values(fields.emails),
    phones: values(fields.phones),
    urls: values(fields.urls),
    impp: values(fields.impp),
    related: values(fields.related),
    addresses,
    categories: fields.categories.map((c) => c.trim()).filter(Boolean),
    custom: fields.custom.filter((c) => c.name.trim() !== ""),
    organization: fields.organization?.name.trim() ? fields.organization : null,
    title: nonEmpty(fields.title?.trim()) ?? null,
    role: nonEmpty(fields.role?.trim()) ?? null,
    notes: fields.notes?.trim() ? fields.notes : null,
  };
  return { ...cleaned, display_name: derivedDisplayName(cleaned) };
}

export function ContactEditorDialog({ close }: DialogProps) {
  const target = usePim((s) => s.contactEditor);
  const contact = useContact(target?.mode === "edit" ? target.contactId : null);
  const books = useAddressBooks();
  if (target === null) {
    return (
      <EmptyDialog
        part="pim.editor"
        textPart="pim.editor.empty"
        hintId="pim-editor-nothing"
        close={close}
      />
    );
  }
  if (books.isPending || (target.mode === "edit" && contact.isPending)) {
    return <LoadingDialog part="pim.editor" textPart="pim.editor.loading" />;
  }
  if (contact.isError || books.isError) {
    return (
      <div data-part="pim.editor" data-state="error">
        <Problem message={problemOf(contact.error ?? books.error)} />
        <CloseButton onClose={close} />
      </div>
    );
  }
  return (
    <ContactEditor
      key={target.mode === "edit" ? target.contactId : "new"}
      target={target}
      contact={contact.data ?? null}
      books={books.data}
      close={close}
    />
  );
}

interface EditorProps {
  target: ContactEditorTarget;
  contact: Contact | null;
  books: readonly AddressBook[];
  close: () => void;
}

function emptyAddress(): PostalAddress {
  return {
    kinds: ["home"],
    label: null,
    preferred: false,
    po_box: null,
    extended: null,
    street: null,
    locality: null,
    region: null,
    postal_code: null,
    country: null,
  };
}

export function ContactEditor({ target, contact, books, close }: EditorProps) {
  const t = useT();
  const ctx = useChangeContext();
  const ids = {
    book: useId(),
    kind: useId(),
    display: useId(),
    given: useId(),
    family: useId(),
    additional: useId(),
    prefix: useId(),
    suffix: useId(),
    nick: useId(),
    org: useId(),
    unit: useId(),
    title: useId(),
    role: useId(),
    notes: useId(),
    categories: useId(),
    photo: useId(),
  };
  const writable = books.filter((b) => !b.read_only);
  // Initialized once per editor (the component is keyed by its target).
  const [initial] = useState<ContactFields>(() =>
    structuredClone(
      contact?.fields ?? (target.mode === "new" ? target.fields : null) ?? emptyContactFields(),
    ),
  );
  const [fields, setFields] = useState<ContactFields>(initial);
  const [bookId, setBookId] = useState<string>(
    contact?.address_book_id ??
      (target.mode === "new" ? target.addressBookId : null) ??
      writable.find((b) => b.role === null)?.id ??
      writable[0]?.id ??
      "",
  );
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const update = (patch: Partial<ContactFields>) => {
    setFields((f) => ({ ...f, ...patch }));
  };
  const setName = (key: keyof NonNullable<ContactFields["name"]>, value: string) => {
    setFields((f) => ({
      ...f,
      name: {
        prefix: f.name?.prefix ?? null,
        given: f.name?.given ?? null,
        additional: f.name?.additional ?? null,
        family: f.name?.family ?? null,
        suffix: f.name?.suffix ?? null,
        [key]: value === "" ? null : value,
      },
    }));
  };
  const readOnly = contact?.read_only === true;

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const cleaned = cleanContact(fields);
    if (!cleaned.display_name) {
      setProblem({ id: "pim-issue-missing-name" });
      return;
    }
    setBusy(true);
    setProblem(null);
    try {
      if (contact) {
        if (bookId !== contact.address_book_id) {
          await pimWrite(
            ctx,
            { type: "MoveContact", contact_id: contact.id, address_book_id: bookId },
            "ContactSaved",
          );
        }
        await pimWrite(
          ctx,
          { type: "UpdateContact", contact_id: contact.id, fields: cleaned },
          "ContactSaved",
        );
      } else {
        const saved = await pimWrite(
          ctx,
          { type: "CreateContact", address_book_id: bookId, fields: cleaned },
          "ContactSaved",
        );
        ctx.pim.getState().showContact(saved.contact.id);
      }
      ctx.pim.getState().editContact(null);
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const onPhoto = async (event: ChangeEvent<HTMLInputElement>) => {
    const file = event.target.files?.[0];
    if (!file) return;
    if (file.size > MAX_PHOTO_BYTES || !file.type.startsWith("image/")) {
      setProblem({ id: "pim-contact-photo-too-large" });
      return;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    let binary = "";
    for (const byte of bytes) binary += String.fromCharCode(byte);
    update({ photo: { mime_type: file.type, sha256: null, url: null, data: btoa(binary) } });
  };

  return (
    <form
      data-part="pim.editor"
      data-kind="contact"
      onSubmit={(e) => void submit(e)}
      aria-busy={busy}
      noValidate
    >
      <EditorMain readOnly={readOnly} busy={busy}>
        <div data-part="form.field" data-field="address-book">
          <label data-part="form.label" htmlFor={ids.book}>
            {contact ? t("pim-contact-move-to") : t("pim-contact-book")}
          </label>
          <select
            id={ids.book}
            data-part="form.select"
            value={bookId}
            onChange={(e) => {
              setBookId(e.target.value);
            }}
          >
            {writable.map((b) => (
              <option key={b.id} value={b.id}>
                {b.role === "collected" ? t("pim-book-collected") : b.settings.name}
              </option>
            ))}
          </select>
        </div>
        <div data-part="form.field" data-field="kind">
          <label data-part="form.label" htmlFor={ids.kind}>
            {t("common-kind")}
          </label>
          <select
            id={ids.kind}
            data-part="form.select"
            value={fields.kind}
            onChange={(e) => {
              update({ kind: e.target.value as ContactKind });
            }}
          >
            {CONTACT_KINDS.map((k) => (
              <option key={k} value={k}>
                {t(`pim-contact-kind-${k}`)}
              </option>
            ))}
          </select>
        </div>

        <fieldset data-part="pim.editor.section" data-section="name">
          <legend data-part="form.section.title">{t("common-name")}</legend>
          {(
            [
              ["prefix", ids.prefix, "pim-contact-prefix"],
              ["given", ids.given, "pim-contact-given"],
              ["additional", ids.additional, "pim-contact-additional"],
              ["family", ids.family, "pim-contact-family"],
              ["suffix", ids.suffix, "pim-contact-suffix"],
            ] as const
          ).map(([key, id, label]) => (
            <div key={key} data-part="form.field" data-field={key}>
              <label data-part="form.label" htmlFor={id}>
                {t(label)}
              </label>
              <input
                id={id}
                data-part="form.input"
                value={fields.name?.[key] ?? ""}
                onChange={(e) => {
                  setName(key, e.target.value);
                }}
              />
            </div>
          ))}
          <div data-part="form.field" data-field="display-name">
            <label data-part="form.label" htmlFor={ids.display}>
              {t("pim-contact-display-name")}
            </label>
            <input
              id={ids.display}
              data-part="form.input"
              value={fields.display_name}
              placeholder={derivedDisplayName({ ...fields, display_name: "" })}
              onChange={(e) => {
                update({ display_name: e.target.value });
              }}
            />
          </div>
          <TextField
            id={ids.nick}
            name="nicknames"
            labelId="pim-contact-nicknames"
            value={fields.nicknames.join(", ")}
            onChange={(v) => {
              update({ nicknames: v.split(",") });
            }}
          />
        </fieldset>

        <fieldset data-part="pim.editor.section" data-section="work">
          <legend data-part="form.section.title">{t("common-work")}</legend>
          <div data-part="form.field" data-field="organization">
            <label data-part="form.label" htmlFor={ids.org}>
              {t("pim-contact-organization")}
            </label>
            <input
              id={ids.org}
              data-part="form.input"
              value={fields.organization?.name ?? ""}
              onChange={(e) => {
                update({
                  organization: { name: e.target.value, units: fields.organization?.units ?? [] },
                });
              }}
            />
          </div>
          <div data-part="form.field" data-field="unit">
            <label data-part="form.label" htmlFor={ids.unit}>
              {t("pim-contact-unit")}
            </label>
            <input
              id={ids.unit}
              data-part="form.input"
              value={(fields.organization?.units ?? []).join(", ")}
              onChange={(e) => {
                update({
                  organization: {
                    name: fields.organization?.name ?? "",
                    units: e.target.value
                      .split(",")
                      .map((u) => u.trim())
                      .filter(Boolean),
                  },
                });
              }}
            />
          </div>
          <TextField
            id={ids.title}
            name="title"
            labelId="pim-contact-title"
            value={fields.title ?? ""}
            onChange={(v) => {
              update({ title: v });
            }}
          />
          <TextField
            id={ids.role}
            name="role"
            labelId="pim-contact-role"
            value={fields.role ?? ""}
            onChange={(v) => {
              update({ role: v });
            }}
          />
        </fieldset>

        <LabeledListEditor
          field="emails"
          legend={t("pim-contact-emails")}
          values={fields.emails}
          kinds={EMAIL_KINDS}
          inputType="email"
          onChange={(emails) => {
            update({ emails });
          }}
        />
        <LabeledListEditor
          field="phones"
          legend={t("pim-contact-phones")}
          values={fields.phones}
          kinds={PHONE_KINDS}
          inputType="tel"
          onChange={(phones) => {
            update({ phones });
          }}
        />
        <AddressListEditor
          values={fields.addresses}
          onChange={(addresses) => {
            update({ addresses });
          }}
        />
        <LabeledListEditor
          field="urls"
          legend={t("pim-contact-urls")}
          values={fields.urls}
          kinds={URL_KINDS}
          inputType="url"
          onChange={(urls) => {
            update({ urls });
          }}
        />
        <LabeledListEditor
          field="impp"
          legend={t("pim-contact-impp")}
          values={fields.impp}
          kinds={["other"]}
          inputType="text"
          onChange={(impp) => {
            update({ impp });
          }}
        />

        <ContactDateEditor
          field="birthday"
          label={t("pim-contact-birthday")}
          value={fields.birthday}
          onChange={(birthday) => {
            update({ birthday });
          }}
        />
        <ContactDateEditor
          field="anniversary"
          label={t("pim-contact-anniversary")}
          value={fields.anniversary}
          onChange={(anniversary) => {
            update({ anniversary });
          }}
        />

        <TextAreaField
          id={ids.notes}
          name="notes"
          labelId="common-notes"
          value={fields.notes ?? ""}
          onChange={(v) => {
            update({ notes: v });
          }}
        />
        <CategoriesField
          id={ids.categories}
          categories={fields.categories}
          onChange={(categories) => {
            update({ categories });
          }}
        />
        <CustomFieldsEditor
          values={fields.custom}
          onChange={(custom) => {
            update({ custom });
          }}
        />

        <fieldset data-part="pim.editor.section" data-section="photo">
          <legend data-part="form.section.title">{t("pim-contact-photo")}</legend>
          {fields.photo?.data && (
            <img
              data-part="pim.editor.photo"
              src={`data:${fields.photo.mime_type ?? "image/jpeg"};base64,${fields.photo.data}`}
              alt={t("pim-contact-photo-of", { name: derivedDisplayName(fields) })}
            />
          )}
          <div data-part="form.field" data-field="photo">
            <label data-part="form.label" htmlFor={ids.photo}>
              {t("pim-contact-photo-choose")}
            </label>
            <input
              id={ids.photo}
              data-part="form.file"
              type="file"
              accept="image/*"
              onChange={(e) => void onPhoto(e)}
            />
          </div>
          {fields.photo && (
            <button
              type="button"
              data-part="form.button"
              onClick={() => {
                update({ photo: null });
              }}
            >
              {t("pim-contact-photo-remove")}
            </button>
          )}
        </fieldset>
      </EditorMain>

      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        {!readOnly && (
          <button
            type="submit"
            data-part="form.button"
            data-variant="primary"
            disabled={busy || bookId === ""}
          >
            {t("common-save")}
          </button>
        )}
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            ctx.pim.getState().editContact(null);
            close();
          }}
        >
          {t("common-cancel")}
        </button>
      </div>
    </form>
  );
}

function LabeledListEditor({
  field,
  legend,
  values,
  kinds,
  inputType,
  onChange,
}: {
  field: string;
  legend: string;
  values: LabeledValue[];
  kinds: readonly string[];
  inputType: "email" | "tel" | "url" | "text";
  onChange: (values: LabeledValue[]) => void;
}) {
  const t = useT();
  const id = useId();
  const set = (index: number, patch: Partial<LabeledValue>) => {
    onChange(values.map((v, i) => (i === index ? { ...v, ...patch } : v)));
  };
  return (
    <fieldset data-part="pim.editor.section" data-section={field}>
      <legend data-part="form.section.title">{legend}</legend>
      {values.map((value, index) => {
        const kind = value.kinds[0]?.toLowerCase() ?? "other";
        return (
          <div key={index} data-part="pim.editor.row" data-row={field}>
            <div data-part="form.field" data-field="value">
              <label data-part="form.label" htmlFor={`${id}-v-${String(index)}`}>
                {t("pim-entry-value", { field: legend, index: index + 1 })}
              </label>
              <input
                id={`${id}-v-${String(index)}`}
                data-part="form.input"
                type={inputType}
                value={value.value}
                onChange={(e) => {
                  set(index, { value: e.target.value });
                }}
              />
            </div>
            <div data-part="form.field" data-field="kind">
              <label data-part="form.label" htmlFor={`${id}-k-${String(index)}`}>
                {t("common-kind")}
              </label>
              <select
                id={`${id}-k-${String(index)}`}
                data-part="form.select"
                value={kinds.includes(kind) ? kind : "other"}
                onChange={(e) => {
                  set(index, { kinds: e.target.value === "other" ? [] : [e.target.value] });
                }}
              >
                {kinds.map((k) => (
                  <option key={k} value={k}>
                    {t("pim-kind", { kind: k })}
                  </option>
                ))}
              </select>
            </div>
            <label data-part="form.check" data-field="preferred">
              <input
                type="checkbox"
                checked={value.preferred}
                onChange={(e) => {
                  onChange(
                    values.map((v, i) => ({
                      ...v,
                      preferred:
                        i === index ? e.target.checked : e.target.checked ? false : v.preferred,
                    })),
                  );
                }}
              />
              <span>{t("pim-entry-preferred")}</span>
            </label>
            <RemoveButton
              label={t("pim-entry-remove", { field: legend, index: index + 1 })}
              onRemove={() => {
                onChange(values.filter((_, i) => i !== index));
              }}
            />
          </div>
        );
      })}
      <button
        type="button"
        data-part="form.button"
        data-action={`add-${field}`}
        onClick={() => {
          onChange([
            ...values,
            {
              value: "",
              kinds: kinds[0] && kinds[0] !== "other" ? [kinds[0]] : [],
              label: null,
              preferred: values.length === 0,
            },
          ]);
        }}
      >
        {t("pim-entry-add", { field: legend })}
      </button>
    </fieldset>
  );
}

function AddressListEditor({
  values,
  onChange,
}: {
  values: PostalAddress[];
  onChange: (values: PostalAddress[]) => void;
}) {
  const t = useT();
  const id = useId();
  const set = (index: number, patch: Partial<PostalAddress>) => {
    onChange(values.map((v, i) => (i === index ? { ...v, ...patch } : v)));
  };
  const parts = [
    ["street", "pim-address-street"],
    ["extended", "pim-address-extended"],
    ["po_box", "pim-address-po-box"],
    ["postal_code", "pim-address-postal-code"],
    ["locality", "pim-address-locality"],
    ["region", "pim-address-region"],
    ["country", "pim-address-country"],
  ] as const;
  return (
    <fieldset data-part="pim.editor.section" data-section="addresses">
      <legend data-part="form.section.title">{t("pim-contact-addresses")}</legend>
      {values.map((address, index) => (
        <fieldset key={index} data-part="pim.editor.address">
          <legend data-part="form.label">{t("pim-address-number", { index: index + 1 })}</legend>
          <div data-part="form.field" data-field="kind">
            <label data-part="form.label" htmlFor={`${id}-kind-${String(index)}`}>
              {t("common-kind")}
            </label>
            <select
              id={`${id}-kind-${String(index)}`}
              data-part="form.select"
              value={address.kinds[0]?.toLowerCase() ?? "other"}
              onChange={(e) => {
                set(index, { kinds: e.target.value === "other" ? [] : [e.target.value] });
              }}
            >
              {ADDRESS_KINDS.map((k) => (
                <option key={k} value={k}>
                  {t("pim-kind", { kind: k })}
                </option>
              ))}
            </select>
          </div>
          {parts.map(([key, label]) => (
            <div key={key} data-part="form.field" data-field={key}>
              <label data-part="form.label" htmlFor={`${id}-${key}-${String(index)}`}>
                {t(label)}
              </label>
              <input
                id={`${id}-${key}-${String(index)}`}
                data-part="form.input"
                value={address[key] ?? ""}
                onChange={(e) => {
                  set(index, { [key]: e.target.value === "" ? null : e.target.value });
                }}
              />
            </div>
          ))}
          <RemoveButton
            label={t("pim-address-remove", { index: index + 1 })}
            onRemove={() => {
              onChange(values.filter((_, i) => i !== index));
            }}
          />
        </fieldset>
      ))}
      <button
        type="button"
        data-part="form.button"
        data-action="add-address"
        onClick={() => {
          onChange([...values, emptyAddress()]);
        }}
      >
        {t("pim-address-add")}
      </button>
    </fieldset>
  );
}

function ContactDateEditor({
  field,
  label,
  value,
  onChange,
}: {
  field: string;
  label: string;
  value: ContactDate | null;
  onChange: (value: ContactDate | null) => void;
}) {
  const t = useT();
  const tz = localZone();
  const day = value ? safeDay(value) : null;
  return (
    <OptionalSection
      field={field}
      label={label}
      toggleText={t("pim-contact-date-known", { field: label })}
      set={value !== null}
      onToggle={(on) => {
        if (!on) onChange(null);
        else {
          const today = todayIn(tz);
          onChange({ year: today.year, month: today.month, day: today.day });
        }
      }}
    >
      {value && day && (
        <>
          <DateTimeInput
            label={label}
            field={field}
            day={day}
            time={null}
            tz={tz}
            onChange={(next) => {
              onChange({
                year: value.year === null ? null : next.year,
                month: next.month,
                day: next.day,
              });
            }}
          />
          <label data-part="form.check" data-field={`${field}-no-year`}>
            <input
              type="checkbox"
              checked={value.year === null}
              onChange={(e) => {
                onChange({ ...value, year: e.target.checked ? null : day.year });
              }}
            />
            <span>{t("pim-contact-date-no-year")}</span>
          </label>
        </>
      )}
    </OptionalSection>
  );
}

function safeDay(value: ContactDate): CalendarDate | null {
  try {
    return new CalendarDate(value.year ?? 2000, value.month, value.day);
  } catch {
    return null;
  }
}

function CustomFieldsEditor({
  values,
  onChange,
}: {
  values: CustomField[];
  onChange: (values: CustomField[]) => void;
}) {
  const t = useT();
  const id = useId();
  const set = (index: number, patch: Partial<CustomField>) => {
    onChange(values.map((v, i) => (i === index ? { ...v, ...patch } : v)));
  };
  return (
    <fieldset data-part="pim.editor.section" data-section="custom">
      <legend data-part="form.section.title">{t("pim-contact-custom")}</legend>
      {values.map((field, index) => (
        <div key={index} data-part="pim.editor.row" data-row="custom">
          <div data-part="form.field" data-field="custom-name">
            <label data-part="form.label" htmlFor={`${id}-n-${String(index)}`}>
              {t("pim-custom-name", { index: index + 1 })}
            </label>
            <input
              id={`${id}-n-${String(index)}`}
              data-part="form.input"
              value={field.name}
              onChange={(e) => {
                set(index, { name: e.target.value });
              }}
            />
          </div>
          <div data-part="form.field" data-field="custom-value">
            <label data-part="form.label" htmlFor={`${id}-v-${String(index)}`}>
              {t("pim-custom-value", { index: index + 1 })}
            </label>
            <input
              id={`${id}-v-${String(index)}`}
              data-part="form.input"
              value={field.value}
              onChange={(e) => {
                set(index, { value: e.target.value });
              }}
            />
          </div>
          <RemoveButton
            label={t("pim-custom-remove", { index: index + 1 })}
            onRemove={() => {
              onChange(values.filter((_, i) => i !== index));
            }}
          />
        </div>
      ))}
      <button
        type="button"
        data-part="form.button"
        data-action="add-custom"
        onClick={() => {
          onChange([...values, { name: "", value: "" }]);
        }}
      >
        {t("pim-custom-add")}
      </button>
    </fieldset>
  );
}
