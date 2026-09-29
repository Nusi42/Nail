/**
 * Root component: picks a transport, loads the resolved config from the core
 * and renders the main window from the layout, styled by the theme.
 */
import { QueryClient, QueryClientProvider, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, type ReactNode } from "react";
import { ActionsProvider } from "./actions/ActionsProvider";
import type { NailClient } from "./api/client";
import { errorMessageIdOf } from "./api/client";
import type { AccountStatus, ThemeMode } from "./api/generated";
import { normalizeLayout, normalizeTheme } from "./api/normalize";
import { createClient } from "./api/select";
import { I18nProvider, useT } from "./i18n/I18nProvider";
import { LayoutContext } from "./layout/LayoutContext";
import { WindowView } from "./layout/LayoutRenderer";
import { WindowLayer } from "./layout/WindowLayer";
import { WidgetRegistryContext, type WidgetRegistry } from "./layout/widgetRegistry";
import {
  ClientContext,
  queryKeys,
  useAccounts,
  useClient,
  useConfig,
  useFolders,
  type AppConfig,
} from "./state/queries";
import { UiStoreContext, createUiStore, useUi, useUiStoreApi, type UiStore } from "./state/uiStore";
import { ThemeProvider } from "./theme/ThemeProvider";
import { builtinWidgets } from "./widgets";

export interface AppProps {
  /** Client to use; by default one is created for the environment. */
  client?: NailClient;
  queryClient?: QueryClient;
  store?: UiStore;
  widgets?: WidgetRegistry;
  /** Window type to render. */
  windowName?: string;
}

const MODES: readonly ThemeMode[] = ["system", "light", "dark", "high-contrast"];

/** UI overrides from the page URL: `?lang=de`, `?mode=dark`. */
export function overridesFromUrl(search: string): {
  localeOverride: string | null;
  modeOverride: ThemeMode | null;
} {
  const params = new URLSearchParams(search);
  const mode = params.get("mode");
  return {
    localeOverride: params.get("lang"),
    modeOverride: MODES.find((m) => m === mode) ?? null,
  };
}

function createQueryClient(): QueryClient {
  return new QueryClient({
    defaultOptions: { queries: { retry: 1, refetchOnWindowFocus: false, staleTime: 5_000 } },
  });
}

export function App({
  client: givenClient,
  queryClient: givenQueryClient,
  store: givenStore,
  widgets,
  windowName,
}: AppProps) {
  const [client, setClient] = useState<NailClient | null>(givenClient ?? null);
  const [failed, setFailed] = useState(false);
  const [queryClient] = useState(() => givenQueryClient ?? createQueryClient());
  const [store] = useState(
    () => givenStore ?? createUiStore(overridesFromUrl(window.location.search)),
  );

  useEffect(() => {
    if (client) return;
    let cancelled = false;
    createClient().then(
      (created) => {
        if (cancelled) created.close();
        else setClient(created);
      },
      (error: unknown) => {
        console.error("nail: cannot create a client", error);
        if (!cancelled) setFailed(true);
      },
    );
    return () => {
      cancelled = true;
    };
  }, [client]);

  let content: ReactNode;
  if (client) {
    content = (
      <ClientContext.Provider value={client}>
        <Shell widgets={widgets ?? builtinWidgets} windowName={windowName ?? "main"} />
      </ClientContext.Provider>
    );
  } else {
    content = (
      <I18nProvider locale={store.getState().localeOverride}>
        <AppStatus
          state={failed ? "error" : "loading"}
          messageId={failed ? "error-unavailable" : "app-loading"}
        />
      </I18nProvider>
    );
  }

  return (
    <QueryClientProvider client={queryClient}>
      <UiStoreContext.Provider value={store}>{content}</UiStoreContext.Provider>
    </QueryClientProvider>
  );
}

function AppStatus({ state, messageId }: { state: string; messageId: string }) {
  const t = useT();
  return (
    <main data-part="app" data-state={state}>
      <div data-part="app.status" role={state === "error" ? "alert" : "status"}>
        {t(messageId)}
      </div>
    </main>
  );
}

