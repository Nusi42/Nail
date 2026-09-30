/**
 * Pure helpers of the sieve feature, mirroring `nail_api::sieve` (ADR 0043):
 * the vacation defaults and validation (`VacationSettings::issues`) and the
 * state names the UI shows.
 */
import type {
  RulePlacement,
  SieveState,
  VacationIssue,
  VacationSettings,
} from "../../api/generated";

export const SIEVE_STATES: readonly SieveState[] = [
  "disabled",
  "unavailable",
  "pending",
  "uploading",
  "in-sync",
  "failed",
];

export const RULE_PLACEMENTS: readonly RulePlacement[] = ["server", "client", "not-expressible"];

export const VACATION_ISSUES: readonly VacationIssue[] = [
  "empty-body",
  "days",
  "date",
  "end-before-start",
  "address",
  "subject",
];

/** `VacationSettings::default()`. */
export function defaultVacation(): VacationSettings {
  return {
    enabled: false,
    subject: "",
    body: "",
    start_date: null,
    end_date: null,
    days: 7,
    addresses: [],
    from: null,
    skip_lists: true,
  };
}

/** `nail_api::sieve::is_iso_date`. */
export function isIsoDate(text: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (!match) return false;
  const month = Number(match[2]);
  const day = Number(match[3]);
  return month >= 1 && month <= 12 && day >= 1 && day <= 31;
}

function badAddress(address: string): boolean {
  const a = address.trim();
  return a === "" || !a.includes("@") || /\s/.test(a);
}

/** `VacationSettings::issues`, in the same order. */
export function vacationIssues(v: VacationSettings): VacationIssue[] {
  const out: VacationIssue[] = [];
  if (v.enabled && v.body.trim() === "") out.push("empty-body");
  if (v.days === 0 || v.days > 365) out.push("days");
  const start = v.start_date;
  const end = v.end_date;
  if ((start !== null && !isIsoDate(start)) || (end !== null && !isIsoDate(end))) {
    out.push("date");
  } else if (start !== null && end !== null && end < start) {
    out.push("end-before-start");
  }
  if (v.addresses.some(badAddress) || (v.from !== null && badAddress(v.from))) {
    out.push("address");
  }
  if (/[\r\n]/.test(v.subject)) out.push("subject");
  return out;
}

/** Slashes and control characters (`char::is_control`), not allowed in script names. */
// eslint-disable-next-line no-control-regex -- control characters are what it looks for.
const SCRIPT_NAME_FORBIDDEN = /[/\\\u0000-\u001f\u007f-\u009f]/;

/** `ManageSieveConfig::issues` (host, port, script name), in the same order. */
export function manageSieveIssues(c: {
  host: string | null;
  port: number | null;
  script_name: string | null;
}): ("host" | "port" | "script-name")[] {
  const out: ("host" | "port" | "script-name")[] = [];
  if (c.host !== null && (c.host.trim() === "" || /\s/.test(c.host))) out.push("host");
  if (c.port !== null && (c.port < 1 || c.port > 65535 || !Number.isInteger(c.port))) {
    out.push("port");
  }
  const name = c.script_name;
  if (
    name !== null &&
    (name.trim() === "" || [...name].length > 128 || SCRIPT_NAME_FORBIDDEN.test(name))
  ) {
    out.push("script-name");
  }
  return out;
}

/** Addresses typed as a comma / whitespace separated list. */
export function parseAddressList(text: string): string[] {
  return text
    .split(/[\s,;]+/)
    .map((a) => a.trim())
    .filter(Boolean);
}
