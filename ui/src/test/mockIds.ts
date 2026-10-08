/**
 * Ids of the mock core's demo data (`api/mockData.ts`) and fixed test inputs
 * that many tests name: one definition each.
 */

/** The personal account's Inbox, Sent and Trash folders. */
export const INBOX = "acc-personal/Inbox";
export const SENT = "acc-personal/Sent";
export const TRASH = "acc-personal/Trash";

/** The Inbox's first message. */
export const FIRST = `${INBOX}#1`;

/** A time zone with daylight saving time (PIM tests). */
export const BERLIN = "Europe/Berlin";

/** A PKCS#11 module path (smartcard tests). */
export const PKCS11_MODULE = "C:\\Windows\\System32\\opensc-pkcs11.dll";
