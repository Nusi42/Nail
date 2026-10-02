/**
 * Launch requests (M2b, ADR 0041): what the desktop app was asked to open
 * when Windows started it — a `mailto:` link opens the composer, a `.eml`
 * file is imported into "Opened files" and shown; an `.ics` / `.vcf` file
 * opens the import dialog with its preview, a `webcal:` link the
 * subscription form (M3a); a draft another program prepared (Simple MAPI,
 * M4b) opens in the composer. Taken from the core once at start-up
 * and again after every `LaunchRequested` event (a second start forwarded
 * its targets). Never in a remote-UI session (desktop only).
 */
import { useEffect, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LaunchRequest, LocalizedMessage } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { ComposeContext } from "../../state/compose";
import { useComposeContext } from "../../state/hooks";
import { startMailtoCompose } from "../composer";
import { IMPORT_DIALOG, pimStoreFor, SOURCES_DIALOG } from "../pim";
import { remoteStore } from "../remote";
import { openProgramDraft } from "../system";

/** A notice the user dismisses: message id and arguments. */
export interface LaunchNotice {
  id: string;
  args?: Record<string, string>;
  /** Why it failed: the core's localized message (its `reason`). */
  reason?: LocalizedMessage;
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
        // The core's localized reason (never its English diagnostic).
        return { id: "launch-open-failed", reason: errorMessageOf(error) };
      }
    case "SubscribeCalendar":
      pimStoreFor(ctx.store).getState().setSubscribeUrl(request.url);
      ctx.store.getState().openWindow(SOURCES_DIALOG);
      return null;
    case "ImportCalendarFile":
    case "ImportContactsFile":
      pimStoreFor(ctx.store).getState().setImportPath(request.path);
      ctx.store.getState().openWindow(IMPORT_DIALOG);
      return null;
    case "ComposeDraft":
      // M4b (ADR 0087): recipients, text and files are in the draft; the
      // composer notes the program (mapi-draft-from-program).
      return openProgramDraft(ctx, request);
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
      <p data-part="launch-notice.text">
        {t(notice.id, {
          ...notice.args,
          ...(notice.reason ? { reason: t(notice.reason.id, notice.reason.args) } : {}),
        })}
      </p>
      <button type="button" data-part="form.button" onClick={() => setNotice(null)}>
        {t("launch-dismiss")}
      </button>
    </div>
  );
}
