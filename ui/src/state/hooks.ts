/**
 * Hooks that bundle what write and composer flows need, for widgets that
 * call them directly (buttons inside widgets rather than layout actions).
 */
import { useQueryClient } from "@tanstack/react-query";
import { useMemo } from "react";
import { useLocale, useT } from "../i18n/I18nProvider";
import type { ComposeContext } from "./compose";
import { useClient } from "./queries";
import { useUiStoreApi } from "./uiStore";

/** Client, UI store, query cache, translation and locale. */
export function useComposeContext(): ComposeContext {
  const client = useClient();
  const store = useUiStoreApi();
  const queryClient = useQueryClient();
  const t = useT();
  const locale = useLocale();
  return useMemo(
    () => ({ client, store, queryClient, t, locale }),
    [client, store, queryClient, t, locale],
  );
}
