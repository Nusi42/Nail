/**
 * Settings labels of the built-in widgets (`ui/src/widgets`, registered in
 * `../../widgets/index.ts`): the account editor, the virtual-account editor
 * and toolbars. Kept here because the widgets folder belongs to another
 * package in M5b (ADR 0109); every feature registers its own.
 */
import type { SettingsWidgetInfo } from "../../layout/settingsIndex";

export const BUILTIN_SETTINGS: Readonly<Record<string, SettingsWidgetInfo>> = {
  "account-settings": {
    labels: [
      "settings-tab-accounts",
      "settings-accounts-label",
      "settings-add-account",
      "settings-general",
      "settings-name",
      "settings-email",
      "settings-enabled",
      "server-incoming",
      "server-outgoing",
      "server-host",
      "server-port",
      "server-tls",
      "server-username",
      "settings-new-password",
      "settings-poll-interval",
      "settings-pop3",
      "settings-leave-on-server",
      "settings-delete-after-days",
      "settings-delete-when-deleted",
      "settings-gmail",
      "settings-gmail-poll-interval",
      "settings-gmail-max-poll-interval",
      "settings-identities",
      "identity-name",
      "identity-email",
      "identity-reply-to",
      "identity-signature",
      "settings-sender-checks",
      "settings-authserv-ids",
      "settings-remove",
    ],
    keywords: "settings-keywords-account-settings",
  },
  "virtual-account-editor": {
    labels: [
      "settings-tab-virtual-accounts",
      "virtual-label",
      "virtual-new",
      "virtual-name",
      "virtual-members",
      "virtual-folders",
      "virtual-mapping-name",
      "virtual-identities",
      "virtual-default-identity",
      "virtual-rule-pattern",
      "virtual-rule-identity",
    ],
    keywords: "settings-keywords-virtual-account-editor",
  },
  toolbar: {
    labels: ["toolbar-customize"],
    keywords: "settings-keywords-toolbar",
  },
};
