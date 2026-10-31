/**
 * The mock core's answer to `OpenLink` (ADR 0206), as the core answers in
 * web mode (`nail_core::links`): web and phone links come back for a new
 * tab, a `mailto:` link's recipients for the composer (only the path; the
 * core decodes every field), anything else is refused.
 */
import type { Response } from "./generated";
import { errorResponse } from "./mockMail";

const MAILTO = /^mailto:/i;
const EXTERNAL = /^(?:https?:\/\/[^/]|tel:)/i;

export function answerOpenLink(url: string): Response {
  if (MAILTO.test(url)) {
    const path = decodeURIComponent(url.replace(MAILTO, "").split("?")[0] ?? "");
    const to = path
      .split(",")
      .map((a) => a.trim())
      .filter((a) => a !== "");
    return {
      type: "LinkOpened",
      outcome: {
        type: "Compose",
        mailto: { to, cc: [], bcc: [], subject: null, body: null, in_reply_to: null },
      },
    };
  }
  if (EXTERNAL.test(url)) {
    return { type: "LinkOpened", outcome: { type: "OpenInBrowser", url } };
  }
  return errorResponse("invalid_request", `links like ${url} are not opened`);
}
