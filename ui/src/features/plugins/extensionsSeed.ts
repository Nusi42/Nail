/**
 * Demo plug-ins for the M4a extension points in the mock core (demo mode,
 * tests, screenshots): each one installed, running and granted, with a
 * simulated behaviour (`ExtensionBehaviour`) and a little history.
 *
 * - `attachment-check` — before-send hook: vetoes a message that mentions
 *   an attachment but has none (the design's example).
 * - `link-guard` — open hook: a banner (with a button) and a badge on mail
 *   from outside the organisation; a link action on every link.
 * - `file-preview` — CSV tables and PDF first pages as attachment
 *   previews; an attachment action.
 * - `snippets` — composer actions (insert / transform text) and a
 *   Markdown block rendered at send time.
 * - `digest` — background jobs on a schedule and on new mail; MCP tool.
 * Plug-in texts are the plug-ins' own (their manifests and answers).
 */
import type {
  AttachmentInfo,
  Draft,
  PluginGrant,
  PluginInfo,
  PluginManifest,
  PluginPreview,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { recordHookCall, type ExtensionBehaviour } from "./extensionsMock";

/** A 1×1 PNG (the "first page" of the demo PDF preview). */
export const DEMO_PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";

const INSTALLED = "2026-09-30T08:00:00Z";

function info(manifest: PluginManifest, grants?: PluginGrant[]): PluginInfo {
  return {
    id: manifest.id,
    manifest,
    source: { kind: "file", path: `C:/plugins/${manifest.id}.nailplugin` },
    enabled: true,
    state: "running",
    grants:
      grants ?? (manifest.permissions ?? []).map((permission) => ({ permission, accounts: null })),
    storage: { keys: 0, used_bytes: 0, quota_bytes: 5_242_880 },
    package_sha256: manifest.id.padEnd(64, "0").slice(0, 64),
    installed_at: INSTALLED,
    updated_at: INSTALLED,
    error: null,
  };
}

export function attachmentCheckManifest(): PluginManifest {
  return {
    id: "attachment-check",
    name: "Attachment check",
    version: "1.2.0",
    apiVersion: "1",
    main: "dist/index.js",
    description: "Stops a message that mentions an attachment but has none.",
    permissions: ["hooks.beforeSend"],
    contributes: { hooks: [{ hook: "beforeSend", timeoutMs: 500 }] },
  };
}

/** Whether a draft mentions an attachment it does not have. */
export function forgotAttachment(draft: Draft): boolean {
  const text = [draft.subject, draft.text, draft.markdown ?? "", draft.html ?? ""].join("\n");
  return /\battach(?:ed|ments?)?\b/i.test(text) && draft.attachments.length === 0;
}

const attachmentCheck: ExtensionBehaviour = {
  beforeSend: (draft) =>
    forgotAttachment(draft)
      ? { veto: { reason: "The message mentions an attachment, but nothing is attached." } }
      : undefined,
};

export function linkGuardManifest(): PluginManifest {
  return {
    id: "link-guard",
    name: "Link guard",
    version: "0.9.0",
    apiVersion: "1",
    main: "dist/index.js",
    description: "Marks mail from outside your organisation and checks links.",
    permissions: ["mail.read", "hooks.open", "notifications"],
    contributes: {
      hooks: [{ hook: "open", filter: "-from:@example.com" }],
      linkActions: [{ id: "check", title: "Check link reputation", icon: "shield" }],
    },
  };
}

const linkGuard: ExtensionBehaviour = {
  open: (message) =>
    message.from && !message.from.endsWith("@example.com")
      ? {
          banners: [
            {
              id: "external",
              level: "warning",
              text: `This message comes from outside your organisation (${message.from}). Be careful with links.`,
              buttons: [{ id: "report", label: "Report as phishing" }],
            },
          ],
          badges: [
            { text: "External", level: "warning", tooltip: "Sent from outside example.com" },
          ],
        }
      : {},
  actions: {
    check: (api, target, clientId) => {
      if (target.kind !== "link") return;
      api.notify({ title: "Link looks safe", body: target.url, level: "success" }, clientId);
    },
  },
};

export function filePreviewManifest(): PluginManifest {
  return {
    id: "file-preview",
    name: "File preview",
    version: "2.0.1",
    apiVersion: "1",
    main: "dist/index.js",
    description: "Previews CSV tables and PDF documents.",
    permissions: ["attachments", "notifications"],
    contributes: {
      attachmentPreviews: [
        { id: "csv", title: "Table", mimeTypes: ["text/csv"], extensions: ["csv"] },
        { id: "pdf", title: "First page", mimeTypes: ["application/pdf"], maxBytes: 5_000_000 },
      ],
      attachmentActions: [
        { id: "summary", title: "Show file summary", mimeTypes: ["text/csv"], extensions: ["csv"] },
      ],
    },
  };
}

function csvPreview(attachment: AttachmentInfo): PluginPreview {
  return {
    kind: "tree",
    tree: [
      { type: "text", value: attachment.filename ?? "table.csv", style: "heading" },
      {
        type: "table",
        columns: [{ label: "Quarter" }, { label: "Revenue" }, { label: "Growth" }],
        rows: [
          ["Q1", "1.2 M", "4 %"],
          ["Q2", "1.4 M", "17 %"],
          ["Q3", "1.5 M", "7 %"],
        ],
      },
    ],
  };
}

const filePreview: ExtensionBehaviour = {
  previews: {
    csv: csvPreview,
    pdf: (attachment) => ({
      kind: "image",
      mimeType: "image/png",
      data: DEMO_PNG,
      alt: `First page of ${attachment.filename ?? "the document"}`,
    }),
  },
  actions: {
    summary: (api, target, clientId) => {
      if (target.kind !== "attachment") return;
      api.notify({ title: "3 rows, 3 columns", level: "info" }, clientId);
    },
  },
};

export function snippetsManifest(): PluginManifest {
  return {
    id: "snippets",
    name: "Snippets",
    version: "1.0.0",
    apiVersion: "1",
    main: "dist/index.js",
    description: "Inserts text snippets and ticket cards.",
    permissions: ["composer"],
    contributes: {
      composerActions: [
        {
          id: "greeting",
          title: "Insert greeting",
          icon: "edit",
          description: "Adds a greeting at the cursor",
        },
        { id: "shout", title: "Make selection uppercase" },
      ],
      markdownBlocks: [
        { id: "ticket", title: "Ticket card", template: "id: NAIL-123\nstatus: open" },
      ],
    },
  };
}

const snippets: ExtensionBehaviour = {
  actions: {
    greeting: (_api, target, _clientId, core) => {
      if (target.kind !== "composer") return;
      core.emit({
        type: "ComposerEditRequested",
        draft_id: target.draft_id,
        client_id: null,
        by: { actor: { kind: "plugin", plugin_id: "snippets" }, name: "Snippets" },
        edit: { op: "insert", text: "Kind regards,\nAda", format: "text" },
      });
    },
    shout: (_api, target, _clientId, core) => {
      if (target.kind !== "composer") return;
      core.emit({
        type: "ComposerEditRequested",
        draft_id: target.draft_id,
        client_id: null,
        by: { actor: { kind: "plugin", plugin_id: "snippets" }, name: "Snippets" },
        edit: {
          op: "replace-selection",
          text: target.selection.text.toUpperCase(),
          format: "text",
        },
      });
    },
  },
};

export function digestManifest(): PluginManifest {
  return {
    id: "digest",
    name: "Daily digest",
    version: "1.1.0",
    apiVersion: "1",
    main: "dist/index.js",
    description: "Sends you a morning summary of unread mail.",
    permissions: ["mail.read", "background", "notifications", "mcp.expose"],
    contributes: {
      jobs: [
        { id: "morning", title: "Morning digest", schedule: "0 8 * * 1-5" },
        { id: "count", title: "Count new mail", events: ["message.received"] },
      ],
      mcpTools: [{ id: "digest", description: "Returns today's digest of unread mail." }],
    },
  };
}

/** The demo extension plug-ins (manifests). */
export function demoExtensionManifests(): PluginManifest[] {
  return [
    attachmentCheckManifest(),
    linkGuardManifest(),
    filePreviewManifest(),
    snippetsManifest(),
    digestManifest(),
  ];
}

const BEHAVIOURS: Record<string, ExtensionBehaviour> = {
  "attachment-check": attachmentCheck,
  "link-guard": linkGuard,
  "file-preview": filePreview,
  snippets,
  digest: { jobs: { morning: () => "ok", count: () => "ok" } },
};

/**
 * Installs the demo plug-ins with their behaviours and some history (job
 * runs, hook calls, CPU time); gives the first message with a PDF a CSV
 * attachment too. Returns the installed plug-ins.
 */
export function installDemoExtensions(core: MockCore): PluginInfo[] {
  const state = core.data.features.plugins;
  const installed = demoExtensionManifests().map((m) => info(m));
  for (const plugin of installed) {
    state.plugins = [...state.plugins.filter((p) => p.id !== plugin.id), plugin];
    state.extensions.behaviours[plugin.id] = BEHAVIOURS[plugin.id] ?? {};
  }
  const now = Date.now();
  const hour = 3_600_000;
  state.extensions.activity.digest = {
    jobs: {
      morning: {
        running: false,
        last_run_at: new Date(now - 2 * hour).toISOString(),
        last_outcome: "ok",
        last_error: null,
        next_run_at: new Date(now + 22 * hour).toISOString(),
        run_count: 14,
        failure_count: 1,
      },
      count: {
        running: false,
        last_run_at: new Date(now - 300_000).toISOString(),
        last_outcome: "timeout",
        last_error: "the job ran longer than 60 s",
        next_run_at: null,
        run_count: 230,
        failure_count: 3,
      },
    },
    hooks: {},
    cpuMs: 18_400,
  };
  for (let i = 0; i < 40; i++)
    recordHookCall(core, "attachment-check", "beforeSend", i === 7 ? "vetoed" : "ok", 3 + (i % 5));
  recordHookCall(core, "link-guard", "open", "timed-out", 1000, "timed out after 1000 ms");
  for (let i = 0; i < 25; i++) recordHookCall(core, "link-guard", "open", "ok", 2 + (i % 3));
  const pdf = core.data.messages.find((m) => core.data.bodies.get(m.id)?.attachments.length);
  const body = pdf ? core.data.bodies.get(pdf.id) : undefined;
  if (body?.attachments.length === 1) {
    body.attachments.push({
      index: 1,
      filename: "q3-figures.csv",
      mime: "text/csv",
      size: 812,
      inline: false,
      shown_in_body: false,
    });
  }
  if (state.host) {
    state.host.behaviours["link-guard"] = {
      onAction: (api, target, componentId, _values, clientId) => {
        if (target.kind !== "banner" || componentId !== "report") return;
        api.notify({ title: "Reported as phishing", level: "success" }, clientId);
      },
    };
  }
  core.emit({ type: "PluginsChanged" });
  return installed;
}
