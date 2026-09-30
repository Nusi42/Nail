/**
 * A quick check of `manifest.json` before packing. Nail's own parser
 * (`PluginManifest::parse` in `crates/nail-api`) is authoritative and runs
 * again on install; this mirrors its most common rules so authors see the
 * problems while building. Issue kinds use the same names as Nail's
 * (`invalid-id`, `unknown-command`, …).
 */

/** Permissions of the preview API (plus `network:<host>`). */
export const PREVIEW_PERMISSIONS = [
  "mail.read",
  "mail.read.body",
  "calendar.read",
  "ui.dialogs",
  "notifications",
  "storage",
];

/** Permissions of the full API (M4) — refused in the preview. */
export const FUTURE_PERMISSIONS = [
  "mail.read.encrypted",
  "mail.organize",
  "mail.send",
  "calendar.write",
  "contacts.read",
  "contacts.write",
  "tasks.read",
  "tasks.write",
  "hooks.arrival",
  "hooks.beforeSend",
  "ui.webview",
  "mcp.expose",
];

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
    } else if (!PREVIEW_PERMISSIONS.includes(p)) {
      issue("unknown-permission", path, p);
    }
  });
  if (seen.has("mail.read.body") && !seen.has("mail.read")) {
    issue("body-without-read", "permissions", "mail.read.body");
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
  /** @type {[string, number][]} */
  const counts = [
    ["commands", 100],
    ["placements", 100],
    ["keys", 100],
    ["panels", 50],
    ["settings", 50],
  ];
  for (const [name, max] of counts) {
    if ((c[name] ?? []).length > max) issue("too-many-contributions", `contributes.${name}`);
  }
  return issues;
}
