/** Small shared pieces of the Exchange UI. */
import { useQueryClient } from "@tanstack/react-query";
import { useId, type ReactNode } from "react";
import type { CategoryColor, LocalizedMessage } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { useGeometry } from "../../theme/ThemeProvider";
import { categoryRgb, categoryTextRgb } from "./model";
import type { ExchangeWriteContext } from "./queries";

/** A problem text (the core's localized refusal or a mirrored issue). */
export function Problem({
  message,
  part = "exchange.problem",
}: {
  message: LocalizedMessage | null;
  part?: string;
}) {
  const t = useT();
  if (message === null) return null;
  return (
    <p data-part={part} role="alert">
      {t(message.id, message.args)}
    </p>
  );
}

/** A dialog's problem and its action row: Cancel first, then `children`. */
export function DialogActions({
  problem,
  onCancel,
  children,
}: {
  problem: LocalizedMessage | null;
  onCancel: () => void;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <>
      <Problem message={problem} />
      <div data-part="exchange.actions">
        <button type="button" data-part="form.button" onClick={onCancel}>
          {t("common-cancel")}
        </button>
        {children}
      </div>
    </>
  );
}

/** {@link DialogActions} with one danger button (delete, remove, hide). */
export function DangerActions({
  problem,
  busy,
  onCancel,
  onConfirm,
  label,
}: {
  problem: LocalizedMessage | null;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
  label: string;
}) {
  return (
    <DialogActions problem={problem} onCancel={onCancel}>
      <button
        type="button"
        data-part="form.button"
        data-variant="danger"
        disabled={busy}
        onClick={onConfirm}
      >
        {label}
      </button>
    </DialogActions>
  );
}

/** Client, query client and UI store for the feature's writes. */
export function useWriteContext(): ExchangeWriteContext & {
  store: ReturnType<typeof useUiStoreApi>;
} {
  return { client: useClient(), queryClient: useQueryClient(), store: useUiStoreApi() };
}

/** A category colour dot or chip background (theme geometry, never inline styles). */
export function CategorySwatch({
  color,
  part = "exchange.swatch",
}: {
  color: CategoryColor;
  part?: string;
}) {
  const geom = `exchange-swatch-${useId().replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  const rgb = categoryRgb(color);
  useGeometry(geom, rgb ? { "background-color": rgb, color: categoryTextRgb(color) ?? "" } : {});
  return <span data-part={part} data-geom={geom} data-color={color} aria-hidden="true" />;
}
