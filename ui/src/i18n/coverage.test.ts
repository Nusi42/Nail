// @vitest-environment node
/**
 * Every message id the UI asks for exists in the English catalogs (German
 * falls back to English per key). Literal ids are collected from the
 * sources; ids built at runtime are listed from the values they are built
 * from.
 */
import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { FluentResource } from "@fluent/bundle";
import ts from "typescript";
import type {
  AccountState,
  DiscoverySource,
  ErrorCode,
  OutboxState,
  SearchErrorKind,
} from "../api/generated";
import { QUERY_HINTS, hintMessageId } from "../widgets/SearchBar";
import { COLUMN_IDS } from "../widgets/MessageList";
import { errorMessageId } from "../api/client";
import * as agentsMessages from "../features/agents/messages";
import * as composerMessages from "../features/composer/messages";
import * as customizationMessages from "../features/customization/messages";
import * as exchangeMessages from "../features/exchange/messages";
import * as gmailMessages from "../features/gmail/messages";
import * as mcpMessages from "../features/mcp/messages";
import * as rulesMessages from "../features/rules/messages";
import * as schedulingMessages from "../features/scheduling/messages";
import * as remoteMessages from "../features/remote/messages";
import * as securityMessages from "../features/security/messages";
import * as menusMessages from "../features/menus/messages";
import * as pimMessages from "../features/pim/messages";
import * as pluginsMessages from "../features/plugins/messages";
import * as sieveMessages from "../features/sieve/messages";
import * as syncMessages from "../features/sync/messages";
import * as systemMessages from "../features/system/messages";
import * as portabilityMessages from "../features/portability/messages";
import * as paletteMessages from "../features/palette/messages";
import * as printMessages from "../features/print/messages";
import * as triageMessages from "../features/triage/messages";
import * as appearanceMessages from "../features/appearance/messages";
import * as shellMessages from "../features/shell/messages";
import { KEY_CONTEXTS } from "../features/customization/editorModel";
import { shippedCatalogs } from "./catalogs";

/** Each feature slot lists its own message prefixes and runtime ids. */
const FEATURE_MESSAGES = [
  composerMessages,
  rulesMessages,
  mcpMessages,
  customizationMessages,
  securityMessages,
  remoteMessages,
  sieveMessages,
  triageMessages,
  syncMessages,
  menusMessages,
  pluginsMessages,
  pimMessages,
  schedulingMessages,
  gmailMessages,
  exchangeMessages,
  agentsMessages,
  systemMessages,
  portabilityMessages,
  paletteMessages,
  printMessages,
  appearanceMessages,
  shellMessages,
];

const srcDir = resolve(process.cwd(), "src");
const repoDir = resolve(process.cwd(), "..");
const localesDir = join(repoDir, "locales");

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
  "launch-",
  ...FEATURE_MESSAGES.flatMap((m) => m.prefixes),
];
/** Literals with those prefixes that are not message ids (part names). */
const NOT_MESSAGES = new Set([
  "status-bar",
  // M5b widget kind (`nail_api::views::SETTINGS_PAGE_WIDGET`).
  "settings-page",
  "window-layer",
  "virtual-accounts",
  "virtual-account-editor",
  "virtual-editor",
  "launch-notice",
  ...FEATURE_MESSAGES.flatMap((m) => m.notMessages),
]);

/** A message id built from a template string, e.g. ``t(`pim-view-${view}`)``. */
interface TemplateId {
  /** `file:line`. */
  where: string;
  /** The template's source text without the backticks. */
  template: string;
  /** Matches every id the template can build (`${…}` = any text). */
  pattern: RegExp;
  /**
   * Every id it can build when each `${…}` has a string-literal union type
   * (e.g. a generated API enum); `null` when one of them is plain `string`.
   */
  ids: string[] | null;
}

