/**
 * M5d WP183: the mock core's theme editor requests answer like the core —
 * details with origins, duplicate / rename / delete, token and part edits
 * that restyle the app when the theme is in use, `base_revision`
 * conflicts, and one undo entry per write.
 */
import { describe, expect, it } from "vitest";
import type { Event as NailEvent, Response, ThemeDetails } from "../../api/generated";
import { mockEditHistory, simulateExternalEdit } from "../../api/editHistoryMock";
import { createEnv } from "../../test/harness";
import { must } from "../../test/must";
import { themeIdFromName } from "./model";
import { applyThemePatches, selectorText } from "./themeEditMock";
import { parseSelector } from "./themes";

function details(answer: Response): ThemeDetails {
  if (answer.type !== "ThemeDetails") throw new Error(`unexpected ${answer.type}`);
  return answer.details;
}

function savedId(answer: Response): string {
  if (answer.type !== "ThemeSaved") throw new Error(`unexpected ${answer.type}`);
  return answer.theme_id;
}

describe("theme ids and selectors", () => {
  it("derives ids from names like the core", () => {
    expect(themeIdFromName("My Ocean!")).toBe("my-ocean");
    expect(themeIdFromName("  Crème brûlée ")).toBe("creme-brulee");
    expect(themeIdFromName("???")).toBe("theme");
    expect(themeIdFromName("2024")).toBe("theme-2024");
    expect(themeIdFromName("a".repeat(200))).toHaveLength(60);
  });

  it("parses and writes part selectors", () => {
    const parsed = parseSelector('split[orientation="horizontal"] split.divider:hover');
    expect(parsed).toEqual({
      selector: "split.divider",
      states: ["hover"],
      conditions: [],
      ancestors: [
        {
          selector: "split",
          states: [],
          conditions: [{ key: "orientation", value: "horizontal" }],
        },
      ],
    });
    expect(selectorText(parsed)).toBe('split[orientation="horizontal"] split.divider:hover');
  });

  it("applies patches and refuses invalid ones", () => {
    const start = { tokens: [], parts: [] };
    const next = applyThemePatches(start, [
      { op: "set-token", category: "color", name: "accent", value: "#ff0066", mode: null },
      { op: "set-token", category: "color", name: "accent", value: "#00ff00", mode: "dark" },
      { op: "set-part-property", selector: "toolbar", property: "gap", value: "4px" },
      { op: "set-part-property", selector: "app", property: "color", value: "red" },
      { op: "reset-part", selector: "app" },
    ]);
    expect(next).toEqual({
      tokens: [{ category: "color", name: "accent", value: "#ff0066", modes: { dark: "#00ff00" } }],
      parts: [{ selector: "toolbar", properties: { gap: "4px" } }],
    });
    if (typeof next === "string") throw new Error(next);
    // Removing the last value removes the token; the last property removes the rule.
    expect(
      applyThemePatches(next, [
        { op: "set-token", category: "color", name: "accent", value: null, mode: null },
        { op: "set-token", category: "color", name: "accent", value: null, mode: "dark" },
        { op: "set-part-property", selector: "toolbar", property: "gap", value: null },
      ]),
    ).toEqual({ tokens: [], parts: [] });
    expect(
      applyThemePatches(start, [
        { op: "set-token", category: "color", name: "x", value: "red; }", mode: null },
      ]),
    ).toBe("invalid-token");
    expect(
      applyThemePatches(start, [
        { op: "set-token", category: "color", name: "x", value: "red", mode: "sepia" },
      ]),
    ).toBe("invalid-token");
    expect(
      applyThemePatches(start, [
        { op: "set-part-property", selector: "Bad Part{", property: "color", value: "red" },
      ]),
    ).toBe("invalid-part");
    expect(
      applyThemePatches(start, [
        { op: "set-part-property", selector: "app", property: "color", value: "url(x)" },
      ]),
    ).toBe("invalid-part");
  });
});

