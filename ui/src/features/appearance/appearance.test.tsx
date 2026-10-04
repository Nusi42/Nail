/**
 * M5a WP153: the appearance page, theme switching through `ThemeChanged`,
 * Follow Windows from the browser, and the mock's theme resolution.
 */
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { App } from "../../App";
import type { Request, ThemeInfo } from "../../api/generated";
import { defaultTheme } from "../../api/mockData";
import { createEnv, renderNode, settleQueries, stubElementSizes, widget } from "../../test/harness";
import { builtinThemes, setMockSystemAppearance } from "./mock";
import { BUILTIN_THEME_FILES, parseThemeFile, previewColors, resolveMockTheme } from "./themes";
import { overridesOf } from "./AppearanceSettings";
import { must } from "../../test/must";
import { stubProperties } from "../../test/stub";

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
});

/** Requests the mock received (for "what did the UI ask for"). */
function recordRequests(env: ReturnType<typeof createEnv>): Request[] {
  const seen: Request[] = [];
  const send = env.transport.send.bind(env.transport);
  env.transport.send = (request) => {
    seen.push(request);
    return send(request);
  };
  return seen;
}

/** Replaces `window.matchMedia`; returns a function that flips the answers. */
function stubMedia(initial: { dark: boolean; contrast: boolean }) {
  let state = { ...initial };
  const listeners = new Set<() => void>();
  const restore = stubProperties(window, {
    matchMedia: ((query: string) => ({
      get matches() {
        if (query === "(prefers-color-scheme: dark)") return state.dark;
        if (query === "(prefers-contrast: more)") return state.contrast;
        return false;
      },
      media: query,
      addEventListener: (_: string, l: () => void) => listeners.add(l),
      removeEventListener: (_: string, l: () => void) => listeners.delete(l),
    })) as unknown as typeof window.matchMedia,
  });
  return {
    set(next: { dark: boolean; contrast: boolean }) {
      state = { ...next };
      act(() => {
        for (const l of [...listeners]) l();
      });
    },
    restore,
  };
}

