/**
 * Root component: picks a transport, loads the resolved config from the core
 * and renders the main window from the layout, styled by the theme.
 */
import { QueryClient, QueryClientProvider, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState, type ReactNode } from "react";
import { ActionsProvider } from "./actions/ActionsProvider";
import type { NailClient } from "./api/client";
import { errorMessageIdOf } from "./api/client";
import type { AccountStatus, ResolvedLayout, ThemeMode } from "./api/generated";
import { normalizeLayout, normalizeTheme } from "./api/normalize";
import { createClient, selectTransportKind } from "./api/select";
import { usePluginLayout } from "./features/plugins";
import { remoteStore } from "./features/remote";
import { ReauthPrompt, RemoteLoginPage, checkRemoteGate, type GateState } from "./RemoteGate";
import { appRegistry } from "./features/registry";
import { I18nProvider, useT } from "./i18n/I18nProvider";
import { LayoutContext } from "./layout/LayoutContext";
import { WindowView } from "./layout/LayoutRenderer";
import { WindowLayer } from "./layout/WindowLayer";
import { WindowSpecsContext, WindowTitlesContext } from "./layout/windowSpecs";
import { DialogRegistryContext } from "./layout/dialogRegistry";
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
import { applyEditHistory } from "./state/editHistory";
import { applyUiStateChange } from "./state/uiState";
import { applyPopOuts, applyWindowStateChange } from "./state/windowState";
import { parsePopOutRoute } from "./state/windows";
import { PopOutHost } from "./features/popout";
import { UiStoreContext, createUiStore, useUi, useUiStoreApi, type UiStore } from "./state/uiStore";
import { BootTheme } from "./theme/BootTheme";
import { ThemeProvider } from "./theme/ThemeProvider";

export interface AppProps {
  /** Client to use; by default one is created for the environment. */
  client?: NailClient;
  queryClient?: QueryClient;
  store?: UiStore;
  widgets?: WidgetRegistry;
  /** Window type to render. */
  windowName?: string;
}

const THEME_MODES: readonly ThemeMode[] = ["system", "light", "dark", "high-contrast"];

