/**
 * Dialog `about-nail` ("About Nail", action `system.about`): the version,
 * how and where Nail is installed, the update channel, and the updater's
 * state with "Check now", release notes, progress, "Restart to install" —
 * or why updates are off (policy). Outside the desktop app (remote UI,
 * `nail serve`) it says that these facts exist only there.
 */
import { errorMessageOf } from "../../api/client";
import type { InstallationInfo } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { channelMessageId, installKindMessageId, installScopeMessageId } from "./model";
import { useInstallation, useUpdateStatus } from "./queries";
import { UpdateSection } from "./UpdateSettings";

function Fact({ name, label, value }: { name: string; label: string; value: string }) {
  return (
    <li data-part="about-nail.fact" data-fact={name}>
      <span data-part="about-nail.fact.label">{label}</span>
      <span data-part="about-nail.fact.value">{value}</span>
    </li>
  );
}

function Facts({ info }: { info: InstallationInfo }) {
  const t = useT();
  const status = useUpdateStatus();
  const channel = status.data?.settings.channel ?? info.forced_channel;
  return (
    <ul data-part="about-nail.facts" aria-label={t("system-about-facts")}>
      <Fact name="version" label={t("system-about-version")} value={info.version} />
      <Fact
        name="installer"
        label={t("system-about-installer")}
        value={t(installKindMessageId(info.kind))}
      />
      {info.scope !== null && (
        <Fact
          name="scope"
          label={t("system-about-scope")}
          value={t(installScopeMessageId(info.scope))}
        />
      )}
      <Fact name="architecture" label={t("system-about-architecture")} value={info.architecture} />
      {channel !== null && (
        <Fact
          name="channel"
          label={t("system-about-channel")}
          value={t(channelMessageId(channel))}
        />
      )}
      {info.install_dir !== null && (
        <Fact name="folder" label={t("common-disk-folder")} value={info.install_dir} />
      )}
      {info.webview_version !== null && (
        <Fact name="webview" label={t("system-about-webview")} value={info.webview_version} />
      )}
      <Fact
        name="mapi"
        label={t("system-about-mapi")}
        value={t(info.mapi_installed ? "system-about-mapi-yes" : "system-about-mapi-no")}
      />
    </ul>
  );
}

/** The content of "About Nail". */
export function AboutNail() {
  const t = useT();
  const installation = useInstallation();
  return (
    <div data-part="about-nail">
      <h3 data-part="about-nail.title">{t("common-nail")}</h3>
      <p data-part="about-nail.tagline">{t("system-about-tagline")}</p>
      {installation.isError && (
        <p data-part="about-nail.unavailable" role="status">
          {t(errorMessageOf(installation.error).id, errorMessageOf(installation.error).args)}
        </p>
      )}
      {installation.data && (
        <>
          <Facts info={installation.data} />
          <section data-part="about-nail.updates" aria-label={t("system-updates-title")}>
            <h4 data-part="about-nail.section.title">{t("system-updates-title")}</h4>
            <UpdateSection showInstallation={false} showOptions={false} />
          </section>
        </>
      )}
    </div>
  );
}

/** Dialog `about-nail` (title `window-about-nail`). */
export function AboutNailDialog() {
  return <AboutNail />;
}
