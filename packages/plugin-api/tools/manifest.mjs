/**
 * A quick check of `manifest.json` before packing. Nail's own parser
 * (`PluginManifest::parse` in `crates/nail-api`) is authoritative and runs
 * again on install; this mirrors its most common rules so authors see the
 * problems while building. Issue kinds use the same names as Nail's
 * (`invalid-id`, `unknown-command`, …).
 */

import { isCron } from "./cron.mjs";

/** Every permission Nail offers (plus `network:<host>`), in display order. */
export const PERMISSIONS = [
  "mail.read",
  "mail.read.body",
  "calendar.read",
  "ui.dialogs",
  "notifications",
  "storage",
  "mail.organize",
  "mail.send",
  "calendar.write",
  "contacts.write",
  "tasks.write",
  "hooks.arrival",
  "hooks.beforeSend",
  "hooks.open",
  "composer",
  "attachments",
  "background",
  "mcp.expose",
];

/** Permissions of the design that this version refuses (`permission-not-in-preview`). */
export const FUTURE_PERMISSIONS = [
  "mail.read.encrypted",
  "contacts.read",
  "tasks.read",
  "ui.webview",
];

/** Permissions another one needs in the same manifest (`missing-required-permission`). */
export const REQUIRED_PERMISSIONS = {
  "mail.read.body": ["mail.read"],
  "mail.organize": ["mail.read"],
  "hooks.arrival": ["mail.read"],
  "hooks.open": ["mail.read"],
};

/** Permissions a grant can limit to accounts. */
export const ACCOUNT_PERMISSIONS = [
  "mail.read",
  "mail.read.body",
  "mail.organize",
  "mail.send",
  "hooks.arrival",
  "hooks.beforeSend",
  "hooks.open",
  "attachments",
];

/** Permissions that let a plug-in change the user's data (the install dialog highlights them). */
export const WRITE_PERMISSIONS = [
  "mail.organize",
  "mail.send",
  "calendar.write",
  "contacts.write",
  "tasks.write",
];

/** Hook kinds and their permissions. */
export const HOOK_PERMISSIONS = {
  arrival: "hooks.arrival",
  beforeSend: "hooks.beforeSend",
  open: "hooks.open",
};

/** Events a background job can subscribe to. */
export const JOB_EVENTS = [
  "message.received",
  "message.sent",
  "selection.changed",
  "event.reminder",
  "app.started",
];

/** MCP scopes (what an exposed tool may require of a client). */
export const MCP_SCOPES = [
  "read",
  "raw-source",
  "attachments",
  "encrypted",
  "context",
  "organize",
  "permanent-delete",
  "draft",
  "send",
  "calendar",
  "pim",
  "rules",
];

/** Largest attachment a preview renderer may declare (bytes). */
export const MAX_PREVIEW_INPUT = 10 * 1024 * 1024;
/** Longest MCP tool name (`<plugin>__<tool>`). */
export const MAX_MCP_TOOL_NAME = 64;
/** Largest `inputSchema` (bytes of JSON). */
export const MAX_MCP_TOOL_SCHEMA = 16 * 1024;

const COMMAND_CONTEXTS = ["list", "reader", "composer", "global"];
const KEY_CONTEXTS = ["global", "list", "reader", "composer", "search", "window"];

/** kebab-case, 1–64 characters, starting with a letter. */
/**
 * @param {unknown} s
 * @returns {boolean}
 */
export function isKebabName(s) {
  return (
    typeof s === "string" &&
    s.length > 0 &&
    s.length <= 64 &&
    /^[a-z][a-z0-9-]*$/.test(s) &&
    !s.endsWith("-") &&
    !s.includes("--")
  );
}

/** Semantic version `MAJOR.MINOR.PATCH[-pre][+build]`. */
/**
 * @param {unknown} s
 * @returns {boolean}
 */
export function isVersion(s) {
  return (
    typeof s === "string" &&
    s.length <= 64 &&
    /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$/.test(
      s,
    )
  );
}

/** A relative `/`-separated path inside a package. */
/**
 * @param {unknown} s
 * @returns {boolean}
 */
