/**
 * The theme of the screens shown before the core's configuration is there:
 * the remote UI's login page (it runs before there is a session, so it
 * cannot ask for `GetConfig`), "Loading…" and "Nail is not available".
 *
 * They use the shipped default theme, bundled at build time (the
 * `nail-api` reference fixture, which a `nail-config` test keeps equal to
 * `defaults/theme.kdl`) and compiled like any theme. It is imported
 * statically, so the first screen is styled at once; the build puts it in
 * its own `default-theme` chunk (vite.config.ts) that loads in parallel
 * with the app and stays cached across app updates.
 */
import type { ReactNode } from "react";
import defaultThemeJson from "../../../crates/nail-api/fixtures/default-theme.json";
import type { ResolvedTheme, ThemeMode } from "../api/generated";
import { normalizeTheme } from "../api/normalize";
import { ThemeProvider } from "./ThemeProvider";

let normalized: ResolvedTheme | null = null;

/** The bundled default theme, normalized once. */
export function bootTheme(): ResolvedTheme {
  normalized ??= normalizeTheme(defaultThemeJson);
  return normalized;
}

export interface BootThemeProps {
  /** The user's mode override (`?mode=dark`), if any. */
  modeOverride?: ThemeMode | null;
  children: ReactNode;
}

export function BootTheme({ modeOverride = null, children }: BootThemeProps) {
  return (
    <ThemeProvider theme={bootTheme()} modeOverride={modeOverride}>
      {children}
    </ThemeProvider>
  );
}