/** UI overrides from the page URL: `?lang=de`, `?mode=dark`. */
export function overridesFromUrl(search: string): {
  localeOverride: string | null;
  modeOverride: ThemeMode | null;
} {
  const params = new URLSearchParams(search);
  const mode = params.get("mode");
  return {
    localeOverride: params.get("lang"),
    modeOverride: THEME_MODES.find((m) => m === mode) ?? null,
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
  // The remote UI's login gate (web mode only; ADR 0041).
  const [gate, setGate] = useState<GateState>(() =>
    givenClient !== undefined ||
    selectTransportKind({
      search: window.location.search,
      hasTauri: "__TAURI_INTERNALS__" in window,
    }) !== "websocket"
      ? "open"
      : "checking",
  );
  const [queryClient] = useState(() => givenQueryClient ?? createQueryClient());
  const [store] = useState(
    () => givenStore ?? createUiStore(overridesFromUrl(window.location.search)),
  );

  useEffect(() => {
    if (gate !== "checking") return;
    let cancelled = false;
    void checkRemoteGate().then((state) => {
      if (!cancelled) setGate(state);
    });
    return () => {
      cancelled = true;
    };
  }, [gate]);

  // A remote session that ends (logout, revoked, expired) shows the login
  // page again; the next login opens a new connection.
  useEffect(
    () =>
      remoteStore.subscribe((state, previous) => {
        if (state.ended !== null && previous.ended === null) {
          setClient((current) => {
            current?.close();
            return null;
          });
          setGate("login");
        }
      }),
    [],
  );

  useEffect(() => {
    if (client || gate !== "open") return;
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
  }, [client, gate]);

  let content: ReactNode;
  if (gate === "login") {
    // Before a session there is no configuration: the default theme styles it.
    content = (
      <I18nProvider locale={store.getState().localeOverride}>
        <BootTheme modeOverride={store.getState().modeOverride}>
          <div data-part="app" data-state="login">
            <RemoteLoginPage
              onLoggedIn={() => {
                setGate("open");
              }}
            />
          </div>
        </BootTheme>
      </I18nProvider>
    );
  } else if (client) {
    content = (
      <ClientContext.Provider value={client}>
        <Shell widgets={widgets ?? appRegistry.widgets} windowName={windowName ?? "main"} />
      </ClientContext.Provider>
    );
  } else {
    content = (
      <I18nProvider locale={store.getState().localeOverride}>
        <BootTheme modeOverride={store.getState().modeOverride}>
          <AppStatus
            state={failed ? "error" : "loading"}
            messageId={failed ? "error-unavailable" : "app-loading"}
          />
        </BootTheme>
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
    const refreshMail = () => {
      invalidate(
        queryKeys.allFolders,
        queryKeys.allMessages,
        queryKeys.allThreads,
        queryKeys.allThreadDetails,
        queryKeys.allSearches,
      );
    };
    const offEvents = client.subscribe((event) => {
      // Feature slots (src/features/*) handle their own events.
      for (const handler of appRegistry.events) {
        try {
          handler(event, { client, queryClient, store });
        } catch (error) {
          console.error("nail: a feature event handler failed", error);
        }
      }
      switch (event.type) {
        case "ConfigChanged":
          queryClient.setQueryData<AppConfig>(queryKeys.config, (old) => ({
            theme: normalizeTheme(event.theme),
            layout: normalizeLayout(event.layout),
            locale: old?.locale ?? null,
          }));
          ui().setConfigError(null);
          break;
        case "UiStateChanged":
          applyUiStateChange(queryClient, event.key, event.value);
          break;
        // M5c (ADR 0116): remembered window state and desktop pop-outs.
        case "WindowStateChanged":
          applyWindowStateChange(queryClient, event.window, event.state);
          break;
        case "PopOutChanged":
          applyPopOuts(queryClient, event.pop_outs);
          break;
        // M5d (ADR 0131): the editors' undo histories.
        case "EditHistoryChanged":
          applyEditHistory(queryClient, event.history);
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
          ui().setSyncError({
            accountId: event.account_id,
            code: event.code,
            localized: event.localized ?? null,
          });
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
        case "SendProgress":
          ui().setSendProgress(event.outbox_id, {
            sentBytes: event.sent_bytes,
            totalBytes: event.total_bytes,
          });
          break;
        case "SendStatus": {
          if (event.state !== "sending") ui().setSendProgress(event.outbox_id, null);
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
    const first = list?.[0];
    if (!list || first === undefined) return;
    if (accountId === null || !list.some((a) => a.id === accountId)) {
      selectAccount(first.id);
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

/** Background components of the feature slots (render nothing). */
function FeatureEffects() {
  return (
    <>
      {appRegistry.effects.map((Effect, index) => (
        <Effect key={index} />
      ))}
    </>
  );
}

/**
 * The resolved layout with an action for every installed plug-in command
 * (`plugin.<id>.<command>`, ADR 0052), so toolbars, menus and keys
 * reach them.
 */
function PluginLayout({
  layout,
  children,
}: {
  layout: ResolvedLayout;
  children: (layout: ResolvedLayout) => ReactNode;
}) {
  return <>{children(usePluginLayout(layout))}</>;
}

function Shell({ widgets, windowName }: { widgets: WidgetRegistry; windowName: string }) {
  const config = useConfig();
  const localeOverride = useUi((s) => s.localeOverride);
  const modeOverride = useUi((s) => s.modeOverride);
  useCoreEvents();
  useDefaultSelection();
  // A pop-out window of the desktop app shows one window (M5c, `#/window/<id>`).
  const [popOut] = useState(() =>
    typeof window === "undefined" ? null : parsePopOutRoute(window.location.hash),
  );

  const locale = localeOverride ?? config.data?.locale ?? null;
  return (
    <I18nProvider locale={locale}>
      {config.data ? (
        <ThemeProvider theme={config.data.theme} modeOverride={modeOverride}>
          <PluginLayout layout={config.data.layout}>
            {(layout) => (
              <LayoutContext.Provider value={layout}>
                <DialogRegistryContext.Provider value={appRegistry.dialogs}>
                  <WindowSpecsContext.Provider value={appRegistry.windows}>
                    <WindowTitlesContext.Provider value={appRegistry.windowTitles}>
                      <ActionsProvider layout={layout} handlers={appRegistry.actions}>
                        <WidgetRegistryContext.Provider value={widgets}>
                          <main
                            data-part="app"
                            data-layout={layout.name}
                            data-theme={config.data.theme.name}
                            data-density={config.data.theme.density}
                          >
                            {popOut !== null ? (
                              <PopOutHost layout={layout} window={popOut} />
                            ) : (
                              <>
                                <WindowView layout={layout} name={windowName} />
                                <WindowLayer layout={layout} />
                              </>
                            )}
                            {/* Inside the landmark: context menus open from here. */}
                            <FeatureEffects />
                          </main>
                          <ReauthPrompt />
                        </WidgetRegistryContext.Provider>
                      </ActionsProvider>
                    </WindowTitlesContext.Provider>
                  </WindowSpecsContext.Provider>
                </DialogRegistryContext.Provider>
              </LayoutContext.Provider>
            )}
          </PluginLayout>
        </ThemeProvider>
      ) : (
        <BootTheme modeOverride={modeOverride}>
          {config.isError ? (
            <AppStatus state="error" messageId={errorMessageIdOf(config.error)} />
          ) : (
            <AppStatus state="loading" messageId="app-loading" />
          )}
        </BootTheme>
      )}
    </I18nProvider>
  );
}