export function isPackagePath(s) {
  return (
    typeof s === "string" &&
    s.length > 0 &&
    s.length <= 200 &&
    s
      .split("/")
      .every(
        (part) => part !== "" && part !== "." && part !== ".." && /^[A-Za-z0-9._-]+$/.test(part),
      )
  );
}

/** A `network:` host pattern: a host name, `*.` + a domain, or an IPv4 address. */
/**
 * @param {unknown} pattern
 * @returns {boolean}
 */
export function isNetworkHost(pattern) {
  if (typeof pattern !== "string") return false;
  const ipv4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(pattern);
  if (ipv4 && ipv4.slice(1).every((n) => Number(n) <= 255 && (n === "0" || !n.startsWith("0")))) {
    return true;
  }
  const wildcard = pattern.startsWith("*.");
  const host = wildcard ? pattern.slice(2) : pattern;
  if (host.length === 0 || host.length > 253) return false;
  const labels = host.split(".");
  return (
    labels.every(
      (l) =>
        l.length > 0 &&
        l.length <= 63 &&
        /^[a-z0-9-]+$/.test(l) &&
        !l.startsWith("-") &&
        !l.endsWith("-"),
    ) &&
    (!wildcard || labels.length >= 2) &&
    !/^\d+$/.test(labels[labels.length - 1] ?? "")
  );
}

/**
 * Checks a parsed manifest.
 * @param {any} manifest
 * @returns {{ kind: string, path: string, detail?: string }[]}
 */