/** The string values of a literal (union) type, `null` for open types. */
function literalValues(checker: ts.TypeChecker, type: ts.Type): string[] | null {
  if (type.isStringLiteral()) return [type.value];
  if (type.isNumberLiteral()) return [String(type.value)];
  if (type.isUnion()) {
    const out: string[] = [];
    for (const member of type.types) {
      const values = literalValues(checker, member);
      if (values === null) return null;
      out.push(...values);
    }
    return out;
  }
  return null;
}

/**
 * Every template-string message id of the sources (arguments of `t(…)` /
 * `ctx.t(…)` and `labelId={…}` props), typed by the TypeScript checker.
 */
function templateIds(): TemplateId[] {
  const config = ts.getParsedCommandLineOfConfigFile(
    resolve(process.cwd(), "tsconfig.app.json"),
    {},
    { ...ts.sys, onUnRecoverableConfigFileDiagnostic: () => undefined },
  );
  if (!config) throw new Error("tsconfig.app.json");
  const files = config.fileNames.filter(
    (f) => !/\.test\.tsx?$/.test(f) && !/[\\/](generated|test)[\\/]/.test(f),
  );
  const program = ts.createProgram(files, config.options);
  const checker = program.getTypeChecker();
  const wanted = new Set(files.map((f) => resolve(f)));
  const out: TemplateId[] = [];
  const isMessageArgument = (node: ts.TemplateExpression): boolean => {
    const parent = node.parent;
    if (ts.isCallExpression(parent) && parent.arguments[0] === node) {
      const callee = parent.expression;
      const name = ts.isIdentifier(callee)
        ? callee.text
        : ts.isPropertyAccessExpression(callee)
          ? callee.name.text
          : "";
      return name === "t";
    }
    return (
      ts.isJsxExpression(parent) &&
      ts.isJsxAttribute(parent.parent) &&
      parent.parent.name.getText() === "labelId"
    );
  };
  for (const source of program.getSourceFiles()) {
    if (!wanted.has(resolve(source.fileName))) continue;
    const visit = (node: ts.Node) => {
      if (ts.isTemplateExpression(node) && isMessageArgument(node)) {
        const escape = (text: string) => text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
        let pattern = escape(node.head.text);
        let ids: string[] | null = [node.head.text];
        for (const span of node.templateSpans) {
          pattern += `.+${escape(span.literal.text)}`;
          const values = literalValues(checker, checker.getTypeAtLocation(span.expression));
          ids =
            ids === null || values === null
              ? null
              : ids.flatMap((id) => values.map((v) => `${id}${v}${span.literal.text}`));
        }
        const line = source.getLineAndCharacterOfPosition(node.getStart()).line + 1;
        out.push({
          where: `${relative(srcDir, source.fileName)}:${line}`,
          template: node.getText().slice(1, -1),
          pattern: new RegExp(`^${pattern}$`),
          ids,
        });
      }
      ts.forEachChild(node, visit);
    };
    visit(source);
  }
  return out;
}

/**
 * Template ids whose values are plain `string`s, so the checker cannot list
 * them; each family's values are listed by a `runtimeIds()` (see where).
 * Prefer a literal union type (or a type guard) over adding to this list.
 */
