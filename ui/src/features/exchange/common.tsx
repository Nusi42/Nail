/** Small shared pieces of the Exchange UI. */
import { useQueryClient } from "@tanstack/react-query";
import { useId } from "react";
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