export function checkManifest(manifest) {
  /** @type {{ kind: string, path: string, detail?: string }[]} */
  const issues = [];
  /**
   * @param {string} kind
   * @param {string} path
   * @param {unknown} [detail]
   */
  const issue = (kind, path, detail) =>
    issues.push(detail === undefined ? { kind, path } : { kind, path, detail: String(detail) });
  if (typeof manifest !== "object" || manifest === null || Array.isArray(manifest)) {
    issue("invalid-json", "", "not an object");
    return issues;
  }
  if (!isKebabName(manifest.id)) issue("invalid-id", "id", manifest.id);
  if (typeof manifest.name !== "string" || !manifest.name.trim() || manifest.name.length > 100) {
    issue("invalid-name", "name", manifest.name);
  }
  if (!isVersion(manifest.version)) issue("invalid-version", "version", manifest.version);
  if (manifest.apiVersion !== "1") {
    issue("unsupported-api-version", "apiVersion", manifest.apiVersion);
  }
  if (!isPackagePath(manifest.main) || !/\.m?js$/.test(manifest.main)) {
    issue("invalid-main", "main", manifest.main);
  }
  if (manifest.homepage !== undefined && !/^https:\/\//.test(String(manifest.homepage))) {
    issue("invalid-homepage", "homepage", manifest.homepage);
  }

  const permissions = Array.isArray(manifest.permissions) ? manifest.permissions : [];
  const seen = new Set();
  permissions.forEach((/** @type {any} */ p, /** @type {number} */ i) => {
    const path = `permissions[${i}]`;
    if (seen.has(p)) issue("duplicate-permission", path, p);
    seen.add(p);
    if (typeof p === "string" && p.startsWith("network:")) {
      if (!isNetworkHost(p.slice(8))) issue("invalid-network-domain", path, p);
    } else if (FUTURE_PERMISSIONS.includes(p)) {
      issue("permission-not-in-preview", path, p);
    } else if (!PERMISSIONS.includes(p)) {
      issue("unknown-permission", path, p);
    }
  });
  if (seen.has("mail.read.body") && !seen.has("mail.read")) {
    issue("body-without-read", "permissions", "mail.read.body");
  }
  for (const p of seen) {
    if (p === "mail.read.body") continue;
    for (const needed of REQUIRED_PERMISSIONS[
      /** @type {keyof typeof REQUIRED_PERMISSIONS} */ (p)
    ] ?? []) {
      if (!seen.has(needed)) {
        issue("missing-required-permission", "permissions", `${p} needs ${needed}`);
      }
    }
  }

  /** @type {Record<string, any>} */
  const c = manifest.contributes ?? {};
  const commands = new Set();
  (c.commands ?? []).forEach((/** @type {any} */ cmd, /** @type {number} */ i) => {
    const path = `contributes.commands[${i}]`;
    if (!isKebabName(cmd?.id) || typeof cmd?.title !== "string" || !cmd.title.trim()) {
      issue("invalid-command", path, cmd?.id);
    } else if (commands.has(cmd.id)) {
      issue("duplicate-command", `${path}.id`, cmd.id);
    }
    for (const ctx of cmd?.contexts ?? []) {
      if (!COMMAND_CONTEXTS.includes(ctx)) issue("invalid-command", `${path}.contexts`, ctx);
    }
    if (cmd?.id) commands.add(cmd.id);
  });
  const panels = new Set();
  (c.panels ?? []).forEach((/** @type {any} */ panel, /** @type {number} */ i) => {
    const path = `contributes.panels[${i}]`;
    if (!isKebabName(panel?.id) || typeof panel?.title !== "string" || !panel.title.trim()) {
      issue("invalid-panel", path, panel?.id);
    } else if (panels.has(panel.id)) {
      issue("duplicate-panel", `${path}.id`, panel.id);
    }
    if (panel?.id) panels.add(panel.id);
  });
  (c.placements ?? []).forEach((/** @type {any} */ pl, /** @type {number} */ i) => {
    const path = `contributes.placements[${i}]`;
    const targets = ["toolbar", "menu", "panel"].filter((k) => pl?.[k] !== undefined);
    if (targets.length !== 1 || (pl.after !== undefined && pl.before !== undefined)) {
      issue("invalid-placement", path);
      return;
    }
    if (targets[0] === "panel") {
      if (!panels.has(pl.panel)) issue("unknown-panel", `${path}.panel`, pl.panel);
    } else if (!commands.has(pl.command)) {
      issue("unknown-command", `${path}.command`, pl.command);
    }
  });
  (c.keys ?? []).forEach((/** @type {any} */ key, /** @type {number} */ i) => {
    const path = `contributes.keys[${i}]`;
    if (typeof key?.key !== "string" || !key.key.trim())
      issue("invalid-key", `${path}.key`, key?.key);
    if (key?.context !== undefined && !KEY_CONTEXTS.includes(key.context)) {
      issue("invalid-key", `${path}.context`, key.context);
    }
    if (!commands.has(key?.command)) issue("unknown-command", `${path}.command`, key?.command);
  });
  const settings = new Set();
  (c.settings ?? []).forEach((/** @type {any} */ s, /** @type {number} */ i) => {
    const path = `contributes.settings[${i}]`;
    if (!["string", "number", "boolean", "select"].includes(s?.type) || !isKebabName(s?.key)) {
      issue("invalid-setting", path, s?.key);
    } else if (settings.has(s.key)) {
      issue("duplicate-setting", `${path}.key`, s.key);
    }
    if (s?.key) settings.add(s.key);
  });
  for (const [tag, file] of Object.entries(c.locales ?? {})) {
    if (
      !/^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(tag) ||
      !isPackagePath(file) ||
      !file.endsWith(".ftl")
    ) {
      issue("invalid-locale", `contributes.locales.${tag}`, file);
    }
  }
  checkExtensions(manifest, seen, issue);
  /** @type {[string, number][]} */
  const counts = [
    ["commands", 100],
    ["placements", 100],
    ["keys", 100],
    ["panels", 50],
    ["settings", 50],
    ["composerActions", 50],
    ["markdownBlocks", 50],
    ["attachmentPreviews", 50],
    ["attachmentActions", 50],
    ["linkActions", 50],
    ["jobs", 10],
    ["mcpTools", 50],
  ];
  for (const [name, max] of counts) {
    if ((c[name] ?? []).length > max) issue("too-many-contributions", `contributes.${name}`);
  }
  if (Object.keys(c.locales ?? {}).length > 50) {
    issue("too-many-contributions", "contributes.locales");
  }
  return issues;
}

