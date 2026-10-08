/**
 * Follow Windows from the browser / WebView (M5a WP153): while Follow
 * Windows is on, the colour scheme and contrast the page sees
 * (`prefers-color-scheme`, `prefers-contrast: more` / `forced-colors:
 * active`) are reported to the core whenever they differ from what the core
 * knows, so it can switch between the light and the dark theme where it
 * cannot read the OS itself (a remote browser). The core decides; this only
 * reports. A core without the request is asked once.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useSyncExternalStore } from "react";
import type { SystemAppearance } from "../../api/generated";
import { useClient } from "../../state/queries";
import { applyAppearance, useAppearance } from "./queries";
import { UNSUPPORTED_CODES } from "../../api/contract";

const QUERIES = {
  dark: "(prefers-color-scheme: dark)",
  contrast: "(prefers-contrast: more)",
  forced: "(forced-colors: active)",
} as const;

function media(query: string): MediaQueryList | null {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(query)
    : null;
}

function subscribe(notify: () => void): () => void {
  const lists = Object.values(QUERIES).map(media);
  for (const list of lists) list?.addEventListener("change", notify);
  return () => {
    for (const list of lists) list?.removeEventListener("change", notify);
  };
}

/** The page's view of the OS appearance, as a stable string key. */
function snapshot(): string {
  const dark = media(QUERIES.dark)?.matches ?? false;
  const contrast =
    (media(QUERIES.contrast)?.matches ?? false) || (media(QUERIES.forced)?.matches ?? false);
  return `${dark ? "dark" : "light"}:${contrast ? "hc" : ""}`;
}

/** The browser's / WebView's colour scheme and contrast. */
export function useBrowserAppearance(): SystemAppearance {
  const key = useSyncExternalStore(subscribe, snapshot, () => "light:");
  const [scheme, hc] = key.split(":");
  return { scheme: scheme === "dark" ? "dark" : "light", high_contrast: hc === "hc" };
}

/**
 * Background effect: while Follow Windows is on, reports when the page's
 * colour scheme or contrast changes, and once at start when the core does
 * not know the OS state. It never pushes back a state the core got from the
 * OS itself (the desktop's own watcher wins until the page sees a change).
 */
export function SystemAppearanceReporter() {
  const client = useClient();
  const queryClient = useQueryClient();
  const appearance = useAppearance();
  const { scheme, high_contrast: highContrast } = useBrowserAppearance();
  const unsupported = useRef(false);
  const lastSeen = useRef<string | null>(null);
  const follow = appearance.data?.settings.follow_system ?? false;
  const loaded = appearance.data !== undefined;
  const coreKnows = appearance.data?.system !== undefined;
  const available = media(QUERIES.dark) !== null;
  useEffect(() => {
    if (!loaded || !available) return;
    const key = `${scheme}:${String(highContrast)}`;
    const changed = lastSeen.current !== null && lastSeen.current !== key;
    const first = lastSeen.current === null;
    lastSeen.current = key;
    if (!follow || unsupported.current) return;
    if (!changed && !(first && !coreKnows)) return;
    const system: SystemAppearance = { scheme, high_contrast: highContrast };
    client
      .call({ type: "ReportSystemAppearance", system })
      .then((response) => {
        // M5b: a remote session follows its own browser; its new theme
        // comes with the answer (the host's `ThemeChanged` is not its own).
        if (response.type === "Themes") {
          applyAppearance(queryClient, response.appearance);
          return;
        }
        // A core older than M5a does not know the request: ask only once.
        if (response.type === "Error" && UNSUPPORTED_CODES.has(response.code)) {
          unsupported.current = true;
        } else if (response.type === "Error") {
          console.warn("nail: cannot report the system appearance", response.message);
        }
      })
      .catch((error: unknown) => {
        console.warn("nail: cannot report the system appearance", error);
      });
  }, [client, queryClient, loaded, available, follow, coreKnows, scheme, highContrast]);
  return null;
}
