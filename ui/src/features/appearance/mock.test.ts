import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import type { Appearance, Event as NailEvent } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { features } from "../registry";
import type { FeatureEventContext } from "../types";
import { appearanceKeys } from ".";
import {
  activeTheme,
  builtinThemes,
  defaultSettings,
  isAppearanceRequest,
  isHexColor,
  overrideIssues,
} from "./mock";

describe("appearance slot (M5a contract)", () => {
  it("is registered", () => {
    expect(features.appearance).toBeDefined();
    expect(isAppearanceRequest({ type: "ListThemes" })).toBe(true);
    expect(isAppearanceRequest({ type: "Ping" })).toBe(false);
  });

  it("picks the active theme like the core", () => {
    const themes = builtinThemes();
    const s = defaultSettings();
    expect(activeTheme(s, { scheme: "light", high_contrast: false }, themes)).toBe("frost");
    expect(activeTheme(s, { scheme: "dark", high_contrast: false }, themes)).toBe("graphite");
    expect(activeTheme(s, { scheme: "dark", high_contrast: true }, themes)).toBe("high-contrast");
    expect(activeTheme(s, undefined, themes)).toBe("frost");
    expect(activeTheme({ ...s, follow_system: false, theme: "nocturne" }, undefined, themes)).toBe(
      "nocturne",
    );
    expect(activeTheme({ ...s, follow_system: false, theme: "gone" }, undefined, themes)).toBe(
      "frost",
    );
  });

  it("validates overrides like the core", () => {
    expect(overrideIssues({})).toEqual([]);
    expect(overrideIssues({ font_size: 15, radius: 0, accent: "#3B5bdb" })).toEqual([]);
    expect(overrideIssues({ font_size: 30, radius: 21, accent: "blue" })).toEqual([
      "font-size-range",
      "radius-range",
      "accent-invalid",
    ]);
    expect(isHexColor("#abc")).toBe(true);
    expect(isHexColor("#abcd")).toBe(false);
  });

  it("answers through the mock transport and reports changes", async () => {
    const t = new MockTransport();
    const events: NailEvent[] = [];
    t.onEvent((e) => events.push(e));
    const listed = await t.send({ type: "ListThemes" });
    expect(listed).toMatchObject({ type: "Themes", appearance: { active: "frost" } });
    if (listed.type !== "Themes") throw new Error("unexpected answer");
    expect(listed.appearance.themes.map((th) => th.id)).toEqual([
      "frost",
      "sage",
      "graphite",
      "nocturne",
      "high-contrast",
    ]);

    expect(await t.send({ type: "SetTheme", theme_id: "nocturne" })).toMatchObject({
      type: "Themes",
      appearance: { active: "nocturne", settings: { follow_system: false } },
    });
    // ThemeChanged, then ConfigChanged with the newly resolved theme (like the core).
    expect(events.map((e) => e.type).slice(-2)).toEqual(["ThemeChanged", "ConfigChanged"]);
    expect(events.at(-1)).toMatchObject({ type: "ConfigChanged", theme: { mode: "dark" } });
    expect(await t.send({ type: "SetTheme", theme_id: "nope" })).toMatchObject({
      type: "Error",
      code: "invalid_request",
    });
    expect(
      await t.send({
        type: "SetThemePreferences",
        preferences: {
          follow_system: true,
          light: "sage",
          dark: "graphite",
          density: "compact",
          overrides: { radius: 99 },
        },
      }),
    ).toMatchObject({ type: "Error", localized: { id: "theme-overrides-issue-radius-range" } });
    expect(
      await t.send({
        type: "SetThemePreferences",
        preferences: {
          follow_system: true,
          light: "sage",
          dark: "graphite",
          density: "compact",
          overrides: { accent: "#2e7d5b" },
        },
      }),
    ).toMatchObject({
      type: "Themes",
      appearance: { active: "sage", settings: { density: "compact" } },
    });
  });

  it("keeps the query cache current on ThemeChanged", () => {
    const queryClient = new QueryClient();
    const appearance = { active: "sage" } as Appearance;
    const ctx = { queryClient } as unknown as FeatureEventContext;
    for (const handler of features.appearance?.events ?? []) {
      handler({ type: "ThemeChanged", appearance }, ctx);
    }
    expect(queryClient.getQueryData(appearanceKeys.themes)).toBe(appearance);
  });
});