describe("theme editor requests in the mock core", () => {
  it("lists a built-in theme's tokens and rules with their origins, read-only", async () => {
    const env = createEnv();
    const d = details(await env.client.call({ type: "GetThemeDetails", theme_id: "sage" }));
    expect(d.editable).toBe(false);
    expect(d.info.id).toBe("sage");
    expect(d.tokens.find((t) => t.category === "color" && t.name === "accent")).toMatchObject({
      origin: "own",
    });
    expect(d.tokens.find((t) => t.category === "space" && t.name === "md")).toMatchObject({
      origin: "base",
    });
    expect(d.parts.length).toBeGreaterThan(0);
    expect(d.parts.every((p) => p.origin === "base")).toBe(true);
    // Built-ins cannot be edited, renamed or deleted.
    expect(
      await env.client.call({
        type: "EditTheme",
        target: { kind: "theme", theme_id: "sage" },
        patches: [],
      }),
    ).toMatchObject({ code: "invalid_request", localized: { id: "edit-issue-not-editable" } });
    expect(
      await env.client.call({ type: "RenameTheme", theme_id: "sage", name: "Mine" }),
    ).toMatchObject({ localized: { id: "edit-issue-not-editable" } });
  });

  it("duplicates themes under unique ids and checks the name", async () => {
    const env = createEnv();
    const events: NailEvent[] = [];
    env.transport.onEvent((e) => events.push(e));
    const first = savedId(
      await env.client.call({ type: "DuplicateTheme", theme_id: "sage", name: "Ocean" }),
    );
    const second = savedId(
      await env.client.call({ type: "DuplicateTheme", theme_id: first, name: "Ocean" }),
    );
    expect([first, second]).toEqual(["ocean", "ocean-2"]);
    const state = env.transport.data.features.appearance;
    expect(state.themes.find((t) => t.id === "ocean")).toMatchObject({
      name: "Ocean",
      kind: "light",
      source: "user",
      extends: "sage",
      file: "themes/ocean.kdl",
    });
    expect(state.themes.find((t) => t.id === "ocean-2")).toMatchObject({ extends: "sage" });
    expect(events.map((e) => e.type)).toContain("ThemeChanged");
    expect(mockEditHistory(env.transport, "appearance").undo.map((e) => e.kind)).toEqual([
      "duplicate-theme",
      "duplicate-theme",
    ]);
    const ocean = details(await env.client.call({ type: "GetThemeDetails", theme_id: "ocean" }));
    expect(ocean.editable).toBe(true);
    // The built-in it copies is inherited now.
    expect(ocean.tokens.find((t) => t.name === "accent")).toMatchObject({ origin: "extended" });
    for (const name of ["", " ", "x".repeat(65)]) {
      expect(
        await env.client.call({ type: "DuplicateTheme", theme_id: "sage", name }),
      ).toMatchObject({ localized: { id: "edit-issue-invalid-theme-name" } });
    }
  });

  it("edits tokens and parts of a user theme, restyling the app while it is in use", async () => {
    const env = createEnv();
    const id = savedId(
      await env.client.call({ type: "DuplicateTheme", theme_id: "frost", name: "Mine" }),
    );
    await env.client.request({ type: "SetTheme", theme_id: id }, "Themes");
    const events: NailEvent[] = [];
    env.transport.onEvent((e) => events.push(e));
    const before = details(await env.client.call({ type: "GetThemeDetails", theme_id: id }));
    const answer = await env.client.call({
      type: "EditTheme",
      target: { kind: "theme", theme_id: id },
      patches: [
        { op: "set-token", category: "color", name: "accent", value: "#ff0066", mode: null },
        { op: "set-token", category: "radius", name: "md", value: "3", mode: null },
        {
          op: "set-part-property",
          selector: "toolbar.button:hover",
          property: "background-color",
          value: "$color.accent",
        },
      ],
      base_revision: before.revision,
    });
    expect(answer).toMatchObject({ type: "Themes", appearance: { active: id } });
    const config = events.find((e) => e.type === "ConfigChanged");
    if (config?.type !== "ConfigChanged") throw new Error("no ConfigChanged");
    expect(config.theme.tokens.color?.accent).toBe("#ff0066");
    expect(config.theme.tokens.radius?.md).toBe("3px");
    const rule = must(config.theme.parts.find((p) => p.selector === "toolbar.button"));
    expect(rule).toMatchObject({
      states: ["hover"],
      declarations: { "background-color": "var(--nail-color-accent)" },
      source: { file: `themes/${id}.kdl` },
    });
    // The gallery previews the theme with its own accent.
    expect(
      env.transport.data.features.appearance.themes.find((t) => t.id === id)?.preview?.accent,
    ).toBe("#ff0066");
    const after = details(await env.client.call({ type: "GetThemeDetails", theme_id: id }));
    expect(after.tokens.find((t) => t.name === "accent" && t.category === "color")).toMatchObject({
      value: "#ff0066",
      origin: "own",
    });
    expect(after.parts.at(-1)).toEqual({
      selector: "toolbar.button:hover",
      properties: { "background-color": "$color.accent" },
      origin: "own",
    });
    expect(after.revision).not.toBe(before.revision);
    expect(mockEditHistory(env.transport, "appearance").undo.at(-1)?.kind).toBe("edit-theme");

    // Undo restores the file (and the app's look).
    await env.client.request({ type: "UndoEdit", scope: "appearance" }, "EditHistory");
    const undone = details(await env.client.call({ type: "GetThemeDetails", theme_id: id }));
    expect(undone.tokens.find((t) => t.name === "accent" && t.category === "color")?.origin).toBe(
      "extended",
    );
  });

  it("refuses edits on a file that changed on disk, and invalid values", async () => {
    const env = createEnv();
    const id = savedId(
      await env.client.call({ type: "DuplicateTheme", theme_id: "graphite", name: "Night" }),
    );
    const d = details(await env.client.call({ type: "GetThemeDetails", theme_id: id }));
    simulateExternalEdit(env.transport, "appearance");
    expect(
      await env.client.call({
        type: "EditTheme",
        target: { kind: "theme", theme_id: id },
        patches: [{ op: "set-token", category: "color", name: "bg", value: "#000", mode: null }],
        base_revision: d.revision,
      }),
    ).toMatchObject({ code: "conflict", localized: { id: "edit-issue-conflict" } });
    expect(
      await env.client.call({
        type: "EditTheme",
        target: { kind: "theme", theme_id: id },
        patches: [{ op: "set-token", category: "Color", name: "bg", value: "#000", mode: null }],
      }),
    ).toMatchObject({ code: "invalid_request", localized: { id: "edit-issue-invalid-token" } });
  });

  it("edits the user's own theme.kdl on top of the active theme", async () => {
    const env = createEnv();
    const events: NailEvent[] = [];
    env.transport.onEvent((e) => events.push(e));
    await env.client.request(
      {
        type: "EditTheme",
        target: { kind: "user" },
        patches: [
          { op: "set-token", category: "color", name: "accent", value: "#123456", mode: null },
        ],
      },
      "Themes",
    );
    const config = events.find((e) => e.type === "ConfigChanged");
    expect(config).toMatchObject({ theme: { tokens: { color: { accent: "#123456" } } } });
  });

  it("renames a theme, moving its file and the selection along", async () => {
    const env = createEnv();
    const id = savedId(
      await env.client.call({ type: "DuplicateTheme", theme_id: "sage", name: "Moss" }),
    );
    savedId(await env.client.call({ type: "DuplicateTheme", theme_id: id, name: "Fern" }));
    await env.client.request({ type: "SetTheme", theme_id: id }, "Themes");
    expect(
      savedId(await env.client.call({ type: "RenameTheme", theme_id: id, name: "Forest" })),
    ).toBe(id);
    const state = env.transport.data.features.appearance;
    expect(state.themes.find((t) => t.id === id)?.name).toBe("Forest");
    expect(
      savedId(
        await env.client.call({
          type: "RenameTheme",
          theme_id: id,
          name: "Forest",
          new_id: "forest",
        }),
      ),
    ).toBe("forest");
    expect(state.themes.map((t) => t.id)).toContain("forest");
    expect(state.themes.map((t) => t.id)).not.toContain(id);
    expect(state.settings.theme).toBe("forest");
    // A copy of a user theme is a copy of its file (same base).
    expect(state.files.fern?.extends).toBe("sage");
    expect(
      await env.client.call({ type: "RenameTheme", theme_id: "forest", name: "X", new_id: "fern" }),
    ).toMatchObject({ localized: { id: "edit-issue-theme-exists" } });
    expect(
      await env.client.call({
        type: "RenameTheme",
        theme_id: "forest",
        name: "X",
        new_id: "Bad Id",
      }),
    ).toMatchObject({ localized: { id: "edit-issue-invalid-theme-name" } });
  });

  it("deletes a user theme; a selection naming it falls back to the default of its kind", async () => {
    const env = createEnv();
    const id = savedId(
      await env.client.call({ type: "DuplicateTheme", theme_id: "nocturne", name: "Deep" }),
    );
    await env.client.request(
      {
        type: "SetThemePreferences",
        preferences: {
          follow_system: true,
          light: "frost",
          dark: id,
          density: "comfortable",
          overrides: {},
        },
      },
      "Themes",
    );
    await env.client.request({ type: "SetTheme", theme_id: id }, "Themes");
    const answer = await env.client.request({ type: "DeleteTheme", theme_id: id }, "Themes");
    expect(answer.appearance.themes.map((t) => t.id)).not.toContain(id);
    expect(answer.appearance.settings).toMatchObject({ theme: "graphite", dark: "graphite" });
    expect(answer.appearance.active).toBe("graphite");
    expect(mockEditHistory(env.transport, "appearance").undo.at(-1)?.kind).toBe("delete-theme");
    // Undo brings the file back.
    await env.client.request({ type: "UndoEdit", scope: "appearance" }, "EditHistory");
    const listed = await env.client.request({ type: "ListThemes" }, "Themes");
    expect(listed.appearance.themes.map((t) => t.id)).toContain(id);
    expect(listed.appearance.settings.theme).toBe(id);
  });
});
