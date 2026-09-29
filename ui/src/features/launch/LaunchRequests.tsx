/**
 * Launch requests (M2b, ADR 0041): what the desktop app was asked to open
 * when Windows started it — a `mailto:` link opens the composer, a `.eml`
 * file is imported into "Opened files" and shown; calendars and contacts
 * are not supported yet and say so. Taken from the core once at start-up
 * and again after every `LaunchRequested` event (a second start forwarded
 * its targets). Never in a remote-UI session (desktop only).
 */
import { useEffect, useState } from "react";
import { NailError } from "../../api/client";
import type { LaunchRequest } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { ComposeContext } from "../../state/compose";
import { useComposeContext } from "../../state/hooks";
import { startMailtoCompose } from "../composer";
import { remoteStore } from "../remote";

/** A notice the user dismisses: message id and arguments. */
export interface LaunchNotice {
  id: string;
  args?: Record<string, string>;
}

/** Acts on one launch request; returns a notice to show, if any. */
export async function handleLaunch(
  ctx: ComposeContext,
  request: LaunchRequest,
): Promise<LaunchNotice | null> {
  switch (request.type) {
    case "Compose":
      await startMailtoCompose(ctx, request.mailto);
      return null;
    case "OpenMessageFile":
      try {
        const { message } = await ctx.client.request(
          { type: "ImportMessageFile", path: request.path },
          "MessageImported",
        );
        const ui = ctx.store.getState();
        ui.selectAccount(message.account_id);
        ui.selectFolder(message.folder_id);
        ui.selectMessage(message.id);
        return null;
      } catch (error) {
        const reason = error instanceof NailError ? error.message : String(error);
        return { id: "launch-open-failed", args: { reason } };
      }
    case "SubscribeCalendar":
    case "ImportCalendarFile":
      return { id: "launch-unsupported-calendar" };
    case "ImportContactsFile":
      return { id: "launch-unsupported-contacts" };
  }
}

/** Takes the pending launch requests and acts on them (app-shell effect). */
export function LaunchRequests() {
  const t = useT();
  const ctx = useComposeContext();
  const [notice, setNotice] = useState<LaunchNotice | null>(null);

  useEffect(() => {
    // A browser on the remote UI has no launch requests (desktop only).
    if (remoteStore.getState().session !== null) return;
    let live = true;
    let running = Promise.resolve();
    const take = () => {
      running = running.then(async () => {
        let requests: LaunchRequest[];
        try {
          ({ requests } = await ctx.client.request(
            { type: "TakeLaunchRequests" },
            "LaunchRequests",
          ));
        } catch {
          return; // An older core without launch requests.
        }
        for (const request of requests) {
          const shown = await handleLaunch(ctx, request).catch(() => null);
          if (live && shown !== null) setNotice(shown);
        }
      });
    };
    take();
    const off = ctx.client.subscribe((event) => {
      if (event.type === "LaunchRequested") take();
    });
    return () => {
      live = false;
      off();
    };
  }, [ctx]);

  if (notice === null) return null;
  return (
    <div data-part="launch-notice" role="status" aria-label={t("launch-notice-label")}>
      <p data-part="launch-notice.text">{t(notice.id, notice.args)}</p>
      <button type="button" data-part="form.button" onClick={() => setNotice(null)}>
        {t("launch-dismiss")}
      </button>
    </div>
  );
}
