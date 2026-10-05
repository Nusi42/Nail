/**
 * Widget `default-apps`: whether Nail opens e-mail links, calendar links
 * and `.eml` / `.ics` / `.vcf` files, whether it is the mail program
 * other programs use for "Send to → Mail recipient" (Simple MAPI), and the
 * button "Choose in Default apps". Windows never lets an app make itself
 * the default, so the button opens Settings → Default apps on Nail's page
 * and the state is read again when the user comes back.
 *
 * Widget option: `show-mapi` (default `#true`) — the "Send to" row.
 */
import { useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { DefaultAppState, LocalizedMessage } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { defaultAppMessageId, defaultAppStateMessageId } from "./model";
import { QueryProblem } from "./QueryProblem";
import { useDefaultApps, useInstallation } from "./queries";

/** "Choose in Default apps": opens Windows Settings on Nail's page. */
export function ChooseDefaultAppsButton({ primary = false }: { primary?: boolean }) {
  const t = useT();
  const client = useClient();
  const [busy, setBusy] = useState(false);
  const [opened, setOpened] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const open = async () => {
    setBusy(true);
    setProblem(null);
    try {
      await client.request({ type: "OpenDefaultAppsSettings" }, "Done");
      setOpened(true);
    } catch (error) {
      setProblem(errorMessageOf(error));
    } finally {
      setBusy(false);
    }
  };
  return (
    <>
      <button
        type="button"
        data-part="form.button"
        data-action="choose-default-apps"
        data-variant={primary ? "primary" : undefined}
        disabled={busy}
        onClick={() => void open()}
      >
        {t("system-default-apps-choose")}
      </button>
      {opened && (
        <p data-part="default-apps.opened" role="status">
          {t("system-default-apps-opened")}
        </p>
      )}
      {problem && (
        <p data-part="default-apps.error" role="alert">
          {t(problem.id, problem.args)}
        </p>
      )}
    </>
  );
}

function Row({
  name,
  label,
  state,
  registered,
}: {
  name: string;
  label: string;
  state: DefaultAppState;
  registered: boolean;
}) {
  const t = useT();
  return (
    <li
      data-part="default-apps.item"
      data-target={name}
      data-state={registered ? state : "unregistered"}
    >
      <span data-part="default-apps.item.name">{label}</span>
      <span data-part="default-apps.item.state">
        {registered ? t(defaultAppStateMessageId(state)) : t("system-default-apps-unregistered")}
      </span>
    </li>
  );
}

/** The list of schemes / file types and the "Send to" row, with the button. */
export function DefaultAppsContent({ showMapi = true }: { showMapi?: boolean }) {
  const t = useT();
  const query = useDefaultApps();
  const installation = useInstallation();
  const data = query.data;
  const mapiInstalled = installation.data?.mapi_installed ?? true;
  return (
    <>
      {query.isError && <QueryProblem error={query.error} part="default-apps.error" />}
      {data && !data.status.registered && (
        <p data-part="default-apps.unregistered" role="status">
          {t("system-default-apps-not-registered")}
        </p>
      )}
      {data && (
        <ul data-part="default-apps.list" aria-label={t("system-default-apps-list")}>
          {data.status.associations.map((association) => (
            <Row
              key={association.target}
              name={association.target}
              label={t(defaultAppMessageId(association.target))}
              state={association.state}
              registered={association.registered}
            />
          ))}
          {showMapi && mapiInstalled && (
            <Row
              name="mapi"
              label={t("system-default-apps-mapi")}
              state={data.status.mapi.state}
              registered={data.status.mapi.registered}
            />
          )}
        </ul>
      )}
      {data && showMapi && !mapiInstalled && (
        <p data-part="form.hint" data-field="mapi">
          {t("system-default-apps-mapi-missing")}
        </p>
      )}
      {data && (
        <div data-part="default-apps.buttons">
          <ChooseDefaultAppsButton primary />
          <button
            type="button"
            data-part="form.button"
            data-action="refresh"
            disabled={query.isFetching}
            onClick={() => void query.refetch()}
          >
            {t("system-default-apps-refresh")}
          </button>
        </div>
      )}
    </>
  );
}

/** Widget `default-apps`. */
export function DefaultAppsWidget({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const title = node.label ? label(node.label) : t("system-default-apps-title");
  return (
    <section data-part="default-apps" data-node-id={node.id ?? undefined} aria-label={title}>
      <h3 data-part="default-apps.title">{t("system-default-apps-title")}</h3>
      <p data-part="default-apps.intro">{t("system-default-apps-intro")}</p>
      <DefaultAppsContent showMapi={optionBool(node, "show-mapi", true)} />
    </section>
  );
}
