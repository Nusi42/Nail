// node --test e2e/visual/lib/*.test.ts — the visual matrix: data-driven
// from ListThemes, skips with reasons, filters.
import assert from "node:assert/strict";
import { test } from "node:test";
import {
  DENSITIES,
  EXPECTED_THEMES,
  parseFilter,
  planVariants,
  preferencesFor,
  probeFromAnswer,
  unavailableReason,
  variantId,
  type AppearanceLike,
  type ThemeInfoLike,
  type ThemeProbe,
} from "./matrix.ts";

function appearance(themes: ThemeInfoLike[]): AppearanceLike {
  return {
    themes,
    settings: {
      theme: "frost",
      follow_system: true,
      light: "frost",
      dark: "graphite",
      density: "comfortable",
      overrides: { font_size: 15 },
    },
    active: "frost",
  };
}

const builtin = (id: string, kind = "light"): ThemeInfoLike => ({ id, kind, source: "builtin" });
const allBuiltins = (): ThemeInfoLike[] =>
  EXPECTED_THEMES.map((id) =>
    builtin(id, id === "graphite" || id === "nocturne" ? "dark" : "light"),
  );

test("the core's not-ready answer skips every expected variant with the reason", () => {
  const probe = probeFromAnswer({
    type: "Error",
    code: "unsupported",
    message: "theme selection arrives with M5a WP152",
    localized: { id: "m5a-not-ready" },
  });
  assert.equal(probe.ok, false);
  const variants = planVariants(probe);
  assert.equal(variants.length, EXPECTED_THEMES.length * DENSITIES.length);
  for (const v of variants) {
    assert.match(v.skip ?? "", /ListThemes is not available \(unsupported \/ m5a-not-ready: /);
    assert.match(v.skip ?? "", new RegExp(`theme "${v.theme}"`));
  }
  assert.deepEqual(
    variants.slice(0, 2).map((v) => v.id),
    ["frost-comfortable", "frost-compact"],
  );
});

test("every listed built-in runs in both densities, in gallery order", () => {
  const probe = probeFromAnswer({ type: "Themes", appearance: appearance(allBuiltins()) });
  const variants = planVariants(probe);
  assert.deepEqual(
    variants.map((v) => v.id),
    EXPECTED_THEMES.flatMap((t) => [`${t}-comfortable`, `${t}-compact`]),
  );
  assert.ok(variants.every((v) => v.skip === undefined));
});

test("missing and broken themes are skipped with a clear message; extra built-ins are added", () => {
  const themes = allBuiltins().filter((t) => t.id !== "sage");
  themes.find((t) => t.id === "nocturne")!.problem = { id: "theme-problem-invalid" };
  themes.push(builtin("ocean"), { id: "mine", kind: "dark", source: "user" });
  const probe: ThemeProbe = { ok: true, appearance: appearance(themes) };
  const byId = new Map(planVariants(probe).map((v) => [v.id, v]));
  assert.match(
    byId.get("sage-compact")!.skip!,
    /theme "sage" is not listed by ListThemes \(listed: frost, graphite/,
  );
  assert.match(byId.get("nocturne-comfortable")!.skip!, /problem theme-problem-invalid/);
  assert.equal(byId.get("frost-compact")!.skip, undefined);
  // A further built-in is captured too (it then needs baselines); user themes never.
  assert.equal(byId.get("ocean-comfortable")!.skip, undefined);
  assert.equal(byId.has("mine-comfortable"), false);
  assert.equal([...byId.keys()].at(-1), "ocean-compact");
});

test("filters narrow themes and densities; unknown densities are refused", () => {
  const probe: ThemeProbe = { ok: true, appearance: appearance(allBuiltins()) };
  const variants = planVariants(probe, {
    themes: parseFilter(" graphite, high-contrast "),
    densities: parseFilter("compact"),
  });
  assert.deepEqual(
    variants.map((v) => v.id),
    ["graphite-compact", "high-contrast-compact"],
  );
  assert.throws(() => planVariants(probe, { densities: ["cozy"] }), /unknown density cozy/);
  assert.equal(parseFilter(undefined), null);
  assert.equal(parseFilter(" , "), null);
});

test("unexpected answers and failures become reasons", () => {
  assert.deepEqual(probeFromAnswer({ type: "Done" }), {
    ok: false,
    reason: "unexpected answer Done",
  });
  const failed: ThemeProbe = { ok: false, reason: "nail serve did not start" };
  assert.match(unavailableReason(failed, "frost")!, /nail serve did not start/);
  assert.equal(
    unavailableReason({ ok: true, appearance: appearance([builtin("frost")]) }, "frost"),
    null,
  );
});

test("preferences select the density with Follow Windows off and no overrides", () => {
  const prefs = preferencesFor(appearance(allBuiltins()), "compact");
  assert.deepEqual(prefs, {
    follow_system: false,
    light: "frost",
    dark: "graphite",
    density: "compact",
    overrides: {},
  });
  assert.equal(variantId("high-contrast", "compact"), "high-contrast-compact");
});

test("the current theme runs only on request, first, comfortable without ListThemes", () => {
  const notReady: ThemeProbe = { ok: false, reason: "unsupported" };
  assert.equal(
    planVariants(notReady).some((v) => v.theme === "current"),
    false,
  );
  const variants = planVariants(notReady, { themes: ["current"] });
  assert.deepEqual(
    variants.map((v) => [v.id, v.skip === undefined]),
    [
      ["current-comfortable", true],
      ["current-compact", false],
    ],
  );
  assert.match(variants[1]!.skip!, /density "compact" cannot be selected/);
  const ready: ThemeProbe = { ok: true, appearance: appearance(allBuiltins()) };
  const both = planVariants(ready, { themes: ["current", "frost"] });
  assert.deepEqual(
    both.map((v) => [v.id, v.skip === undefined]),
    [
      ["current-comfortable", true],
      ["current-compact", true],
      ["frost-comfortable", true],
      ["frost-compact", true],
    ],
  );
});
