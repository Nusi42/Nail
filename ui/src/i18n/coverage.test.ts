// @vitest-environment node
/**
 * Every message id the UI asks for exists in the English catalogs (German
 * falls back to English per key). Literal ids are collected from the
 * sources; ids built at runtime are listed from the values they are built
 * from.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { FluentResource } from "@fluent/bundle";
import type {
  AccountState,
  DiscoverySource,
  ErrorCode,
  OutboxState,
  SearchErrorKind,
} from "../api/generated";
import { QUERY_HINTS, hintMessageId } from "../widgets/SearchBar";
import { errorMessageId } from "../api/client";

const srcDir = resolve(process.cwd(), "src");
const localesDir = resolve(process.cwd(), "../locales");

function sources(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "generated" && entry.name !== "test") out.push(...sources(path));
    } else if (/\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      out.push(path);
    }
  }
  return out;
}

function catalogIds(locale: string): Set<string> {
  const ids = new Set<string>();
  const dir = join(localesDir, locale);
  for (const file of readdirSync(dir).filter((f) => f.endsWith(".ftl"))) {
    const resource = new FluentResource(readFileSync(join(dir, file), "utf8"));
    for (const entry of resource.body) ids.add(entry.id);
  }
  return ids;
}

/** Prefixes of message ids written literally in the sources. */
const PREFIXES = [
  "composer-",
  "setup-",
  "settings-",
  "virtual-",
  "outbox-",
  "search-bar-",
  "search-hint-",
  "status-",
  "thread-view-",
  "folder-picker-",
  "window-",
  "server-",
  "sign-in-",
  "oauth-provider-",
  "identity-",
  "message-list-",
  "message-view-",
  "message-header-",
  "account-switcher-",
  "folder-tree-",
  "toolbar-",
  "layout-",
  "app-",
];
/** Literals with those prefixes that are not message ids (part names). */
const NOT_MESSAGES = new Set([
  "status-bar",
  "window-layer",
  "virtual-accounts",
  "virtual-account-editor",
  "virtual-editor",
]);

function literalIds(): Set<string> {
  const ids = new Set<string>();
  for (const file of sources(srcDir)) {
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(/["']([a-z][a-z0-9]*(?:-[a-z0-9]+)+)["']/g)) {
      const id = match[1] as string;
      if (PREFIXES.some((p) => id.startsWith(p)) && !NOT_MESSAGES.has(id)) ids.add(id);
    }
  }
  return ids;
}

const ERROR_CODES: ErrorCode[] = [
  "invalid_request",
  "not_found",
  "unsupported",
  "auth_failed",
  "network",
  "config",
  "unavailable",
  "internal",
  "conflict",
  "tls",
  "too_large",
  "rejected",
  "cancelled",
];
const STATES: AccountState[] = ["online", "connecting", "offline", "auth-error", "disabled"];
const OUTBOX: OutboxState[] = ["scheduled", "sending", "sent", "failed", "cancelled"];
const KINDS: SearchErrorKind[] = [
  "unexpected-token",
  "unexpected-end",
  "unterminated-quote",
  "unbalanced-parenthesis",
  "unknown-field",
  "invalid-value",
];
const SOURCES: DiscoverySource[] = [
  "autoconfig",
  "well-known",
  "ispdb",
  "srv",
  "autodiscover",
  "guess",
];
const VALUE_HINTS = ["is:read", "is:unflagged", "is:answered"];

function runtimeIds(): string[] {
  return [
    ...ERROR_CODES.map(errorMessageId),
    ...STATES.map((s) => `account-state-${s}`),
    ...OUTBOX.map((s) => `outbox-state-${s}`),
    ...KINDS.map((k) => `search-error-${k}`),
    ...SOURCES.map((s) => `setup-source-${s}`),
    ...["address", "servers", "test", "done"].map((s) => `setup-step-${s}`),
    ...["to", "cc", "bcc", "subject"].map((f) => `composer-field-${f}`),
    ...["incoming", "outgoing"].map((s) => `server-${s}`),
    ...["google", "microsoft"].flatMap((p) => [`oauth-provider-${p}`, `sign-in-oauth-${p}`]),
    ...["composer", "outbox", "move", "account-setup", "settings"].map((w) => `window-${w}`),
    ...[...QUERY_HINTS, ...VALUE_HINTS].map(hintMessageId),
    "account-all",
  ];
}

describe("English catalog coverage", () => {
  const en = catalogIds("en");

  it("finds the literal ids", () => {
    expect(literalIds().size).toBeGreaterThan(150);
  });

  it("defines every id the UI uses", () => {
    const missing = [...literalIds(), ...runtimeIds()].filter((id) => !en.has(id));
    expect(missing).toEqual([]);
  });

  it("keeps the German catalogs as the same (empty) files", () => {
    const files = (locale: string) =>
      readdirSync(join(localesDir, locale))
        .filter((f) => f.endsWith(".ftl"))
        .sort();
    expect(files("de")).toEqual(files("en"));
  });
});
