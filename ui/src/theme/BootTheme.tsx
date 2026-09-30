/**
 * The theme of the screens shown before the core's configuration is there:
 * the remote UI's login page (it runs before there is a session, so it
 * cannot ask for `GetConfig`), "Loading…" and "Nail is not available".
 *
 * They use the shipped default theme, bundled at build time (the
 * `nail-api` reference fixture, which a `nail-config` test keeps equal to
 * `defaults/theme.kdl`) and compiled like any theme. It is a separate chunk
 * loaded right away; until it is there (a few milliseconds) the screen
 * stays empty rather than flashing unstyled content.
 */
import { useEffect, useState, type ReactNode } from "react";
import type { ResolvedTheme, ThemeMode } from "../api/generated";
import { normalizeTheme } from "../api/normalize";
import { ThemeProvider } from "./ThemeProvider";

let loaded: ResolvedTheme | null = null;
let loading: Promise<ResolvedTheme> | null = null;

/** Loads (once) and normalizes the bundled default theme. */
export function loadBootTheme(): Promise<ResolvedTheme> {
  loading ??= import("../../../crates/nail-api/fixtures/default-theme.json").then((m) => {
    loaded = normalizeTheme(m.default);
    return loaded;
  });
  return loading;
}

export interface BootThemeProps {
  /** The user's mode override (`?mode=dark`), if any. */
  modeOverride?: ThemeMode | null;
  children: ReactNode;
}

export function BootTheme({ modeOverride = null, children }: BootThemeProps) {
  const [theme, setTheme] = useState<ResolvedTheme | null | "failed">(loaded);
  useEffect(() => {
    if (theme !== null) return;
    let live = true;
    loadBootTheme().then(
      (t) => {
        if (live) setTheme(t);
      },
      (error: unknown) => {
        console.error("nail: cannot load the default theme", error);
        if (live) setTheme("failed");
      },
    );
    return () => {
      live = false;
    };
  }, [theme]);
  if (theme === null) return null;
  // Without a theme the page still works, just unstyled.
  if (theme === "failed") return <>{children}</>;
  return (
    <ThemeProvider theme={theme} modeOverride={modeOverride}>
      {children}
    </ThemeProvider>
  );
}