/**
 * A title: not blank, at most 100 characters.
 * @param {unknown} s
 */
function isTitle(s) {
  return typeof s === "string" && s.trim().length > 0 && [...s].length <= 100;
}

/**
 * At most 1000 characters (or absent).
 * @param {unknown} s
 */
function isDescription(s) {
  return s === undefined || (typeof s === "string" && [...s].length <= 1000);
}

/**
 * A MIME type pattern: `type/sub` or `type/*`, lower case.
 * @param {unknown} mime
 */
export function isMimePattern(mime) {
  if (typeof mime !== "string") return false;
  const [ty, sub, ...rest] = mime.split("/");
  const token = (/** @type {string | undefined} */ t) =>
    typeof t === "string" && t.length > 0 && t.length <= 64 && /^[a-z0-9.+-]+$/.test(t);
  return rest.length === 0 && token(ty) && (sub === "*" || token(sub));
}

/**
 * A file extension: 1–16 lower-case letters and digits, no dot.
 * @param {unknown} ext
 */
export function isExtension(ext) {
  return typeof ext === "string" && /^[a-z0-9]{1,16}$/.test(ext);
}

/**
 * MIME types and extensions of a contribution are valid (at most 20 each).
 * @param {unknown} mimes
 * @param {unknown} exts
 */
function matchersOk(mimes, exts) {
  const m = mimes ?? [];
  const e = exts ?? [];
  return (
    Array.isArray(m) &&
    Array.isArray(e) &&
    m.length <= 20 &&
    e.length <= 20 &&
    m.every(isMimePattern) &&
    e.every(isExtension)
  );
}

/**
 * The M4a contributions: hooks, composer actions, Markdown blocks,
 * attachment previews and actions, link actions, jobs, MCP tools (each
 * needs its permission).
 * @param {any} manifest
 * @param {Set<unknown>} permissions
 * @param {(kind: string, path: string, detail?: unknown) => void} issue
 */
