/**
 * "Open in new window": the title-bar button that pops a window out into
 * its own OS window (M5c WP173). Shown only in the desktop app and only for
 * windows whose spec allows it (`WindowSpec.pop_out`; confirmations never
 * pop out); the browser UI keeps every window in the page.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useContext, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { LayoutContext } from "../../layout/LayoutContext";
import { useWindowSpecs } from "../../layout/windowSpecs";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import type { WindowSpec } from "../../api/generated";
import { resolveWindowSpec } from "../../state/windows";
import { Icon } from "../../widgets/Icon";
import { usePopOutsAvailable } from "./hooks";
import { errorMessageId, popOutWindow, windowTitleId } from "./model";

export function PopOutButton({ window, spec }: { window: string; spec?: WindowSpec }) {
  const t = useT();
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();
  const layout = useContext(LayoutContext);
  const specs = useWindowSpecs();
  const available = usePopOutsAvailable();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const resolved = spec ?? resolveWindowSpec(window, layout, specs);
  if (!available || !resolved.pop_out) return null;
  const label = error === null ? t("popout-open") : t("popout-open-error", { reason: t(error) });
  return (
    <button
      type="button"
      data-part="window.pop-out"
      data-state={error === null ? undefined : "error"}
      aria-label={label}
      title={label}
      disabled={busy}
      onClick={() => {
        setBusy(true);
        setError(null);
        popOutWindow({ client, store, queryClient }, window, t(windowTitleId(window)))
          .catch((e: unknown) => setError(errorMessageId(e)))
          .finally(() => setBusy(false));
      }}
    >
      <Icon name="external-link" part="window.pop-out.icon" />
    </button>
  );
}
