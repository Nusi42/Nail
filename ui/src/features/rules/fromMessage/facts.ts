/**
 * "Values from this mail" (R2, ADR 0222): when the rules window was opened
 * from a message (its `origin` in the rules store), or a message is
 * selected, a condition's or an action's value field offers that message's
 * values — sender address, name and domain, recipients, reply-to, subject
 * (whole or one word), mailing list, account, folder, header names, tags.
 * The values come from the core (`GetMessageFacts`); this module only
 * picks those that fit a field and puts the picked one into the condition
 * or action (with the comparison and the address field that fit it).
 */
import { useQuery } from "@tanstack/react-query";
import type { NailClient } from "../../../api/client";
import type {
  Account,
  Address,
  AddressField,
  Condition,
  Folder,
  FolderRef,
  MessageFacts,
  MessageId,
  RuleAction,
  TextMatch,
  TextOp,
} from "../../../api/generated";
import { useClient } from "../../../state/queries";

/** What kind of value a suggestion is (its label in the list). */
export const MESSAGE_VALUE_KINDS = [
  "address",
  "name",
  "domain",
  "subject",
  "subject-word",
  "mailing-list",
  "account",
  "folder",
  "header-name",
  "tag",
] as const;

export type MessageValueKind = (typeof MESSAGE_VALUE_KINDS)[number];

/** The address fields of a message, in the order the list shows them. */
const ADDRESS_SOURCES = [
  "from",
  "sender",
  "reply-to",
  "to",
  "cc",
] as const satisfies readonly AddressField[];

type AddressSource = (typeof ADDRESS_SOURCES)[number];

/**
 * Where in the message a value comes from: an address field, or one of
 * the message's other parts. The list groups the values by it.
 */
export const MESSAGE_VALUE_SOURCES = [
  ...ADDRESS_SOURCES,
  "subject",
  "list-id",
  "account",
  "folder",
  "header",
  "tag",
] as const;

export type MessageValueSource = (typeof MESSAGE_VALUE_SOURCES)[number];

/** One value of the message a condition or action field can take. */
export interface MessageValue {
  kind: MessageValueKind;
  /** Where in the message it comes from. */
  source: MessageValueSource;
  /** What the field takes. */
  value: string;
  /** What the list shows (the value, or a name with its address). */
  label: string;
  /** The comparison that fits the value (text fields). */
  op: TextOp;
}

/**
 * What the facts alone do not hold: the names of the accounts, the folders
 * of the message's account, and the message's tags (from its summary, when
 * the list has it).
 */
export interface MessageValueContext {
  accounts?: readonly Account[] | undefined;
  folders?: readonly Folder[] | undefined;
  tags?: readonly string[] | undefined;
}

/** Most addresses of one field the list offers (a mail to a crowd). */
export const MAX_ADDRESSES_PER_FIELD = 10;

/** Most subject words offered. */
export const MAX_SUBJECT_WORDS = 12;

/** The shortest subject word worth a rule (in characters). */
export const MIN_SUBJECT_WORD = 3;

export const messageFactsKeys = {
  facts: (messageId: MessageId) => ["message-facts", messageId] as const,
};

/** The values of one message rules can test. */
export async function getMessageFacts(
  client: NailClient,
  messageId: MessageId,
): Promise<MessageFacts> {
  return (await client.request({ type: "GetMessageFacts", message_id: messageId }, "MessageFacts"))
    .facts;
}

/** The values of the message, `null` id = no message (the query stays idle). */
export function useMessageFacts(messageId: MessageId | null) {
  const client = useClient();
  return useQuery({
    queryKey: messageFactsKeys.facts(messageId ?? ""),
    queryFn: () => getMessageFacts(client, messageId ?? ""),
    enabled: messageId !== null,
  });
}

/** The domain of an address (`ada@mail.example.org` → `mail.example.org`). */
export function domainOf(email: string): string | null {
  const at = email.lastIndexOf("@");
  const domain = at < 0 ? "" : email.slice(at + 1).trim();
  return domain === "" ? null : domain;
}

