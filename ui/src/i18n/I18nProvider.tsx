import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import type { Os } from "../api/generated";
import { shippedCatalogs } from "./catalogs";
import {
  createLocalizer,
  resolveLabel,
  type Catalogs,
  type Localizer,
  type MessageArgs,
} from "./localizer";

const I18nContext = createContext<Localizer | null>(null);

/** Requested locales: explicit override first, then the browser / OS. */
export function requestedLocales(override: string | null | undefined): string[] {
  const browser = typeof navigator === "undefined" ? [] : [...navigator.languages];
  if (browser.length === 0 && typeof navigator !== "undefined" && navigator.language) {
    browser.push(navigator.language);
  }
  return override ? [override, ...browser] : browser;
}

export interface I18nProviderProps {
  /** Locale override (settings / `?lang=`); `null` follows the browser. */
  locale?: string | null;
  /** Catalogs to use; defaults to the shipped `locales/` files. */
  catalogs?: Catalogs;
  /** The core's system, passed to every message as `$os` (ADR 0202); unknown before a session. */
  os?: Os | null;
  children: ReactNode;
}

export function I18nProvider({
  locale = null,
  catalogs = shippedCatalogs,
  os = null,
  children,
}: I18nProviderProps) {
  const localizer = useMemo(
    () => createLocalizer(catalogs, requestedLocales(locale), import.meta.env.DEV, locale, os),
    [catalogs, locale, os],
  );
  useEffect(() => {
    document.documentElement.lang = localizer.locale;
  }, [localizer.locale]);
  return <I18nContext.Provider value={localizer}>{children}</I18nContext.Provider>;
}

function useLocalizer(): Localizer {
  const localizer = useContext(I18nContext);
  if (!localizer) {
    throw new Error("useT() must be used inside <I18nProvider>");
  }
  return localizer;
}

/** Returns the translate function `t(id, args?)`. */
export function useT(): (id: string, args?: MessageArgs) => string {
  const localizer = useLocalizer();
  return localizer.t;
}

/** Returns a resolver for layout labels (`@id` or literal text). */
export function useLabel(): (label: string) => string {
  const localizer = useLocalizer();
  return useMemo(() => (label: string) => resolveLabel(localizer, label), [localizer]);
}

/** The selected locale, for `Intl` formatting. */
export function useLocale(): string {
  return useLocalizer().locale;
}
