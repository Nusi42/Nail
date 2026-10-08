/**
 * A quick check of `manifest.json` before packing. Nail's own parser
 * (`PluginManifest::parse` in `crates/nail-api`) is authoritative and runs
 * again on install; this mirrors its most common rules so authors see the
 * problems while building. Issue kinds use the same names as Nail's
 * (`invalid-id`, `unknown-command`, …).
 */

import { CONTRACT } from "./contract.mjs";
import { isCron } from "./cron.mjs";

/** Every permission Nail offers (plus `network:<host>`), in display order. */
export const { PLUGIN_PERMISSIONS: PERMISSIONS } = CONTRACT;

/** Permissions of the design that this version refuses (`permission-not-in-preview`). */
export const { FUTURE_PERMISSIONS } = CONTRACT;

/** Permissions another one needs in the same manifest (`missing-required-permission`). */
export const REQUIRED_PERMISSIONS = /** @type {Partial<Record<string, string[]>>} */ (
  CONTRACT.REQUIRED_PERMISSIONS
);

/** Permissions a grant can limit to accounts. */
export const { ACCOUNT_PERMISSIONS } = CONTRACT;

/** Permissions that let a plug-in change the user's data (the install dialog highlights them). */
export const { WRITE_PERMISSIONS } = CONTRACT;

/** Hook kinds and their permissions. */
export const { HOOK_PERMISSIONS } = CONTRACT;

/** Events a background job can subscribe to. */
export const { PLUGIN_EVENT_NAME_VALUES: JOB_EVENTS } = CONTRACT;

/** MCP scopes (what an exposed tool may require of a client). */
export const { MCP_SCOPE_VALUES: MCP_SCOPES } = CONTRACT;

/**
 * Largest attachment a preview renderer may declare (bytes), longest MCP
 * tool name (`<plugin>__<tool>`), largest `inputSchema` (bytes of JSON).
 */
export const { MAX_PREVIEW_INPUT, MAX_MCP_TOOL_NAME, MAX_MCP_TOOL_SCHEMA } = CONTRACT;

const COMMAND_CONTEXTS = ["list", "reader", "composer", "global"];
const { KEY_CONTEXTS } = CONTRACT;

/**
 * Whether `value` is a JSON object (not `null`, not an array).
 *
 * @param {unknown} value
 * @returns {value is Record<string, unknown>}
 */
function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * `value` as an object's fields; `{}` for anything else (the checks then
 * report the missing fields).
 *
 * @param {unknown} value
 * @returns {Record<string, unknown>}
 */
function fields(value) {
  return isRecord(value) ? value : {};
}

/**
 * `value` as a list; `[]` for anything else.
 *
 * @param {unknown} value
 * @returns {unknown[]}
 */
function items(value) {
  return Array.isArray(value) ? value : [];
}

/**
 * kebab-case, 1–64 characters, starting with a letter.
 *
 * @param {unknown} s
 * @returns {s is string}
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

/**
 * Semantic version `MAJOR.MINOR.PATCH[-pre][+build]`.
 *
 * @param {unknown} s
 * @returns {s is string}
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

/**
 * A relative `/`-separated path inside a package.
 *
 * @param {unknown} s
 * @returns {s is string}
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

/**
 * A `network:` host pattern: a host name, `*.` + a domain, or an IPv4 address.
 *
 * @param {unknown} pattern
 * @returns {pattern is string}
 */