const OPEN_TEMPLATES = new Set([
  // Toolbar commands (`composer/messages.ts`: FORMAT_COMMANDS).
  "composer-format-${id}",
  "composer-format-${id}-short",
  // The palette's names (`gmail/messages.ts`: LABEL_COLORS).
  "gmail-color-${name}",
  // Only names in NAMED_KEYS (`menus/messages.ts`).
  "context-menu-key-${name}",
  // Parts with a context menu (`menus/messages.ts`: CONTEXT_MENU_PARTS).
  'context-menu-label-${open.target.part.replaceAll(".", "-")}',
  // Columns filtered to COLUMN_IDS (`i18n/coverage.test.ts`).
  "message-list-column-${c}",
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
  "locked",
  "no_key",
  "crypto",
  "forbidden",
  "reauth_required",
  "rate_limited",
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
  "provider",
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
    ...COLUMN_IDS.map((c) => `message-list-column-${c}`),
    "account-all",
    ...FEATURE_MESSAGES.flatMap((m) => m.runtimeIds()),
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

  describe("ids built from template strings", () => {
    let templates: TemplateId[] = [];
    // One TypeScript program over the sources (a few seconds; longer when
    // cargo builds run on the same machine).
    beforeAll(() => {
      templates = templateIds();
    }, 120_000);

    it("finds them", () => {
      expect(templates.length).toBeGreaterThan(150);
      expect(templates.filter((x) => x.ids !== null).length).toBeGreaterThan(150);
    });

    it("defines every id a typed template can build", () => {
      // `${value}` with a literal union type: every value needs a message.
      const missing = templates.flatMap(({ where, ids }) =>
        (ids ?? []).filter((id) => !en.has(id)).map((id) => `${where} ${id}`),
      );
      expect(missing).toEqual([]);
    });

    it("lists the values of every untyped template", () => {
      // `${value}: string` is only checked through the ids a feature's
      // `runtimeIds()` builds from the values; such a template must be
      // reviewed into OPEN_TEMPLATES and match some listed id.
      const known = [...runtimeIds(), ...literalIds()];
      const open = templates.filter((x) => x.ids === null);
      expect(
        open.filter((x) => !OPEN_TEMPLATES.has(x.template)).map((x) => `${x.where} ${x.template}`),
      ).toEqual([]);
      expect(
        open.filter((x) => !known.some((id) => x.pattern.test(id))).map((x) => x.template),
      ).toEqual([]);
      const stale = [...OPEN_TEMPLATES].filter((tpl) => !open.some((x) => x.template === tpl));
      expect(stale).toEqual([]);
    });
  });

  it("labels every key context the UI, the default layout and plug-ins use", () => {
    const contexts = new Set<string>(KEY_CONTEXTS);
    const fromSources = new Set<string>();
    for (const file of sources(srcDir)) {
      for (const match of readFileSync(file, "utf8").matchAll(/data-key-context="([^"]+)"/g)) {
        fromSources.add(match[1] as string);
      }
    }
    const layout = readFileSync(join(repoDir, "defaults/layout.kdl"), "utf8");
    const fromLayout = [...layout.matchAll(/\bkeys\s+context="([^"]+)"/g)].map((m) => m[1]);
    // The contexts a plug-in manifest may bind keys in (Rust and the SDK's
    // manifest checker).
    const fromPlugins = [
      "crates/nail-api/src/plugins/manifest.rs",
      "packages/plugin-api/tools/manifest.mjs",
    ].flatMap((path) => {
      const list = /KEY_CONTEXTS[^=]*=\s*&?\[([^\]]*)\]/.exec(
        readFileSync(join(repoDir, path), "utf8"),
      );
      expect(list, path).not.toBeNull();
      return [...(list?.[1] ?? "").matchAll(/"([^"]+)"/g)].map((m) => m[1]);
    });
    expect(fromLayout.length).toBeGreaterThan(3);
    expect(fromPlugins.length).toBeGreaterThan(3);
    const unknown = [...fromSources, ...fromLayout, ...fromPlugins].filter(
      (c) => !contexts.has(c as string),
    );
    expect(unknown).toEqual([]);
    // No stale entries: every context but `global` is set by some element.
    expect(KEY_CONTEXTS.filter((c) => c !== "global" && !fromSources.has(c))).toEqual([]);
    expect(
      KEY_CONTEXTS.map((c) => `customize-keys-context-${c}`).filter((id) => !en.has(id)),
    ).toEqual([]);
  });

  const files = (locale: string) =>
    readdirSync(join(localesDir, locale))
      .filter((f) => f.endsWith(".ftl"))
      .sort();

  it("keeps the German catalogs as the same (empty) files", () => {
    expect(files("de")).toEqual(files("en"));
    for (const feature of ["composer", "rules", "mcp", "customization", "security"]) {
      expect(files("en")).toContain(`${feature}.ftl`);
    }
  });

  it("bundles every catalog file without a list to maintain", () => {
    for (const locale of ["en", "de"]) {
      expect(shippedCatalogs[locale]?.length).toBe(files(locale).length);
    }
  });
});
