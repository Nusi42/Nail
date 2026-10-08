import { render, screen } from "@testing-library/react";
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { FluentResource } from "@fluent/bundle";
import { defaultLayout } from "../api/mockData";
import { shippedCatalogs, groupCatalogs } from "./catalogs";
import { I18nProvider, useLabel, useT } from "./I18nProvider";
import { createLocalizer, labelMessageId, negotiateLocale, resolveLabel } from "./localizer";
import { must } from "../test/must";
import { sourceFiles } from "../test/sourceFiles";

const catalogs = {
  en: ["hello = Hello\nbye = Goodbye\ncount = { $n ->\n [one] One mail\n *[other] { $n } mails\n}"],
  de: ["hello = Hallo"],
};

describe("localizer", () => {
  it("resolves keys in the selected language and falls back to English per key", () => {
    const l = createLocalizer(catalogs, ["de-DE"]);
    expect(l.locale).toBe("de");
    expect(l.t("hello")).toBe("Hallo");
    expect(l.t("bye")).toBe("Goodbye");
    expect(l.t("count", { n: 3 })).toBe("3 mails");
    expect(l.t("count", { n: 1 })).toBe("One mail");
  });

  it("returns the id for keys missing everywhere (and warns in dev)", () => {
    const warn = vi.spyOn(console, "warn").mockReturnValue(undefined);
    const l = createLocalizer(catalogs, ["en"], true);
    expect(l.t("missing-key")).toBe("missing-key");
    expect(l.has("missing-key")).toBe(false);
    expect(warn).toHaveBeenCalledOnce();
    warn.mockRestore();
  });

  it("negotiates the locale from the requested list", () => {
    expect(negotiateLocale(["de-AT", "en"], ["en", "de"])).toBe("de");
    expect(negotiateLocale(["fr-FR"], ["en", "de"])).toBe("en");
    expect(negotiateLocale([], ["en", "de"])).toBe("en");
  });

  it("maps @labels to Fluent ids and keeps literal labels", () => {
    expect(labelMessageId("@mail.action.reply-all")).toBe("mail-action-reply-all");
    const l = createLocalizer(shippedCatalogs, ["en"]);
    expect(resolveLabel(l, "@mail.action.reply")).toBe("Reply");
    expect(resolveLabel(l, "My pane")).toBe("My pane");
  });

  it("groups catalog files by locale directory", () => {
    expect(
      groupCatalogs({
        "/x/locales/en/a.ftl": "a",
        "/x/locales/de/a.ftl": "b",
        "/x/other.txt": "c",
      }),
    ).toEqual({
      de: ["b"],
      en: ["a"],
    });
  });
});

describe("shipped catalogs", () => {
  // Vitest runs with the ui/ package as working directory.
  const localesDir = resolve(process.cwd(), "..", "locales");
  const srcDir = resolve(process.cwd(), "src");

  it("bundles en and de", () => {
    expect(Object.keys(shippedCatalogs).sort()).toEqual(["de", "en"]);
    expect(createLocalizer(shippedCatalogs, ["en"]).t("app-name")).toBe("Nail");
  });

  it("a German browser stays English while the shipped German catalog is empty", () => {
    expect(createLocalizer(shippedCatalogs, ["de"]).locale).toBe("en");
  });

  it("falls back from the (empty) German catalog to English", () => {
    const l = createLocalizer(shippedCatalogs, ["de"], false, "de");
    expect(l.locale).toBe("de");
    expect(l.t("message-list-empty")).toBe("No messages in this folder.");
  });

  it("has the same files in every locale", () => {
    const files = (lang: string) => readdirSync(join(localesDir, lang)).sort();
    expect(files("de")).toEqual(files("en"));
  });

  it("contains every message id used by the UI source and the default layout", () => {
    const en = createLocalizer(shippedCatalogs, ["en"]);
    const ids = new Set<string>();
    for (const path of sourceFiles(srcDir, {
      include: /\.tsx?$/,
      exclude: /\.test\.tsx?$/,
      skipDirs: ["test"],
    })) {
      const source = readFileSync(path, "utf8");
      for (const m of source.matchAll(/\bt\(\s*"([a-z0-9-]+)"/g)) ids.add(must(m[1]));
      for (const m of source.matchAll(/messageId(?:=|: )"([a-z0-9-]+)"/g)) ids.add(must(m[1]));
    }
    for (const field of ["from", "to", "cc", "reply-to", "date"])
      ids.add(`message-header-${field}`);
    for (const code of [
      "invalid-request",
      "not-found",
      "unsupported",
      "auth-failed",
      "network",
      "config",
      "unavailable",
      "internal",
    ]) {
      ids.add(`error-${code}`);
    }
    const layout = defaultLayout();
    for (const def of Object.values(layout.actions))
      if (def.label?.startsWith("@")) ids.add(labelMessageId(def.label));
    expect(ids.size).toBeGreaterThan(30);
    expect([...ids].filter((id) => !en.has(id))).toEqual([]);
  });

  it("parses without errors", () => {
    for (const source of shippedCatalogs.en ?? []) {
      expect(
        new FluentResource(source).body.filter((e) => !("id" in e) && !("content" in e)),
      ).toEqual([]);
    }
  });
});

function Probe() {
  const t = useT();
  const label = useLabel();
  return (
    <p>
      {t("hello")} / {t("bye")} / {label("@layout.pane.folders")}
    </p>
  );
}

/** The catalogs plus the layout label `Probe` resolves. */
const probeCatalogs = { ...catalogs, en: [...catalogs.en, "layout-pane-folders = Folders"] };

describe("I18nProvider", () => {
  it("uses the override, falls back per key and sets <html lang>", () => {
    render(
      <I18nProvider locale="de" catalogs={probeCatalogs}>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByText("Hallo / Goodbye / Folders")).toBeInTheDocument();
    expect(document.documentElement.lang).toBe("de");
  });

  it("follows navigator.language without an override", () => {
    const spy = vi.spyOn(navigator, "languages", "get").mockReturnValue(["de-CH", "en"]);
    render(
      <I18nProvider catalogs={probeCatalogs}>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByText(/^Hallo/)).toBeInTheDocument();
    spy.mockRestore();
  });
});
