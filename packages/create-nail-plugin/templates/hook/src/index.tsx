/**
 * A starter hook plug-in (see manifest.json):
 *
 * - before send: stops a message without a subject (the user can still
 *   "Send anyway" — a veto never blocks sending for good);
 * - on open: shows an "External" badge on mail from outside the domain of
 *   the `internal-domain` setting.
 *
 * Hooks run inside Nail with a time budget (manifest `timeoutMs`); a hook
 * that throws or runs too long is skipped. Texts come from locales/*.ftl
 * through `nail.l10n`.
 */
import { badge, definePlugin, nail, veto } from "@nail/plugin-api";

/** The domain of an address (`ada@example.com` → `example.com`). */
export function domainOf(email: string): string {
  return email.slice(email.lastIndexOf("@") + 1).toLowerCase();
}

export default definePlugin({
  hooks: {
    beforeSend(message) {
      if (message.subject.trim() === "") return veto(nail.l10n.t("veto-no-subject"));
      return null;
    },
    open({ message }) {
      const internal = String(nail.settings.get("internal-domain") ?? "").toLowerCase();
      const domain = message.from ? domainOf(message.from.email) : "";
      if (!internal || !domain || domain === internal || domain.endsWith(`.${internal}`)) {
        return null;
      }
      return {
        badges: [
          badge(nail.l10n.t("badge-external"), {
            level: "warning",
            tooltip: nail.l10n.t("badge-external-tooltip", { domain, internal }),
          }),
        ],
      };
    },
  },
});