function checkExtensions(manifest, permissions, issue) {
  /** @type {Record<string, any>} */
  const c = manifest.contributes ?? {};
  const list = (/** @type {string} */ name) => (Array.isArray(c[name]) ? c[name] : []);
  const hooks = list("hooks");
  /** @type {[string, string, boolean][]} */
  const needs = [
    [
      "hooks.arrival",
      "contributes.hooks",
      hooks.some((/** @type {any} */ h) => h?.hook === "arrival"),
    ],
    [
      "hooks.beforeSend",
      "contributes.hooks",
      hooks.some((/** @type {any} */ h) => h?.hook === "beforeSend"),
    ],
    ["hooks.open", "contributes.hooks", hooks.some((/** @type {any} */ h) => h?.hook === "open")],
    ["composer", "contributes.composerActions", list("composerActions").length > 0],
    ["composer", "contributes.markdownBlocks", list("markdownBlocks").length > 0],
    ["attachments", "contributes.attachmentPreviews", list("attachmentPreviews").length > 0],
    ["attachments", "contributes.attachmentActions", list("attachmentActions").length > 0],
    ["background", "contributes.jobs", list("jobs").length > 0],
    ["mcp.expose", "contributes.mcpTools", list("mcpTools").length > 0],
  ];
  for (const [permission, path, used] of needs) {
    if (used && !permissions.has(permission)) {
      issue("contribution-needs-permission", path, permission);
    }
  }

  const seenHooks = new Set();
  hooks.forEach((/** @type {any} */ hook, /** @type {number} */ i) => {
    const path = `contributes.hooks[${i}]`;
    const kind = hook?.hook;
    const filterOk =
      hook?.filter === undefined ||
      (kind !== "beforeSend" &&
        typeof hook.filter === "string" &&
        hook.filter.trim().length > 0 &&
        [...hook.filter].length <= 500);
    const timeoutOk =
      hook?.timeoutMs === undefined ||
      (Number.isInteger(hook.timeoutMs) && hook.timeoutMs >= 50 && hook.timeoutMs <= 10_000);
    if (!(kind in HOOK_PERMISSIONS) || !filterOk || !timeoutOk) {
      issue("invalid-hook", path, kind);
    } else if (seenHooks.has(kind)) {
      issue("duplicate-hook", path, kind);
    } else {
      seenHooks.add(kind);
    }
  });

  /**
   * @param {string} name
   * @param {(item: any) => boolean} valid
   */
  const extensions = (name, valid) => {
    const seen = new Set();
    list(name).forEach((/** @type {any} */ item, /** @type {number} */ i) => {
      const path = `contributes.${name}[${i}]`;
      if (!isKebabName(item?.id) || !valid(item)) {
        issue("invalid-extension", path, item?.id);
      } else if (seen.has(item.id)) {
        issue("duplicate-extension", path, item.id);
      } else {
        seen.add(item.id);
      }
    });
  };
  extensions("composerActions", (a) => isTitle(a.title) && isDescription(a.description));
  extensions(
    "markdownBlocks",
    (b) => isTitle(b.title) && (b.template === undefined || isDescription(b.template)),
  );
  extensions(
    "attachmentPreviews",
    (p) =>
      isTitle(p.title) &&
      ((p.mimeTypes ?? []).length > 0 || (p.extensions ?? []).length > 0) &&
      (p.maxBytes === undefined ||
        (Number.isInteger(p.maxBytes) && p.maxBytes > 0 && p.maxBytes <= MAX_PREVIEW_INPUT)) &&
      matchersOk(p.mimeTypes, p.extensions),
  );
  extensions("attachmentActions", (a) => isTitle(a.title) && matchersOk(a.mimeTypes, a.extensions));
  extensions(
    "linkActions",
    (a) =>
      isTitle(a.title) &&
      Array.isArray(a.hosts ?? []) &&
      (a.hosts ?? []).length <= 20 &&
      (a.hosts ?? []).every(isNetworkHost),
  );

  const jobs = new Set();
  list("jobs").forEach((/** @type {any} */ job, /** @type {number} */ i) => {
    const path = `contributes.jobs[${i}]`;
    const events = job?.events ?? [];
    const valid =
      isKebabName(job?.id) &&
      (job.title === undefined || isTitle(job.title)) &&
      Array.isArray(events) &&
      events.every((/** @type {unknown} */ e) => JOB_EVENTS.includes(String(e))) &&
      (job.schedule !== undefined || events.length > 0) &&
      (job.schedule === undefined || isCron(job.schedule));
    if (!valid) issue("invalid-job", path, job?.id);
    else if (jobs.has(job.id)) issue("duplicate-job", path, job.id);
    else jobs.add(job.id);
  });

  const tools = new Set();
  list("mcpTools").forEach((/** @type {any} */ tool, /** @type {number} */ i) => {
    const path = `contributes.mcpTools[${i}]`;
    const nameOk =
      isKebabName(tool?.id) && `${manifest.id}__${tool.id}`.length <= MAX_MCP_TOOL_NAME;
    const descriptionOk =
      typeof tool?.description === "string" &&
      tool.description.trim().length > 0 &&
      [...tool.description].length <= 1000;
    const schema = tool?.inputSchema;
    const schemaOk =
      schema === undefined ||
      (typeof schema === "object" &&
        schema !== null &&
        !Array.isArray(schema) &&
        schema.type === "object" &&
        Buffer.byteLength(JSON.stringify(schema)) <= MAX_MCP_TOOL_SCHEMA);
    const scopes = tool?.scopes ?? ["read"];
    const scopesOk =
      Array.isArray(scopes) &&
      scopes.length > 0 &&
      scopes.every((/** @type {unknown} */ s) => MCP_SCOPES.includes(String(s)));
    if (!nameOk || !descriptionOk || !schemaOk || !scopesOk) {
      issue("invalid-mcp-tool", path, tool?.id);
    } else if (tools.has(tool.id)) {
      issue("duplicate-mcp-tool", path, tool.id);
    } else {
      tools.add(tool.id);
    }
  });
}
