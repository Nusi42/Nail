/**
 * The offer to turn a layout's strip-era reader stacks into the card feed
 * (P2 WP203; `./readerMigration.ts`), like the M5b layout offer
 * (`../customization/LayoutMigration.tsx`): shown once, never modal, both
 * answers remembered in the UI state. "Use cards" writes the change as one
 * edit (undoable); "Not now" leaves the layout alone.
 */
import { useId, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useClient } from "../../state/queries";
import { useSetUiState, useUiState } from "../../state/uiState";
import { errorMessageIdOf } from "../../api/client";
import { UI_STATE_LAYOUT_OFFER, needsLayoutMigration } from "../customization/LayoutMigration";
import { UI_STATE_READER_OFFER, needsReaderMigration, readerMigrationPatches } from "./readerStack";

export function ReaderOffer() {
  const t = useT();
  const layout = useLayoutConfig();
  const client = useClient();
  const uiState = useUiState();
  const setUiState = useSetUiState();
  const titleId = useId();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!uiState.isSuccess || !needsReaderMigration(layout)) return null;
  if (Object.hasOwn(uiState.data, UI_STATE_READER_OFFER)) return null;
  // One offer at a time: the M5b layout offer (it may bring the new reader
  // along with the new default) goes first.
  if (needsLayoutMigration(layout) && !Object.hasOwn(uiState.data, UI_STATE_LAYOUT_OFFER)) {
    return null;
  }

  const answer = (value: "dismissed" | "migrated") => {
    setUiState.mutate({ key: UI_STATE_READER_OFFER, value });
  };
  const migrate = () => {
    setBusy(true);
    setError(null);
    client
      .request({ type: "UpdateLayout", patches: readerMigrationPatches(layout) }, "Done")
      .then(() => {
        answer("migrated");
      })
      .catch((e: unknown) => {
        setError(errorMessageIdOf(e));
      })
      .finally(() => {
        setBusy(false);
      });
  };

  return (
    <section data-part="layout-offer" data-offer="reader" aria-labelledby={titleId}>
      <h2 data-part="layout-offer.title" id={titleId}>
        {t("thread-view-offer-title")}
      </h2>
      <p data-part="layout-offer.text">{t("thread-view-offer-text")}</p>
      {error !== null && (
        <p data-part="layout-offer.error" role="alert">
          {t(error)}
        </p>
      )}
      <div data-part="layout-offer.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          disabled={busy}
          onClick={migrate}
        >
          {t("thread-view-offer-apply")}
        </button>
        <button
          type="button"
          data-part="form.button"
          disabled={busy}
          onClick={() => {
            answer("dismissed");
          }}
        >
          {t("thread-view-offer-dismiss")}
        </button>
      </div>
    </section>
  );
}