/** Applies events pushed by the core to the query cache and UI state. */
function useCoreEvents(): void {
  const client = useClient();
  const queryClient = useQueryClient();
  const store = useUiStoreApi();
  useEffect(() => {
    const ui = () => store.getState();
    const invalidate = (...keys: (readonly unknown[])[]) => {
      for (const queryKey of keys) void queryClient.invalidateQueries({ queryKey });
    };
    const refreshMail = () =>
      invalidate(
        queryKeys.allFolders,
        queryKeys.allMessages,
        queryKeys.allThreads,
        queryKeys.allThreadDetails,
        queryKeys.allSearches,
      );
    const offEvents = client.subscribe((event) => {
      switch (event.type) {
        case "ConfigChanged":
          queryClient.setQueryData<AppConfig>(queryKeys.config, (old) => ({
            theme: normalizeTheme(event.theme),
            layout: normalizeLayout(event.layout),
            locale: old?.locale ?? null,
          }));
          ui().setConfigError(null);
          break;
        case "ConfigError":
          ui().setConfigError({
            file: event.file,
            line: event.line,
            column: event.column,
            message: event.message,
          });
          break;
        case "MailChanged":
          refreshMail();
          break;
        case "SyncFailed":
          ui().setSync(null);
          ui().setSyncError({ accountId: event.account_id, code: event.code });
          break;
        case "SyncProgress":
          ui().setSync({ accountId: event.account_id, done: event.done, total: event.total });
          ui().setSyncError(null);
          if (event.total > 0 && event.done >= event.total) refreshMail();
          break;
        case "NewMail":
          refreshMail();
          break;
        case "OpStatus":
          if (event.status.state === "done" || event.status.state === "failed") {
            ui().opFinished(event.status.op_id, event.status.error);
            // A failed write: show the core's state again.
            if (event.status.state === "failed") refreshMail();
          }
          break;
        case "SendStatus": {
          const last = ui().lastSend;
          if (last?.id === event.outbox_id) {
            ui().setLastSend({
              ...last,
              state: event.state,
              error: event.error,
              message: event.message,
            });
          }
          invalidate(queryKeys.outbox);
          break;
        }
        case "AccountStatus":
          queryClient.setQueryData<AccountStatus[]>(queryKeys.accountStatuses, (old = []) => [
            ...old.filter((s) => s.account_id !== event.status.account_id),
            event.status,
          ]);
          break;
        case "AccountsChanged":
          invalidate(
            queryKeys.accounts,
            queryKeys.accountsConfig,
            queryKeys.allIdentities,
            queryKeys.virtualAccounts,
            queryKeys.accountStatuses,
            queryKeys.allFolders,
            queryKeys.allMessages,
          );
          break;
        case "OAuthFinished":
          ui().setOAuthResult(event.flow_id, event.error);
          break;
      }
    });
    ui().setConnection(client.status());
    const offStatus = client.onStatus((status) => {
      const previous = ui().connection;
      ui().setConnection(status);
      if (status === "connected" && previous === "reconnecting") {
        void queryClient.invalidateQueries();
      }
    });
    return () => {
      offEvents();
      offStatus();
    };
  }, [client, queryClient, store]);
}

/** Selects the first account and its inbox until the user chooses otherwise. */
function useDefaultSelection(): void {
  const accounts = useAccounts();
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const selectAccount = useUi((s) => s.selectAccount);
  const selectFolder = useUi((s) => s.selectFolder);
  const folders = useFolders(accountId);

  useEffect(() => {
    const list = accounts.data;
    if (!list || list.length === 0) return;
    if (accountId === null || !list.some((a) => a.id === accountId)) {
      selectAccount((list[0] as (typeof list)[number]).id);
    }
  }, [accounts.data, accountId, selectAccount]);

  useEffect(() => {
    const list = folders.data;
    if (!list || list.length === 0) return;
    if (folderId === null || !list.some((f) => f.id === folderId)) {
      selectFolder((list.find((f) => f.role === "inbox") ?? list[0])?.id ?? null);
    }
  }, [folders.data, folderId, selectFolder]);
}

function Shell({ widgets, windowName }: { widgets: WidgetRegistry; windowName: string }) {
  const config = useConfig();
  const localeOverride = useUi((s) => s.localeOverride);
  const modeOverride = useUi((s) => s.modeOverride);
  useCoreEvents();
  useDefaultSelection();

  const locale = localeOverride ?? config.data?.locale ?? null;
  return (
    <I18nProvider locale={locale}>
      {config.data ? (
        <ThemeProvider theme={config.data.theme} modeOverride={modeOverride}>
          <LayoutContext.Provider value={config.data.layout}>
            <ActionsProvider layout={config.data.layout}>
              <WidgetRegistryContext.Provider value={widgets}>
                <main
                  data-part="app"
                  data-layout={config.data.layout.name}
                  data-theme={config.data.theme.name}
                >
                  <WindowView layout={config.data.layout} name={windowName} />
                  <WindowLayer layout={config.data.layout} />
                </main>
              </WidgetRegistryContext.Provider>
            </ActionsProvider>
          </LayoutContext.Provider>
        </ThemeProvider>
      ) : config.isError ? (
        <AppStatus state="error" messageId={errorMessageIdOf(config.error)} />
      ) : (
        <AppStatus state="loading" messageId="app-loading" />
      )}
    </I18nProvider>
  );
}