/**
 * The words of a subject worth a `contains` test: letters and digits with
 * inner punctuation (`nail-users`, `v1.2`), at least
 * {@link MIN_SUBJECT_WORD} long, each once (case-insensitive), at most
 * {@link MAX_SUBJECT_WORDS}.
 */
export function subjectWords(subject: string): string[] {
  const seen = new Set<string>();
  const words: string[] = [];
  for (const raw of subject.split(/\s+/u)) {
    const word = raw.replace(/^[^\p{L}\p{N}]+|[^\p{L}\p{N}]+$/gu, "");
    const key = word.toLowerCase();
    if (word.length < MIN_SUBJECT_WORD || seen.has(key)) continue;
    seen.add(key);
    words.push(word);
    if (words.length === MAX_SUBJECT_WORDS) break;
  }
  return words;
}

function addressesOf(facts: MessageFacts, source: AddressSource): readonly Address[] {
  switch (source) {
    case "from":
      return facts.from;
    case "sender":
      return facts.sender;
    case "reply-to":
      return facts.reply_to;
    case "to":
      return facts.to;
    case "cc":
      return facts.cc;
  }
}

/** Address, name and domain of each address of one field, each once. */
function addressValues(facts: MessageFacts, source: AddressSource): MessageValue[] {
  const out: MessageValue[] = [];
  const add = (kind: MessageValueKind, value: string, label: string, op: TextOp) => {
    const key = value.toLowerCase();
    if (out.some((v) => v.kind === kind && v.value.toLowerCase() === key)) return;
    out.push({ kind, source, value, label, op });
  };
  const addresses = addressesOf(facts, source).slice(0, MAX_ADDRESSES_PER_FIELD);
  for (const address of addresses) {
    const name = address.name?.trim() ?? "";
    const label = name === "" ? address.email : `${name} <${address.email}>`;
    add("address", address.email, label, "equals");
    if (name !== "") add("name", name, name, "equals");
  }
  for (const address of addresses) {
    const domain = domainOf(address.email);
    // `ends-with "@example.org"`: that domain, not a longer one ending in
    // the same letters.
    if (domain !== null) add("domain", `@${domain}`, domain, "ends-with");
  }
  return out;
}

function value(kind: MessageValueKind, source: MessageValueSource, text: string): MessageValue {
  return { kind, source, value: text, label: text, op: "equals" };
}

function subjectValues(facts: MessageFacts): MessageValue[] {
  const subject = facts.subject.trim();
  if (subject === "") return [];
  return [
    value("subject", "subject", subject),
    ...subjectWords(subject).map((word): MessageValue => ({
      ...value("subject-word", "subject", word),
      op: "contains",
    })),
  ];
}

function listValues(facts: MessageFacts): MessageValue[] {
  const id = facts.list_id ?? "";
  // Like "Create rule from message": `list-id contains="<id>"`.
  return id === "" ? [] : [{ ...value("mailing-list", "list-id", id), op: "contains" }];
}

function accountValues(facts: MessageFacts, context: MessageValueContext): MessageValue[] {
  const account = context.accounts?.find((a) => a.id === facts.account_id);
  const label = account ? account.name || account.email || account.id : facts.account_id;
  return [{ ...value("account", "account", facts.account_id), label }];
}

function folderValues(facts: MessageFacts, context: MessageValueContext): MessageValue[] {
  const folder = context.folders?.find((f) => f.id === facts.folder_id);
  return folder ? [value("folder", "folder", folder.path)] : [];
}

function tagValues(context: MessageValueContext): MessageValue[] {
  return (context.tags ?? []).map((tag) => value("tag", "tag", tag));
}

/** The address fields an address condition's field covers. */
function fieldSources(field: AddressField): readonly AddressSource[] {
  return field === "recipients" ? ["to", "cc"] : [field];
}

function addressSourceOf(text: string): AddressSource | undefined {
  const lower = text.trim().toLowerCase();
  return ADDRESS_SOURCES.find((s) => s === lower);
}

