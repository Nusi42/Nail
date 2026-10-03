/**
 * Applies a resolved theme: compiles it, writes the single app stylesheet,
 * sets the colour mode on `<html>` and hot-swaps on every new theme object
 * (e.g. after a `ConfigChanged` event). This is the only component that
 * writes CSS.
 */
import {
  createContext,
  useContext,
  useLayoutEffect,
  useMemo,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";
import type { Density, ResolvedTheme, ThemeMode } from "../api/generated";
import { compileTheme, effectiveMode } from "./compile";
import { readerCss } from "./reader";
import { StyleManager, type Declarations } from "./styleManager";

export type EffectiveMode = "light" | "dark" | "high-contrast";

interface ThemeContextValue {
  theme: ResolvedTheme;
  mode: EffectiveMode;
  manager: StyleManager;
  readerCss: string;
}

const ThemeContext = createContext<ThemeContextValue | null>(null);

function mediaQuery(query: string): MediaQueryList | null {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia(query)
    : null;
}

function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    (notify) => {
      const list = mediaQuery(query);
      list?.addEventListener("change", notify);
      return () => list?.removeEventListener("change", notify);
    },
    () => mediaQuery(query)?.matches ?? false,
    () => false,
  );
}

export interface ThemeProviderProps {
  theme: ResolvedTheme;
  /** User override of the theme's mode (`null` = use the theme's). */
  modeOverride?: ThemeMode | null;
  children: ReactNode;
}

export function ThemeProvider({ theme, modeOverride = null, children }: ThemeProviderProps) {
  const [manager] = useState(() => new StyleManager());
  const prefersDark = useMediaQuery("(prefers-color-scheme: dark)");
  const prefersContrast = useMediaQuery("(prefers-contrast: more)");
  const mode = effectiveMode(modeOverride ?? theme.mode, prefersDark, prefersContrast);
  const compiled = useMemo(() => compileTheme(theme), [theme]);

  useLayoutEffect(() => {
    manager.setTheme(compiled.css);
    for (const warning of compiled.warnings) {
      console.warn(`nail: theme rule dropped: ${warning}`);
    }
  }, [manager, compiled]);

  useLayoutEffect(() => () => manager.dispose(), [manager]);

  useLayoutEffect(() => {
    document.documentElement.dataset.nailMode = mode;
  }, [mode]);

  // Density (M5a): the compact token variant and density-dependent rules
  // key on this attribute; the `app` part carries `data-density` too.
  const density: Density = theme.density === "compact" ? "compact" : "comfortable";
  useLayoutEffect(() => {
    document.documentElement.dataset.nailDensity = density;
  }, [density]);

  const value = useMemo(
    () => ({ theme, mode, manager, readerCss: readerCss(theme, mode) }),
    [theme, mode, manager],
  );
  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

function useThemeContext(): ThemeContextValue {
  const value = useContext(ThemeContext);
  if (!value) {
    throw new Error("theme hooks must be used inside <ThemeProvider>");
  }
  return value;
}

/** The resolved theme (icons, message settings, …). */
export function useTheme(): ResolvedTheme {
  return useThemeContext().theme;
}

/** The theme's density (Comfortable / Compact). */
export function useDensity(): Density {
  return useThemeContext().theme.density === "compact" ? "compact" : "comfortable";
}

/** The effective colour mode. */
export function useThemeMode(): EffectiveMode {
  return useThemeContext().mode;
}

/** Stylesheet for the reader iframe document. */
export function useReaderCss(): string {
  return useThemeContext().readerCss;
}

/**
 * Applies data-driven geometry (sizes) to the element carrying
 * `data-geom={id}`. Values go through the ThemeProvider's stylesheet, never
 * through inline styles.
 */
export function useGeometry(id: string, declarations: Declarations): void {
  const { manager } = useThemeContext();
  const key = JSON.stringify(declarations);
  useLayoutEffect(() => () => manager.removeGeometry(id), [manager, id]);
  useLayoutEffect(() => {
    manager.setGeometry(id, JSON.parse(key) as Declarations);
  }, [manager, id, key]);
}

/** Test / inspector access to the style manager. */
export function useStyleManager(): StyleManager {
  return useThemeContext().manager;
}
