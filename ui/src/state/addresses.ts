/**
 * Recipient fields: parsing what the user typed into `Address`es and
 * formatting addresses back into editable text.
 */
import type { Address } from "../api/generated";

const EMAIL_RE = /^[^\s@<>",;]+@[^\s@<>",;]+\.[^\s@<>",;]+$/;

/** Whether a bare address looks valid (the core validates for real). */
export function isEmail(value: string): boolean {
  return EMAIL_RE.test(value);
}

/**
 * A plausible email address for an attendee or a contact (looser than
 * {@link isEmail}: no dot needed in the domain; the core validates for real).
 */
export function plausibleEmail(text: string): boolean {
  const at = text.lastIndexOf("@");
  return at > 0 && at < text.length - 1 && !/\s/.test(text) && !text.slice(at + 1).startsWith(".");
}

/** Splits on commas and semicolons outside double quotes and angle brackets. */
function splitList(text: string): string[] {
  const parts: string[] = [];
  let current = "";
  let quoted = false;
  let angle = false;
  for (const c of text) {
    if (c === '"') quoted = !quoted;
    else if (c === "<" && !quoted) angle = true;
    else if (c === ">" && !quoted) angle = false;
    if ((c === "," || c === ";") && !quoted && !angle) {
      parts.push(current);
      current = "";
    } else {
      current += c;
    }
  }
  parts.push(current);
  return parts.map((p) => p.trim()).filter((p) => p !== "");
}

export interface ParsedAddresses {
  addresses: Address[];
  /** Entries that are not valid addresses, as typed. */
  invalid: string[];
}

/** Parses `Ada <ada@example.org>, "Doe, J." <j@x.org>; bob@y.org`. */
export function parseAddressList(text: string): ParsedAddresses {
  const addresses: Address[] = [];
  const invalid: string[] = [];
  for (const part of splitList(text)) {
    const angle = /^(.*)<([^<>]+)>$/.exec(part);
    if (angle) {
      const email = (angle[2] ?? "").trim();
      const name = (angle[1] ?? "")
        .trim()
        .replace(/^"(.*)"$/, "$1")
        .trim();
      if (isEmail(email)) addresses.push({ name: name === "" ? null : name, email });
      else invalid.push(part);
    } else if (isEmail(part)) {
      addresses.push({ name: null, email: part });
    } else {
      invalid.push(part);
    }
  }
  return { addresses, invalid };
}

/** One address as `Name <email>` (quoted when the name needs it) or `email`. */
export function formatAddress(address: Address): string {
  if (!address.name) return address.email;
  const name = /[,;<>"@]/.test(address.name)
    ? `"${address.name.replaceAll('"', "")}"`
    : address.name;
  return `${name} <${address.email}>`;
}

/** Addresses as an editable field value. */
export function formatAddressList(addresses: readonly Address[]): string {
  return addresses.map(formatAddress).join(", ");
}

/** A short display form: the name, or the address. */
export function displayName(address: Address | null | undefined): string {
  return address ? (address.name ?? address.email) : "";
}

/**
 * The Cc of a reply to all: the original's To and Cc without the people
 * answered, the user's own addresses (`own`, lower-cased) and repeats.
 */
export function replyAllCc(
  answered: readonly Address[],
  original: { to: readonly Address[]; cc: readonly Address[] },
  own: ReadonlySet<string>,
): Address[] {
  const seen = new Set(answered.map((a) => a.email.toLowerCase()));
  return [...original.to, ...original.cc].filter((a) => {
    const key = a.email.toLowerCase();
    if (seen.has(key) || own.has(key)) return false;
    seen.add(key);
    return true;
  });
}
