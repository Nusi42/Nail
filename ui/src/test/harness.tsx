/**
 * Test harness: a MockTransport-backed client plus all providers a widget
 * needs, so each widget can be rendered on its own.
 */
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, waitFor } from "@testing-library/react";
import type { ReactElement, ReactNode } from "react";
import { ActionsProvider } from "../actions/ActionsProvider";
import { TransportClient } from "../api/client";
import type { LayoutNode, ResolvedLayout, ResolvedTheme } from "../api/generated";
import { MockTransport } from "../api/mock";
import { createMockData } from "../api/mockData";
import { normalizeNode } from "../api/normalize";
import { I18nProvider } from "../i18n/I18nProvider";
import type { Catalogs } from "../i18n/localizer";
import { LayoutContext } from "../layout/LayoutContext";
import { DialogRegistryContext } from "../layout/dialogRegistry";
import { WindowSpecsContext, WindowTitlesContext } from "../layout/windowSpecs";
import { WidgetRegistryContext, type WidgetNode } from "../layout/widgetRegistry";
import { usePluginLayout } from "../features/plugins";
import { appRegistry } from "../features/registry";
import { ClientContext } from "../state/queries";
import { UiStoreContext, createUiStore, type UiState, type UiStore } from "../state/uiStore";
import { ThemeProvider } from "../theme/ThemeProvider";
import { NodeView } from "../layout/LayoutRenderer";
import { trackEnv } from "./liveEnvs";

export interface Env {
  transport: MockTransport;
  client: TransportClient;
  store: UiStore;
  queryClient: QueryClient;
}

export function createEnv(options: { inboxSize?: number; state?: Partial<UiState> } = {}): Env {
  const transport = new MockTransport({ data: createMockData(options.inboxSize ?? 30) });
  const env: Env = {
    transport,
    client: new TransportClient(transport),
    store: createUiStore({ connection: "connected", ...options.state }),
    queryClient: new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: Number.POSITIVE_INFINITY } },
    }),
  };
  // `setup.ts` lets it settle before unmounting the test's DOM.
  trackEnv(env);
  return env;
}

export interface ProvidersProps {
  env: Env;
  theme?: ResolvedTheme;
  layout?: ResolvedLayout;
  locale?: string | null;
  catalogs?: Catalogs;
  children: ReactNode;
}

/** The layout with plug-in command actions, as the app resolves it. */
function PluginLayout({
  layout,
  children,
}: {
  layout: ResolvedLayout;
  children: (layout: ResolvedLayout) => ReactNode;
}) {
  return <>{children(usePluginLayout(layout))}</>;
}

export function Providers({
  env,
  theme,
  layout,
  locale = "en",
  catalogs,
  children,
}: ProvidersProps) {
  const resolvedLayout = layout ?? env.transport.data.layout;
  return (
    <QueryClientProvider client={env.queryClient}>
      <UiStoreContext.Provider value={env.store}>
        <ClientContext.Provider value={env.client}>
          <I18nProvider locale={locale} {...(catalogs ? { catalogs } : {})}>
            <ThemeProvider theme={theme ?? env.transport.data.theme}>
              <PluginLayout layout={resolvedLayout}>
                {(withPlugins) => (
                  <LayoutContext.Provider value={withPlugins}>
                    <DialogRegistryContext.Provider value={appRegistry.dialogs}>
                      <WindowSpecsContext.Provider value={appRegistry.windows}>
                        <WindowTitlesContext.Provider value={appRegistry.windowTitles}>
                          <ActionsProvider layout={withPlugins} handlers={appRegistry.actions}>
                            <WidgetRegistryContext.Provider value={appRegistry.widgets}>
                              {children}
                            </WidgetRegistryContext.Provider>
                          </ActionsProvider>
                        </WindowTitlesContext.Provider>
                      </WindowSpecsContext.Provider>
                    </DialogRegistryContext.Provider>
                  </LayoutContext.Provider>
                )}
              </PluginLayout>
            </ThemeProvider>
          </I18nProvider>
        </ClientContext.Provider>
      </UiStoreContext.Provider>
    </QueryClientProvider>
  );
}

/**
 * Waits until the env's queries and mutations are idle, then lets React
 * commit their last results inside `act` (a test that ends meanwhile would
 * see those updates outside act).
 */
export async function settleQueries(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
    expect(env.queryClient.isMutating()).toBe(0);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** A widget node from a sparse description. */
export function widget(kind: string, extra: Record<string, unknown> = {}): WidgetNode {
  return normalizeNode({ type: "widget", kind, id: kind, ...extra }) as WidgetNode;
}

export function renderWithProviders(
  ui: ReactElement,
  env: Env,
  props: Omit<ProvidersProps, "env" | "children"> = {},
) {
  return render(
    <Providers env={env} {...props}>
      {ui}
    </Providers>,
  );
}

/** Renders one layout node (e.g. a single widget) with all providers. */
export function renderNode(
  node: LayoutNode,
  env: Env,
  props: Omit<ProvidersProps, "env" | "children"> = {},
) {
  return renderWithProviders(<NodeView node={node} path="test" />, env, props);
}

/**
 * jsdom has no layout. Give elements plausible sizes so the virtualizer
 * renders rows: the message list viewport is 600px high, rows 40px.
 */
export function stubElementSizes(): () => void {
  const height = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
  const width = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetWidth");
  Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
    configurable: true,
    get(this: HTMLElement) {
      return this.dataset.part === "message-list" ? 600 : 40;
    },
  });
  Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
    configurable: true,
    get: () => 800,
  });
  return () => {
    if (height) Object.defineProperty(HTMLElement.prototype, "offsetHeight", height);
    if (width) Object.defineProperty(HTMLElement.prototype, "offsetWidth", width);
  };
}

/**
 * Sets the jsdom window width and notifies listeners (inside `act`: a
 * mounted layout re-renders for the new width, also from `afterEach`).
 */
export function setWindowWidth(width: number): void {
  Object.defineProperty(window, "innerWidth", { configurable: true, writable: true, value: width });
  act(() => {
    window.dispatchEvent(new Event("resize"));
  });
}
