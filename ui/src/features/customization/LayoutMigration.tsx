/**
 * Offering the M5b default layout to users with a layout of their own
 * (WP165, ADR 0114). A `layout.kdl` written before the navigation rail keeps
 * working: its own `window "main"` still shows and switching to a view opens
 * the view's window. Once, Nail offers the new default through the merge
 * dialog — prefilled with the shipped `defaults/layout.kdl`, so the user
 * picks what to take over (usually the `main` window) and nothing changes
 * until they apply it. "Not now" and "Review" are both remembered in the UI
 * state, so the offer never comes back by itself.
 */
import { useId } from "react";
import defaultLayoutText from "../../../../defaults/layout.kdl?raw";
import type { LayoutNode, ResolvedLayout } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useSetUiState, useUiState } from "../../state/uiState";
import { useUiStoreApi } from "../../state/uiStore";
import { DIALOGS } from "./names";
import { customizationStore } from "./state";

/** UI-state key: `"dismissed"` or `"reviewed"` once the user answered. */
export const UI_STATE_LAYOUT_OFFER = "customize.layout-offer";

/** The widget kind that shows views in the main window. */
const VIEW_HOST = "view-host";

/** The shipped default layout (`defaults/layout.kdl`), the incoming file of the offer. */
export const DEFAULT_LAYOUT_TEXT: string = defaultLayoutText;

function hasWidget(node: LayoutNode, kind: string): boolean {
  if (node.type === "widget") return node.kind === kind;
  return node.children.some((child) => hasWidget(child, kind));
}

/**
 * Whether `layout` is a pre-rail layout of the user's: it has views (the
 * default's), but its main window shows none of them in a view host.
 */
export function needsLayoutMigration(layout: ResolvedLayout): boolean {
  if (Object.keys(layout.views).length === 0) return false;
  const main = layout.windows.main ?? [];
  return main.length > 0 && !main.some((bp) => hasWidget(bp.root, VIEW_HOST));
}

/** The offer, shown once over the main window (an app effect). */
export function LayoutMigration() {
  const t = useT();
  const layout = useLayoutConfig();
  const uiState = useUiState();
  const setUiState = useSetUiState();
  const store = useUiStoreApi();
  const titleId = useId();
  if (!uiState.isSuccess || !needsLayoutMigration(layout)) return null;
  if (Object.hasOwn(uiState.data, UI_STATE_LAYOUT_OFFER)) return null;

  const answer = (value: "dismissed" | "reviewed") => {
    setUiState.mutate({ key: UI_STATE_LAYOUT_OFFER, value });
  };
  const review = () => {
    customizationStore.getState().prefillMerge({ file: "layout", incoming: DEFAULT_LAYOUT_TEXT });
    store.getState().openWindow(DIALOGS.merge);
    answer("reviewed");
  };

  return (
    <section data-part="layout-offer" aria-labelledby={titleId}>
      <h2 data-part="layout-offer.title" id={titleId}>
        {t("customize-layout-offer-title")}
      </h2>
      <p data-part="layout-offer.text">{t("customize-layout-offer-text")}</p>
      <div data-part="layout-offer.actions">
        <button type="button" data-part="form.button" data-variant="primary" onClick={review}>
          {t("customize-layout-offer-review")}
        </button>
        <button
          type="button"
          data-part="form.button"
          onClick={() => {
            answer("dismissed");
          }}
        >
          {t("common-not-now")}
        </button>
      </div>
    </section>
  );
}
