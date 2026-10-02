/**
 * "Attachment check" — a before-send hook: when the subject or the text
 * the user wrote mentions an attachment ("attached", "enclosed", „anbei“)
 * but nothing is attached, it stops sending with a reason. Nail names the
 * plug-in in the message and offers "Send anyway", so the check can never
 * keep a message from being sent.
 *
 * The keywords come from the plug-in's catalogs (`attachment-keywords` in
 * locales/<language>.ftl, read with `nail.l10n.t`), so each language has
 * its own list, plus the user's own words from the settings.
 *
 * Permissions (manifest.json): hooks.beforeSend only — the hook gets the
 * outgoing message; the plug-in reads nothing else and changes nothing.
 */
import { definePlugin, nail, veto, type PluginOutgoingMessage } from "@nail/plugin-api";

import { findMention, ownText, parseKeywords } from "./check";

/** The keywords of Nail's language plus the user's own. */
function keywords() {
  const own = nail.settings.get("extra-keywords");
  return parseKeywords(
    `${nail.l10n.t("attachment-keywords")},${typeof own === "string" ? own : ""}`,
  );
}

/** The before-send hook. */
export function checkAttachments(message: PluginOutgoingMessage) {
  if (message.attachments.length > 0) return null;
  const checkQuoted = nail.settings.get("check-quoted") === true;
  const text = `${message.subject}\n${ownText(message.text, checkQuoted)}`;
  const keyword = findMention(text, keywords());
  return keyword ? veto(nail.l10n.t("veto-reason", { keyword })) : null;
}

export default definePlugin({
  hooks: { beforeSend: checkAttachments },
});