describe("theme files for the mock core", () => {
  it("reads the shipped built-in themes", () => {
    expect([...BUILTIN_THEME_FILES.keys()]).toEqual(
      expect.arrayContaining(["frost", "sage", "graphite", "nocturne", "high-contrast"]),
    );
    expect(BUILTIN_THEME_FILES.get("frost")?.mode).toBe("light");
    expect(BUILTIN_THEME_FILES.get("graphite")?.mode).toBe("dark");
    expect(BUILTIN_THEME_FILES.get("high-contrast")?.mode).toBe("high-contrast");
    expect(BUILTIN_THEME_FILES.get("frost")?.tokens.color?.accent).toMatch(/^#[0-9a-f]{6}$/i);
  });

  it("previews a user theme with the palette of the built-in it extends", () => {
    const info = (id: string, extra: Partial<ThemeInfo> = {}): ThemeInfo => ({
      id,
      name: id,
      kind: "light",
      source: "user",
      file: `themes/${id}.kdl`,
      ...extra,
    });
    const frost = BUILTIN_THEME_FILES.get("frost")?.tokens.color;
    const graphite = BUILTIN_THEME_FILES.get("graphite")?.tokens.color;
    const sage = BUILTIN_THEME_FILES.get("sage")?.tokens.color;
    expect(frost?.accent).toBeTruthy();
    const ocean = info("ocean", { extends: "sage" });
    const deep = info("deep", { extends: "ocean" });
    const night = info("night", { kind: "dark", extends: "default" });
    const loop = info("loop", { extends: "loop" });
    const all = [ocean, deep, night, loop];
    // Built-ins preview with their own palette.
    expect(previewColors(info("graphite", { source: "builtin", kind: "dark" }), all)).toBe(
      graphite,
    );
    // Through user themes to the built-in at the end of the chain.
    expect(previewColors(ocean, all)).toBe(sage);
    expect(previewColors(deep, all)).toBe(sage);
    // No built-in in the chain (or a loop): the built-in of its kind.
    expect(previewColors(night, all)).toBe(graphite);
    expect(previewColors(loop, all)).toBe(frost);
    for (const theme of all) {
      expect(Object.keys(previewColors(theme, all))).toEqual(
        expect.arrayContaining(["bg-subtle", "bg", "accent", "fg"]),
      );
    }
  });

  it("previews a user theme with its own colours when the core sends them", () => {
    const sage = BUILTIN_THEME_FILES.get("sage")?.tokens.color ?? {};
    const ocean: ThemeInfo = {
      id: "ocean",
      name: "Ocean",
      kind: "light",
      source: "user",
      extends: "sage",
      file: "themes/ocean.kdl",
      preview: { accent: "#0077aa" },
    };
    const colors = previewColors(ocean, [ocean]);
    expect(colors.accent).toBe("#0077aa");
    expect(colors.bg).toBe(sage.bg);
    // The mock core sends every built-in's own preview colours.
    const frost = builtinThemes().find((t) => t.id === "frost");
    expect(frost?.preview?.accent).toBe(BUILTIN_THEME_FILES.get("frost")?.tokens.color?.accent);
  });

  it("parses token values, variants, references and comments", () => {
    const file = parseThemeFile(
      "demo",
      `// a comment
theme "Demo" extends="default" mode="dark" {
    tokens {
        color "bg" "#101010" print="#ffffff" // trailing comment
        color "flagged" "$color.danger"
        font "ui" #""Segoe UI", sans-serif"#
        font-size "body" 16 compact=15
        border "thin" "2px solid $color.border"
        motion "fast" "0ms"
    }
    part "toolbar.button" { border "$border.thin"; }
}`,
    );
    expect(file).toMatchObject({ id: "demo", name: "Demo", mode: "dark" });
    expect(file.tokens).toEqual({
      color: { bg: "#101010", flagged: "var(--nail-color-danger)" },
      font: { ui: '"Segoe UI", sans-serif' },
      "font-size": { body: "16px" },
      border: { thin: "2px solid var(--nail-color-border)" },
      motion: { fast: "0ms" },
    });
    expect(file.variants).toEqual({
      print: { color: { bg: "#ffffff" } },
      compact: { "font-size": { body: "15px" } },
    });
  });

  it("lays a theme over the base like the core: variants dropped, user tokens kept", () => {
    const base = defaultTheme();
    const user = structuredClone(base);
    must(user.tokens.color).fg = "#123456";
    const frost = BUILTIN_THEME_FILES.get("frost");
    const resolved = resolveMockTheme(
      user,
      "frost",
      { density: "compact", overrides: { font_size: 16, radius: 4, accent: "#aa0000" } },
      base,
    );
    expect(resolved.name).toBe(frost?.name);
    expect(resolved.mode).toBe("light");
    expect(resolved.density).toBe("compact");
    expect(resolved.tokens.color?.bg).toBe(frost?.tokens.color?.bg);
    // The base's dark variant of a replaced token is gone.
    expect(resolved.modes.dark?.color?.bg).toBeUndefined();
    // The user's own token wins over the theme.
    expect(resolved.tokens.color?.fg).toBe("#123456");
    expect(resolved.tokens["font-size"]?.body).toBe("16px");
    expect(resolved.modes.compact?.["font-size"]?.body).toBeUndefined();
    expect(resolved.tokens.radius?.md).toBe("4px");
    expect(resolved.tokens.color?.accent).toBe("#aa0000");
  });

  it("reads override drafts", () => {
    expect(overridesOf({ fontSize: "", radius: "", accent: "" })).toEqual({});
    expect(overridesOf({ fontSize: "15", radius: "8", accent: " #3b5bdb " })).toEqual({
      font_size: 15,
      radius: 8,
      accent: "#3b5bdb",
    });
    expect(overridesOf({ fontSize: "big", radius: "", accent: "" })).toBeNull();
  });
});

describe("appearance-settings widget", () => {
  it("shows the gallery, Follow Windows with its pair, and density", async () => {
    const env = createEnv();
    renderNode(widget("appearance-settings"), env);
    const gallery = await screen.findByRole("radiogroup", { name: "Themes" });
    const cards = within(gallery).getAllByRole("radio");
    expect(cards.map((c) => c.getAttribute("data-theme-id"))).toEqual([
      "frost",
      "sage",
      "graphite",
      "nocturne",
      "high-contrast",
    ]);
    expect(within(gallery).getByRole("radio", { name: /Frost/ })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByRole("switch", { name: "Follow Windows" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByLabelText("Light theme")).toHaveValue("frost");
    expect(screen.getByLabelText("Dark theme")).toHaveValue("graphite");
    // Only themes of the right kind are offered for each half of the pair.
    expect(
      within(screen.getByLabelText("Dark theme"))
        .getAllByRole("option")
        .map((o) => o.getAttribute("value")),
    ).toEqual(["graphite", "nocturne"]);
    expect(screen.getByRole("radio", { name: "Comfortable" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await settleQueries(env);
  });

  it("picks a fixed theme and turns Follow Windows off", async () => {
    const env = createEnv();
    const requests = recordRequests(env);
    renderNode(widget("appearance-settings"), env);
    const nocturne = await screen.findByRole("radio", { name: /Nocturne/ });
    fireEvent.click(nocturne);
    await waitFor(() => expect(nocturne).toHaveAttribute("aria-checked", "true"));
    expect(requests).toContainEqual({ type: "SetTheme", theme_id: "nocturne" });
    expect(screen.getByRole("switch", { name: "Follow Windows" })).toHaveAttribute(
      "aria-checked",
      "false",
    );
    expect(screen.queryByLabelText("Light theme")).toBeNull();
    expect(env.transport.data.features.appearance.settings).toMatchObject({
      theme: "nocturne",
      follow_system: false,
    });
    await settleQueries(env);
  });

  it("changes the pair, density and Follow Windows through SetThemePreferences", async () => {
    const env = createEnv();
    renderNode(widget("appearance-settings"), env);
    fireEvent.change(await screen.findByLabelText("Dark theme"), {
      target: { value: "nocturne" },
    });
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.dark).toBe("nocturne");
    });
    fireEvent.click(screen.getByRole("radio", { name: "Compact" }));
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.density).toBe("compact");
    });
    fireEvent.click(screen.getByRole("switch", { name: "Follow Windows" }));
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.follow_system).toBe(false);
    });
    expect(env.transport.data.features.appearance.settings).toMatchObject({
      light: "frost",
      dark: "nocturne",
      density: "compact",
    });
    await settleQueries(env);
  });

  it("validates and applies the overrides", async () => {
    const env = createEnv();
    renderNode(widget("appearance-settings"), env);
    const size = await screen.findByLabelText("Text size in pixels (11–20)");
    fireEvent.change(size, { target: { value: "40" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Choose a text size between 11 and 20 pixels.",
    );
    fireEvent.change(size, { target: { value: "15" } });
    fireEvent.change(screen.getByLabelText("Accent colour"), { target: { value: "#2e7d5b" } });
    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.overrides).toEqual({
        font_size: 15,
        accent: "#2e7d5b",
      });
    });
    fireEvent.click(screen.getByRole("button", { name: "Use theme defaults" }));
    await waitFor(() => {
      expect(env.transport.data.features.appearance.settings.overrides).toEqual({});
    });
    await settleQueries(env);
  });

  it("explains when the core cannot change themes yet", async () => {
    const env = createEnv();
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) =>
      request.type === "ListThemes"
        ? {
            type: "Error",
            code: "unsupported",
            message: "not ready",
            localized: { id: "error-unsupported" },
          }
        : send(request);
    renderNode(widget("appearance-settings"), env);
    expect(await screen.findByRole("status")).toHaveTextContent("This is not supported yet.");
    await settleQueries(env);
  });
});

