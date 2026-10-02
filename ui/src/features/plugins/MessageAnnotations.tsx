/**
 * What plug-ins' open hooks add to a message (M4a, ADR 0079): banners
 * above the message and badges in its header. They never rewrite the
 * message itself. Each one names its plug-in (the theme can style them
 * per plug-in through `data-plugin`); plug-in text is plain text. Banner
 * buttons go back to the plug-in as `PluginUiAction` with the banner
 * target.
 *
 * Mount points (reader): `PluginBanners` above the message body,
 * `PluginBadges` in the message header. Both share one query.
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, MessageId, PluginBanner } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { Icon } from "../../widgets/Icon";
import { clientIdOf } from "./context";
import { useMessageAnnotations } from "./queries";

function Banner({ messageId, banner }: { messageId: MessageId; banner: PluginBanner }) {
  const t = useT();
  const client = useClient();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const level = banner.banner.level ?? "info";
  const press = async (componentId: string) => {
    setBusy(true);
    setProblem(null);
    try {
      await client.request(
        {
          type: "PluginUiAction",
          target: {
            kind: "banner",
            message_id: messageId,
            banner: `${banner.plugin_id}.${banner.banner.id}`,
          },
          component_id: componentId,
          values: {},
          client_id: clientIdOf(client),
        },
        "Done",
      );
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <li
      data-part="plugin-banner"
      data-plugin={banner.plugin_id}
      data-banner={banner.banner.id}
      data-level={level}
      aria-busy={busy}
    >
      <span data-part="plugin-banner.plugin">
        {t("plugin-banner-from", { plugin: banner.plugin_name })}
      </span>
      <span data-part="plugin-banner.text">{banner.banner.text}</span>
      {(banner.banner.buttons ?? []).length > 0 && (
        <span data-part="plugin-banner.buttons">
          {(banner.banner.buttons ?? []).map((button) => (
            <button
              key={button.id}
              type="button"
              data-part="plugin-banner.button"
              data-plugin={banner.plugin_id}
              data-id={button.id}
              disabled={busy}
              onClick={() => void press(button.id)}
            >
              {button.label}
            </button>
          ))}
        </span>
      )}
      {problem && (
        <span data-part="plugin-banner.error" role="alert">
          {t(problem.id, problem.args)}
        </span>
      )}
    </li>
  );
}

/** Banners of the plug-ins' open hooks for a message. */
export function PluginBanners({ messageId }: { messageId: MessageId }) {
  const t = useT();
  const annotations = useMessageAnnotations(messageId);
  const banners = annotations.data?.message_id === messageId ? annotations.data.banners : [];
  if (banners.length === 0) return null;
  return (
    <ul data-part="plugin-banners" aria-label={t("plugin-banners-label")}>
      {banners.map((banner) => (
        <Banner
          key={`${banner.plugin_id}.${banner.banner.id}`}
          messageId={messageId}
          banner={banner}
        />
      ))}
    </ul>
  );
}

/** Badges of the plug-ins' open hooks for a message header. */
export function PluginBadges({ messageId }: { messageId: MessageId }) {
  const t = useT();
  const annotations = useMessageAnnotations(messageId);
  const badges = annotations.data?.message_id === messageId ? annotations.data.badges : [];
  if (badges.length === 0) return null;
  return (
    <ul data-part="plugin-badges" aria-label={t("plugin-badges-label")}>
      {badges.map((badge, index) => {
        const description = t(badge.badge.tooltip ? "plugin-badge-tooltip" : "plugin-badge-from", {
          plugin: badge.plugin_name,
          tooltip: badge.badge.tooltip ?? "",
        });
        return (
          <li
            key={`${badge.plugin_id}-${index}`}
            data-part="plugin-badge"
            data-plugin={badge.plugin_id}
            data-level={badge.badge.level ?? "info"}
            title={description}
          >
            <Icon name={badge.badge.icon} part="plugin-badge.icon" />
            <span data-part="plugin-badge.text">{badge.badge.text}</span>
            <span data-part="plugin-badge.description">{description}</span>
          </li>
        );
      })}
    </ul>
  );
}
