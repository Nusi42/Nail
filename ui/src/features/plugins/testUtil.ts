/**
 * Test helpers of the plug-in feature: install the example plug-in in the
 * mock core, wire the feature's event handler like `App.tsx` does, and
 * render with a layout that knows the plug-in's commands.
 */
import { act } from "@testing-library/react";
import type { PluginGrant, PluginInfo, ResolvedLayout } from "../../api/generated";
import type { Env } from "../../test/harness";
import { handlePluginsEvent } from ".";
import { withPluginActions } from "./commands";
import { followUpManifest } from "./mockHost";
import { pluginKeys } from "./queries";
import { type AttachmentInfo } from "../../api/generated";

export const PACKAGE = { kind: "file", path: "C:/plugins/follow-up.nailplugin" } as const;
export const REMIND = "plugin.follow-up.remind";

export const ALL_GRANTS: PluginGrant[] = (followUpManifest().permissions ?? []).map(
  (permission) => ({ permission, accounts: null }),
);

/** Installs the example plug-in in the mock core; returns it. */
export async function installFollowUp(
  env: Env,
  grants: PluginGrant[] = ALL_GRANTS,
): Promise<PluginInfo> {
  const preview = await env.transport.send({ type: "PreviewPluginInstall", source: PACKAGE });
  if (preview.type !== "PluginInstallPreview") throw new Error("no preview");
  const r = await env.transport.send({
    type: "InstallPlugin",
    source: PACKAGE,
    package_sha256: preview.preview.package_sha256,
    grants,
    enable: true,
  });
  if (r.type !== "Plugin") throw new Error(JSON.stringify(r));
  env.queryClient.setQueryData(pluginKeys.list, [r.plugin]);
  return r.plugin;
}

/** Routes core events to the feature, like the app shell. */
export function wireEvents(env: Env): () => void {
  return env.client.subscribe((event) => {
    handlePluginsEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    });
  });
}

/** The mock layout with the plug-in command in the reader toolbar and on a key. */
export function pluginLayout(env: Env, plugins: PluginInfo[]): ResolvedLayout {
  const layout = structuredClone(env.transport.data.layout);
  layout.toolbars.reader = [
    ...(layout.toolbars.reader ?? []),
    { type: "action", action: REMIND, args: [], label: null, icon: null },
  ];
  layout.keys = [...layout.keys, { context: "global", key: "shift+r", action: REMIND, args: [] }];
  return withPluginActions(layout, plugins);
}

/**
 * Lets queued mock work (`later(…, 0)`) and promise chains run, inside
 * `act` so React commits what they change.
 */
export async function tick(ms = 20): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, ms));
  });
}

export { must } from "../../test/must";

/** A mock message with two attachments (a PDF, then a CSV). */
export function withAttachments(env: Env): {
  messageId: string;
  pdf: AttachmentInfo;
  csv: AttachmentInfo;
} {
  const m = env.transport.data.messages.find(
    (x) => (env.transport.data.bodies.get(x.id)?.attachments.length ?? 0) > 1,
  );
  const body = m ? env.transport.data.bodies.get(m.id) : undefined;
  if (!m || !body?.attachments[0] || !body.attachments[1]) throw new Error("no attachments");
  return { messageId: m.id, pdf: body.attachments[0], csv: body.attachments[1] };
}