describe("theme switching in the app", () => {
  it("restyles the app when the core reports another theme (ThemeChanged)", async () => {
    const env = createEnv({ state: { connection: "connecting" } });
    const { container } = render(
      <App client={env.client} queryClient={env.queryClient} store={env.store} />,
    );
    await screen.findAllByRole("option");
    const app = container.querySelector('[data-part="app"]');
    await waitFor(() => expect(app).toHaveAttribute("data-theme", "Frost"));
    expect(document.documentElement.dataset.nailMode).toBe("light");
    act(() => {
      setMockSystemAppearance(env.transport, { scheme: "dark", high_contrast: false });
    });
    await waitFor(() => expect(app).toHaveAttribute("data-theme", "Graphite"));
    expect(document.documentElement.dataset.nailMode).toBe("dark");
    act(() => {
      setMockSystemAppearance(env.transport, { scheme: "dark", high_contrast: true });
    });
    await waitFor(() => {
      expect(document.documentElement.dataset.nailMode).toBe("high-contrast");
    });
    await settleQueries(env);
  });

  it("applies density to the root and the app part", async () => {
    const env = createEnv({ state: { connection: "connecting" } });
    const { container } = render(
      <App client={env.client} queryClient={env.queryClient} store={env.store} />,
    );
    await screen.findAllByRole("option");
    const app = container.querySelector('[data-part="app"]');
    expect(app).toHaveAttribute("data-density", "comfortable");
    await act(async () => {
      await env.client.request(
        {
          type: "SetThemePreferences",
          preferences: {
            follow_system: true,
            light: "frost",
            dark: "graphite",
            density: "compact",
            overrides: {},
          },
        },
        "Themes",
      );
    });
    await waitFor(() => expect(app).toHaveAttribute("data-density", "compact"));
    expect(document.documentElement.dataset.nailDensity).toBe("compact");
    await settleQueries(env);
  });
});