export function isNetworkHost(pattern) {
  if (typeof pattern !== "string") return false;
  const ipv4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(pattern);
  if (ipv4?.slice(1).every((n) => Number(n) <= 255 && (n === "0" || !n.startsWith("0")))) {
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
 * Length in characters as Nail's parser counts them (Rust's
 * `str::chars().count()`: Unicode scalar values, not UTF-16 units).
 *
 * @param {string} s
 */
function charCount(s) {
  return Array.from(s).length;
}

/**
 * An issue's detail as text: strings as they are, other JSON values as JSON.
 *
 * @param {unknown} detail
 */
function describe(detail) {
  if (typeof detail === "string") return detail;
  return JSON.stringify(detail);
}

/**
 * Reports a problem: its kind, the JSON path and an optional detail.
 *
 * @typedef {(kind: string, path: string, detail?: unknown) => void} Issue
 */

/**
 * Checks a parsed manifest.
 *
 * @param {unknown} manifest
 * @returns {{ kind: string, path: string, detail?: string }[]}
 */
export function checkManifest(manifest) {
  /** @type {{ kind: string, path: string, detail?: string }[]} */
  const issues = [];
  /** @type {Issue} */
  const issue = (kind, path, detail) => {
    issues.push(detail === undefined ? { kind, path } : { kind, path, detail: describe(detail) });
  };
  if (!isRecord(manifest)) {
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
  if (
    manifest.homepage !== undefined &&
    !(typeof manifest.homepage === "string" && manifest.homepage.startsWith("https://"))
  ) {
    issue("invalid-homepage", "homepage", manifest.homepage);
  }

  /** @type {Set<unknown>} */
  const seen = new Set();
  items(manifest.permissions).forEach((p, i) => {
    const path = `permissions[${String(i)}]`;
    if (seen.has(p)) issue("duplicate-permission", path, p);
    seen.add(p);
    if (typeof p === "string" && p.startsWith("network:")) {
      if (!isNetworkHost(p.slice(8))) issue("invalid-network-domain", path, p);
    } else if (typeof p === "string" && FUTURE_PERMISSIONS.includes(p)) {
      issue("permission-not-in-preview", path, p);
    } else if (typeof p !== "string" || !PERMISSIONS.includes(p)) {
      issue("unknown-permission", path, p);
    }
  });
  if (seen.has("mail.read.body") && !seen.has("mail.read")) {
    issue("body-without-read", "permissions", "mail.read.body");
  }
  for (const p of seen) {
    if (typeof p !== "string" || p === "mail.read.body") continue;
    for (const needed of REQUIRED_PERMISSIONS[p] ?? []) {
      if (!seen.has(needed)) {
        issue("missing-required-permission", "permissions", `${p} needs ${needed}`);
      }
    }
  }

  const c = fields(manifest.contributes);
  /** @type {Set<unknown>} */
  const commands = new Set();
  items(c.commands).forEach((entry, i) => {
    const cmd = fields(entry);
    const path = `contributes.commands[${String(i)}]`;
    if (!isKebabName(cmd.id) || typeof cmd.title !== "string" || !cmd.title.trim()) {
      issue("invalid-command", path, cmd.id);
    } else if (commands.has(cmd.id)) {
      issue("duplicate-command", `${path}.id`, cmd.id);
    }
    if (cmd.contexts !== undefined && !Array.isArray(cmd.contexts)) {
      issue("invalid-command", `${path}.contexts`, cmd.contexts);
    }
    for (const ctx of items(cmd.contexts)) {
      if (typeof ctx !== "string" || !COMMAND_CONTEXTS.includes(ctx)) {
        issue("invalid-command", `${path}.contexts`, ctx);
      }
    }
    if (cmd.id) commands.add(cmd.id);
  });
  /** @type {Set<unknown>} */
  const panels = new Set();
  items(c.panels).forEach((entry, i) => {
    const panel = fields(entry);
    const path = `contributes.panels[${String(i)}]`;
    if (!isKebabName(panel.id) || typeof panel.title !== "string" || !panel.title.trim()) {
      issue("invalid-panel", path, panel.id);
    } else if (panels.has(panel.id)) {
      issue("duplicate-panel", `${path}.id`, panel.id);
    }
    if (panel.id) panels.add(panel.id);
  });
  items(c.placements).forEach((entry, i) => {
    const pl = fields(entry);
    const path = `contributes.placements[${String(i)}]`;
    const targets = ["toolbar", "menu", "panel"].filter((k) => pl[k] !== undefined);
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
  items(c.keys).forEach((entry, i) => {
    const key = fields(entry);
    const path = `contributes.keys[${String(i)}]`;
    if (typeof key.key !== "string" || !key.key.trim())
      issue("invalid-key", `${path}.key`, key.key);
    if (
      key.context !== undefined &&
      (typeof key.context !== "string" || !KEY_CONTEXTS.includes(key.context))
    ) {
      issue("invalid-key", `${path}.context`, key.context);
    }
    if (!commands.has(key.command)) issue("unknown-command", `${path}.command`, key.command);
  });
  /** @type {Set<unknown>} */
  const settings = new Set();
  items(c.settings).forEach((entry, i) => {
    const s = fields(entry);
    const path = `contributes.settings[${String(i)}]`;
    const typeOk =
      typeof s.type === "string" && ["string", "number", "boolean", "select"].includes(s.type);
    if (!typeOk || !isKebabName(s.key)) {
      issue("invalid-setting", path, s.key);
    } else if (settings.has(s.key)) {
      issue("duplicate-setting", `${path}.key`, s.key);
    }
    if (s.key) settings.add(s.key);
  });
  if (c.locales !== undefined && !isRecord(c.locales)) {
    issue("invalid-locale", "contributes.locales", c.locales);
  }
  const locales = fields(c.locales);
  for (const [tag, file] of Object.entries(locales)) {
    if (
      !/^[a-z]{2,3}(-[A-Za-z0-9]{2,8})*$/.test(tag) ||
      !isPackagePath(file) ||
      !file.endsWith(".ftl")
    ) {
      issue("invalid-locale", `contributes.locales.${tag}`, file);
    }
  }
  checkExtensions(typeof manifest.id === "string" ? manifest.id : "", c, seen, issue);
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
    if (items(c[name]).length > max) issue("too-many-contributions", `contributes.${name}`);
  }
  if (Object.keys(locales).length > 50) {
    issue("too-many-contributions", "contributes.locales");
  }
  return issues;
}

/**
 * A title: not blank, at most 100 characters.
 *
 * @param {unknown} s
 */
function isTitle(s) {
  return typeof s === "string" && s.trim().length > 0 && charCount(s) <= 100;
}

/**
 * At most 1000 characters (or absent).
 *
 * @param {unknown} s
 */
function isDescription(s) {
  return s === undefined || (typeof s === "string" && charCount(s) <= 1000);
}

/**
 * A MIME type pattern: `type/sub` or `type/*`, lower case.
 *
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
 *
 * @param {unknown} ext
 */
export function isExtension(ext) {
  return typeof ext === "string" && /^[a-z0-9]{1,16}$/.test(ext);
}

/**
 * MIME types and extensions of a contribution are valid (at most 20 each).
 *
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
 * Whether `value` is an integer in `[min, max]`.
 *
 * @param {unknown} value
 * @param {number} min
 * @param {number} max
 */
function isIntegerIn(value, min, max) {
  return typeof value === "number" && Number.isInteger(value) && value >= min && value <= max;
}

/**
 * The M4a contributions: hooks, composer actions, Markdown blocks,
 * attachment previews and actions, link actions, jobs, MCP tools (each
 * needs its permission).
 *
 * @param {string} pluginId
 * @param {Record<string, unknown>} c the manifest's `contributes`
 * @param {Set<unknown>} permissions
 * @param {Issue} issue
 */
function checkExtensions(pluginId, c, permissions, issue) {
  /** @param {string} name */
  const list = (name) => items(c[name]).map(fields);
  const hooks = list("hooks");
  /** @type {[string, string, boolean][]} */
  const needs = [
    ["hooks.arrival", "contributes.hooks", hooks.some((h) => h.hook === "arrival")],
    ["hooks.beforeSend", "contributes.hooks", hooks.some((h) => h.hook === "beforeSend")],
    ["hooks.open", "contributes.hooks", hooks.some((h) => h.hook === "open")],
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

  /** @type {Set<string>} */
  const seenHooks = new Set();
  hooks.forEach((hook, i) => {
    const path = `contributes.hooks[${String(i)}]`;
    const kind = hook.hook;
    const filterOk =
      hook.filter === undefined ||
      (kind !== "beforeSend" &&
        typeof hook.filter === "string" &&
        hook.filter.trim().length > 0 &&
        charCount(hook.filter) <= 500);
    const timeoutOk = hook.timeoutMs === undefined || isIntegerIn(hook.timeoutMs, 50, 10_000);
    if (
      typeof kind !== "string" ||
      !Object.hasOwn(HOOK_PERMISSIONS, kind) ||
      !filterOk ||
      !timeoutOk
    ) {
      issue("invalid-hook", path, kind);
    } else if (seenHooks.has(kind)) {
      issue("duplicate-hook", path, kind);
    } else {
      seenHooks.add(kind);
    }
  });

  /**
   * @param {string} name
   * @param {(item: Record<string, unknown>) => boolean} valid
   */
  const extensions = (name, valid) => {
    /** @type {Set<string>} */
    const seen = new Set();
    list(name).forEach((item, i) => {
      const path = `contributes.${name}[${String(i)}]`;
      if (!isKebabName(item.id) || !valid(item)) {
        issue("invalid-extension", path, item.id);
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
      (items(p.mimeTypes).length > 0 || items(p.extensions).length > 0) &&
      (p.maxBytes === undefined || isIntegerIn(p.maxBytes, 1, MAX_PREVIEW_INPUT)) &&
      matchersOk(p.mimeTypes, p.extensions),
  );
  extensions("attachmentActions", (a) => isTitle(a.title) && matchersOk(a.mimeTypes, a.extensions));
  extensions("linkActions", (a) => {
    const hosts = a.hosts ?? [];
    return (
      isTitle(a.title) && Array.isArray(hosts) && hosts.length <= 20 && hosts.every(isNetworkHost)
    );
  });

  /** @type {Set<string>} */
  const jobs = new Set();
  list("jobs").forEach((job, i) => {
    const path = `contributes.jobs[${String(i)}]`;
    const events = job.events ?? [];
    const valid =
      isKebabName(job.id) &&
      (job.title === undefined || isTitle(job.title)) &&
      Array.isArray(events) &&
      events.every((/** @type {unknown} */ e) => typeof e === "string" && JOB_EVENTS.includes(e)) &&
      (job.schedule !== undefined || events.length > 0) &&
      (job.schedule === undefined || isCron(job.schedule));
    if (!valid || !isKebabName(job.id)) issue("invalid-job", path, job.id);
    else if (jobs.has(job.id)) issue("duplicate-job", path, job.id);
    else jobs.add(job.id);
  });

  /** @type {Set<string>} */
  const tools = new Set();
  list("mcpTools").forEach((tool, i) => {
    const path = `contributes.mcpTools[${String(i)}]`;
    const nameOk = isKebabName(tool.id) && `${pluginId}__${tool.id}`.length <= MAX_MCP_TOOL_NAME;
    const descriptionOk =
      typeof tool.description === "string" &&
      tool.description.trim().length > 0 &&
      charCount(tool.description) <= 1000;
    const schema = tool.inputSchema;
    const schemaOk =
      schema === undefined ||
      (isRecord(schema) &&
        schema.type === "object" &&
        Buffer.byteLength(JSON.stringify(schema)) <= MAX_MCP_TOOL_SCHEMA);
    const scopes = tool.scopes ?? ["read"];
    const scopesOk =
      Array.isArray(scopes) &&
      scopes.length > 0 &&
      scopes.every((/** @type {unknown} */ s) => typeof s === "string" && MCP_SCOPES.includes(s));
    if (!nameOk || !isKebabName(tool.id) || !descriptionOk || !schemaOk || !scopesOk) {
      issue("invalid-mcp-tool", path, tool.id);
    } else if (tools.has(tool.id)) {
      issue("duplicate-mcp-tool", path, tool.id);
    } else {
      tools.add(tool.id);
    }
  });
}
