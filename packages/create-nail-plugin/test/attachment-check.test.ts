/**
 * The "Attachment check" example (examples/attachment-check): its keyword
 * logic and its before-send hook against the SDK's test host, in English
 * and German (the keyword lists come from the plug-in's catalogs).
 */
import path from "node:path";
import { pathToFileURL } from "node:url";

import type { PluginOutgoingMessage } from "@nail/plugin-api";
import { loadCatalogs } from "@nail/plugin-api/tools";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";

import {
  loadBundle,
  prepareExample,
  readManifest,
  type LoadedPlugin,
  type PreparedExample,
} from "./helpers";

type Check = typeof import("../../../examples/attachment-check/src/check");

let example: PreparedExample;
let check: Check;
beforeAll(async () => {
  example = prepareExample("attachment-check");
  check = (await import(pathToFileURL(path.join(example.source, "src", "check.ts")).href)) as Check;
});
afterAll(() => example.cleanup());

const outgoing = (
  text: string,
  extra: Partial<PluginOutgoingMessage> = {},
): PluginOutgoingMessage => ({
  draftId: "d1",
  accountId: "work",
  identityId: "work-main",
  from: { name: null, email: "me@example.org" },
  to: [{ name: "Ada", email: "ada@example.org" }],
  cc: [],
  bcc: [],
  subject: "Quarterly report",
  text,
  attachments: [],
  encrypted: false,
  ...extra,
});

describe("keywords", () => {
  it("parses lists with prefixes, phrases and duplicates", () => {
    expect(check.parseKeywords(" attach*, enclos* ,\nsee  the file, ATTACH*, , x*y, *")).toEqual([
      { text: "attach", prefix: true },
      { text: "enclos", prefix: true },
      { text: "see the file", prefix: false },
    ]);
  });

  it("finds whole words, any case, the first one in the text", () => {
    const keywords = check.parseKeywords("attach*, see the file, anbei");
    expect(check.findMention("Please find the report ATTACHED.", keywords)).toBe("ATTACHED");
    expect(check.findMention("See\nthe   file I sent; it is attached", keywords)).toBe(
      "See\nthe   file",
    );
    expect(check.findMention("Reattach the cable", keywords)).toBeNull(); // not a word start
    expect(check.findMention("Anbeiß", keywords)).toBeNull(); // not the whole word
    expect(check.findMention("Die Datei ist anbei.", keywords)).toBe("anbei");
    expect(check.findMention("", keywords)).toBeNull();
  });

  it("ignores the signature and, unless asked, quoted text", () => {
    const text = "Thanks!\n> I attached the old one\n-- \nSent with attachments enabled";
    expect(check.ownText(text, false)).toBe("Thanks!");
    expect(check.ownText(text, true)).toBe("Thanks!\n> I attached the old one");
    expect(check.ownText("a\r\nb", false)).toBe("a\nb");
  });
});

describe("the before-send hook", () => {
  let loaded: LoadedPlugin | undefined;
  afterEach(() => {
    loaded?.restore();
    loaded = undefined;
  });

  async function load(options: Parameters<typeof loadBundle>[1] = {}) {
    loaded = await loadBundle(example.bundle, {
      manifest: readManifest(example.source),
      catalogs: loadCatalogs(example.source),
      ...options,
    });
    return loaded.host;
  }

  it("vetoes a mail that mentions a missing attachment, naming the word", async () => {
    const host = await load();
    expect(host.hookNames()).toEqual(["beforeSend"]);
    const run = await host.runBeforeSend(outgoing("Hi Ada,\nthe figures are enclosed.\nBest"));
    expect(run).toEqual({
      outcome: "vetoed",
      result: {
        veto: {
          reason:
            "You mention “enclosed”, but nothing is attached. Attach the file, or send the message anyway.",
        },
      },
      problems: [],
    });
    // The subject counts too.
    expect(
      (await host.runBeforeSend(outgoing("Hi", { subject: "Attachment: report" }))).outcome,
    ).toBe("vetoed");
  });

  it("lets mail through that has an attachment or mentions none", async () => {
    const host = await load();
    const withFile = outgoing("Report attached.", {
      attachments: [{ name: "report.pdf", mimeType: "application/pdf", size: 1200 }],
    });
    expect(await host.runBeforeSend(withFile)).toEqual({ outcome: "ok", problems: [] });
    expect(await host.runBeforeSend(outgoing("See you tomorrow."))).toEqual({
      outcome: "ok",
      problems: [],
    });
    // Quoted text is not checked unless the setting says so.
    const reply = outgoing("Sounds good.\n\n> I attached the draft.");
    expect((await host.runBeforeSend(reply)).outcome).toBe("ok");
    host.changeSettings({ "check-quoted": true });
    expect((await host.runBeforeSend(reply)).outcome).toBe("vetoed");
  });

  it("uses the keywords of Nail's language", async () => {
    const host = await load({ locale: "de" });
    const run = await host.runBeforeSend(
      outgoing("Hallo Ada,\nanbei die Zahlen.", { subject: "Zahlen" }),
    );
    expect(run.result?.veto?.reason).toBe(
      "Sie erwähnen „anbei“, aber es ist nichts angehängt. Hängen Sie die Datei an oder senden Sie die Nachricht trotzdem.",
    );
    // English words are not on the German list.
    expect((await host.runBeforeSend(outgoing("Report attached.", { subject: "x" }))).outcome).toBe(
      "ok",
    );
    // A language without a catalog falls back to English.
    const french = await load({ locale: "fr" });
    expect(
      (await french.runBeforeSend(outgoing("Report attached.", { subject: "x" }))).outcome,
    ).toBe("vetoed");
  });

  it("adds the user's own words", async () => {
    const host = await load({ settings: { "extra-keywords": "spreadsheet*, PFA" } });
    expect(
      (await host.runBeforeSend(outgoing("The spreadsheets are ready", { subject: "x" }))).result,
    ).toMatchObject({ veto: { reason: expect.stringContaining("“spreadsheets”") } });
    expect((await host.runBeforeSend(outgoing("pfa", { subject: "x" }))).outcome).toBe("vetoed");
  });

  it("is skipped without its permission", async () => {
    const host = await load({ permissions: [] });
    expect((await host.runBeforeSend(outgoing("attached"))).outcome).toBe("skipped");
  });
});