/** The values of a header the facts know by name (none for other headers). */
function headerValues(facts: MessageFacts, name: string): MessageValue[] {
  const address = addressSourceOf(name);
  const lower = name.trim().toLowerCase();
  const values =
    address !== undefined
      ? addressValues(facts, address)
      : lower === "subject"
        ? subjectValues(facts)
        : lower === "list-id"
          ? listValues(facts)
          : [];
  // A header test sees the raw field (`Ada <ada@example.org>`): only
  // `contains` finds a part of it.
  return values.map((v) => ({ ...v, op: "contains" }));
}

function headerNameValues(facts: MessageFacts): MessageValue[] {
  return facts.header_names.map((name) => value("header-name", "header", name));
}

/**
 * The values a condition's field can take from the message. Address
 * conditions offer every address field (the condition's own first: picking
 * another one switches the field); header conditions offer the header's
 * values where the facts know them, then the message's header names.
 * Groups and conditions without such a field offer nothing.
 */
export function valuesForCondition(
  facts: MessageFacts,
  condition: Condition,
  context: MessageValueContext = {},
): MessageValue[] {
  switch (condition.type) {
    case "address": {
      const own = fieldSources(condition.field);
      const order = [...own, ...ADDRESS_SOURCES.filter((s) => !own.includes(s))];
      return order.flatMap((source) => addressValues(facts, source));
    }
    case "subject":
      return subjectValues(facts);
    case "list-id":
      return listValues(facts);
    case "header":
      return [...headerValues(facts, condition.name), ...headerNameValues(facts)];
    case "account":
      return accountValues(facts, context);
    case "folder":
      return folderValues(facts, context);
    case "tag":
      return tagValues(context);
    case "all":
    case "any":
    case "not":
    case "body":
    case "query":
    case "attachment":
    case "size":
    case "date":
    case "age":
    case "flag":
    case "auth":
      return [];
  }
}

/**
 * The values an action's field can take from the message: the message's
 * folder for move / copy, its tags for tag / untag.
 */
export function valuesForAction(
  facts: MessageFacts,
  action: RuleAction,
  context: MessageValueContext = {},
): MessageValue[] {
  switch (action.type) {
    case "move":
    case "copy":
      return folderValues(facts, context);
    case "tag":
    case "untag":
      return tagValues(context);
    case "delete":
    case "archive":
    case "flag":
    case "mark-read":
    case "forward":
    case "redirect":
    case "reply":
    case "notify":
    case "stop":
      return [];
  }
}

function matchOf(current: TextMatch | null, picked: MessageValue): TextMatch {
  return { op: picked.op, value: picked.value, case_sensitive: current?.case_sensitive ?? false };
}

/** A folder reference to the picked path, keeping the reference's account. */
function folderRefTo(current: FolderRef, path: string): FolderRef {
  return { type: "path", path, account_id: current.account_id };
}

/**
 * The condition with the picked value: its text, the comparison that fits
 * (an address's domain is `ends-with "@domain"`), and for address
 * conditions the field the value came from (unless the condition's field
 * already covers it, like "recipients" for To and Cc). A header name
 * becomes the header condition's name.
 */
export function applyToCondition(condition: Condition, picked: MessageValue): Condition {
  switch (condition.type) {
    case "address": {
      const source = addressSourceOf(picked.source);
      const field =
        source === undefined || fieldSources(condition.field).includes(source)
          ? condition.field
          : source;
      return { ...condition, field, matches: matchOf(condition.matches, picked) };
    }
    case "subject":
    case "list-id":
      return { ...condition, matches: matchOf(condition.matches, picked) };
    case "header":
      return picked.kind === "header-name"
        ? { ...condition, name: picked.value }
        : { ...condition, matches: matchOf(condition.matches, picked) };
    case "account":
      return { ...condition, account_id: picked.value };
    case "folder":
      return { ...condition, folder: folderRefTo(condition.folder, picked.value) };
    case "tag":
      return { ...condition, tag: picked.value };
    default:
      return condition;
  }
}

/** The action with the picked value (a folder path or a tag). */
export function applyToAction(action: RuleAction, picked: MessageValue): RuleAction {
  switch (action.type) {
    case "move":
    case "copy":
      return { ...action, folder: folderRefTo(action.folder, picked.value) };
    case "tag":
    case "untag":
      return { ...action, tag: picked.value };
    default:
      return action;
  }
}