describe("Follow Windows from the browser", () => {
  it("reports the page's colour scheme and contrast while Follow Windows is on", async () => {
    const media = stubMedia({ dark: false, contrast: false });
    try {
      const env = createEnv({ state: { connection: "connecting" } });
      const requests = recordRequests(env);
      const { container } = render(
        <App client={env.client} queryClient={env.queryClient} store={env.store} />,
      );
      await screen.findAllByRole("option");
      const app = container.querySelector('[data-part="app"]');
      // The mock core already thinks "light": nothing to report.
      expect(requests.some((r) => (r.type as string) === "ReportSystemAppearance")).toBe(false);
      media.set({ dark: true, contrast: false });
      await waitFor(() => expect(app).toHaveAttribute("data-theme", "Graphite"));
      expect(requests).toContainEqual({
        type: "ReportSystemAppearance",
        system: { scheme: "dark", high_contrast: false },
      });
      media.set({ dark: true, contrast: true });
      await waitFor(() => {
        expect(env.transport.data.features.appearance.system).toEqual({
          scheme: "dark",
          high_contrast: true,
        });
      });
      await settleQueries(env);
    } finally {
      media.restore();
    }
  });

  it("does not report while a fixed theme is chosen, and stops for a core without the request", async () => {
    const media = stubMedia({ dark: true, contrast: false });
    try {
      const env = createEnv({ state: { connection: "connecting" } });
      env.transport.data.features.appearance.settings.follow_system = false;
      const requests = recordRequests(env);
      render(<App client={env.client} queryClient={env.queryClient} store={env.store} />);
      await screen.findAllByRole("option");
      await settleQueries(env);
      expect(requests.some((r) => (r.type as string) === "ReportSystemAppearance")).toBe(false);

      const env2 = createEnv({ state: { connection: "connecting" } });
      const seen: string[] = [];
      const send = env2.transport.send.bind(env2.transport);
      env2.transport.send = async (request) => {
        seen.push(request.type);
        return (request.type as string) === "ReportSystemAppearance"
          ? { type: "Error", code: "invalid_request", message: "unknown variant" }
          : send(request);
      };
      render(<App client={env2.client} queryClient={env2.queryClient} store={env2.store} />);
      await waitFor(() => {
        expect(seen).toContain("ListThemes");
      });
      await settleQueries(env2);
      // The core knows the OS state: nothing is pushed back at start.
      expect(seen).not.toContain("ReportSystemAppearance");
      media.set({ dark: false, contrast: false });
      await waitFor(() => {
        expect(seen).toContain("ReportSystemAppearance");
      });
      media.set({ dark: true, contrast: false });
      await settleQueries(env2);
      expect(seen.filter((t) => t === "ReportSystemAppearance")).toHaveLength(1);
    } finally {
      media.restore();
    }
  });
});
