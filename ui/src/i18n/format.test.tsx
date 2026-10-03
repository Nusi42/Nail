/**
 * Dates follow the UI locale (the Fluent locale in use), not the browser's
 * formatting locale (M2c carry-over WP65).
 */
import { act, render, screen, within } from "@testing-library/react";
import { readFileSync, readdirSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { createEnv, renderNode, stubElementSizes, widget } from "../test/harness";
import { I18nProvider } from "./I18nProvider";
import {
  DATE_TIME,
  dateFormatter,
  formatDateTime,
  formatRelative,
  formatShortDate,
  useDateFormat,
} from "./format";

const catalogs = { en: ["hello = Hello"], de: ["hello = Hallo"] };
const WHEN = new Date(2031, 2, 5, 14, 30); // 5 March 2031, 14:30 local time
const NOW = new Date(2031, 2, 7, 9, 0).getTime();

/** The browser's languages (`navigator.languages`) for one test. */
function browserLanguages(languages: string[]) {
  return vi.spyOn(navigator, "languages", "get").mockReturnValue(languages);
}

afterEach(() => {
  vi.restoreAllMocks();
});

/**
 * Lets the mock core answer and React commit until `query` finds something,
 * bounded by event-loop turns rather than wall-clock time (deterministic
 * under load).
 */
async function settleUntil<T>(query: () => T | null, what: string, turns = 100): Promise<T> {
  for (let turn = 0; turn < turns; turn++) {
    const found = query();
    if (found !== null) return found;
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
  }
  throw new Error(`${what} did not appear within ${turns} event-loop turns`);
}

function Probe() {
  const format = useDateFormat();
  return (
    <p>
      <span data-testid="locale">{format.locale}</span>
      <span data-testid="absolute">{format.dateTime(WHEN)}</span>
      <span data-testid="relative">{format.relative(WHEN, NOW)}</span>
    </p>
  );
}

function probe(override: string | null) {
  render(
    <I18nProvider locale={override} catalogs={catalogs}>
      <Probe />
    </I18nProvider>,
  );
  return {
    locale: screen.getByTestId("locale").textContent,
    absolute: screen.getByTestId("absolute").textContent,
    relative: screen.getByTestId("relative").textContent,
  };
}

describe("date formatting", () => {
  it("formats in the given locale", () => {
    expect(formatDateTime(WHEN, "de", DATE_TIME)).toBe(
      new Intl.DateTimeFormat("de", DATE_TIME).format(WHEN),
    );
    expect(formatDateTime(WHEN, "de", DATE_TIME)).not.toBe(formatDateTime(WHEN, "en", DATE_TIME));
    expect(formatRelative(new Date(NOW - 26 * 3600_000), NOW, "de")).toBe("gestern");
    expect(formatRelative(new Date(NOW - 26 * 3600_000), NOW, "en")).toBe("yesterday");
    expect(formatRelative(new Date(NOW + 3 * 86_400_000), NOW, "en")).toBe("in 3 days");
    expect(formatShortDate(WHEN, NOW, "de")).toBe(
      new Intl.DateTimeFormat("de", { day: "numeric", month: "short" }).format(WHEN),
    );
  });

  it("shows values that are not dates as they came", () => {
    expect(formatDateTime("not a date", "en")).toBe("not a date");
    expect(formatDateTime(Number.NaN, "en")).toBe("");
    expect(formatDateTime(WHEN.toISOString(), "en")).toBe(formatDateTime(WHEN, "en"));
    expect(dateFormatter("de").dateTime(WHEN.getTime())).toBe(formatDateTime(WHEN, "de"));
  });

  it("uses the UI locale override, not the browser's language", () => {
    browserLanguages(["de-DE", "de"]);
    const shown = probe("en");
    expect(shown.locale).toBe("en");
    expect(shown.absolute).toBe(formatDateTime(WHEN, "en"));
    expect(shown.absolute).not.toBe(formatDateTime(WHEN, "de-DE"));
    expect(shown.relative).toBe("2 days ago");
  });

  it("formats German dates for a German UI in an English browser", () => {
    browserLanguages(["en-US", "en"]);
    const shown = probe("de");
    expect(shown.locale).toBe("de");
    expect(shown.absolute).toBe(formatDateTime(WHEN, "de"));
    expect(shown.relative).toBe("vorgestern");
  });

  it("never formats in a browser language the UI does not speak", () => {
    // French is not shipped: the UI (texts and dates) is English.
    browserLanguages(["fr-FR", "fr"]);
    const shown = probe(null);
    expect(shown.locale).toBe("en");
    expect(shown.absolute).toBe(formatDateTime(WHEN, "en"));
    expect(shown.absolute).not.toBe(formatDateTime(WHEN, "fr-FR"));
    expect(shown.relative).toBe("2 days ago");
  });

  it("stays English in a German browser while the German catalogs are empty", () => {
    // M5b: the shipped `locales/de` files exist but hold no message yet.
    browserLanguages(["de-DE", "de"]);
    render(
      <I18nProvider locale={null} catalogs={{ en: ["hello = Hello"], de: ["# empty", ""] }}>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByTestId("locale").textContent).toBe("en");
    expect(screen.getByTestId("absolute").textContent).toBe(formatDateTime(WHEN, "en"));
    expect(screen.getByTestId("relative").textContent).toBe("2 days ago");
  });

  it("honours an explicit German choice even while its catalogs are empty", () => {
    browserLanguages(["en-US"]);
    render(
      <I18nProvider locale="de" catalogs={{ en: ["hello = Hello"], de: [""] }}>
        <Probe />
      </I18nProvider>,
    );
    expect(screen.getByTestId("locale").textContent).toBe("de");
    expect(screen.getByTestId("relative").textContent).toBe("vorgestern");
  });

  it("follows the negotiated catalog language, not the browser's region", () => {
    browserLanguages(["de-CH"]);
    const shown = probe(null);
    expect(shown.locale).toBe("de");
    expect(shown.absolute).toBe(formatDateTime(WHEN, "de"));
  });
});

describe("widgets show dates in the UI locale", () => {
  it("the outbox shows the send time in German in an English browser", async () => {
    browserLanguages(["en-US"]);
    const env = createEnv();
    const sendAt = WHEN.toISOString();
    const { draft_id } = await env.client.request(
      {
        type: "SaveDraft",
        draft: {
          id: null,
          identity_id: "ada@example.org",
          to: [{ name: "Grace", email: "grace@navy.example" }],
          cc: [],
          bcc: [],
          subject: "Later",
          mode: "plain",
          text: "x",
          html: null,
          markdown: null,
          attachments: [],
          context: null,
          signature: "identity",
          markdown_part: false,
        },
      },
      "DraftSaved",
    );
    await env.client.request(
      { type: "SendDraft", draft_id, delay_secs: null, send_at: sendAt },
      "SendQueued",
    );
    renderNode(widget("outbox"), env, { locale: "de" });
    const item = await settleUntil(
      () => document.querySelector<HTMLElement>('[data-part="outbox.item"]'),
      "the outbox item",
    );
    expect(item).toHaveTextContent("Later");
    const time = item.querySelector('[data-part="outbox.item.time"]');
    // The text is English (empty German catalog), the date German.
    expect(time).toHaveTextContent(`Due ${formatDateTime(WHEN, "de", DATE_TIME)}`);
    expect(time?.textContent).not.toContain(formatDateTime(WHEN, "en-US", DATE_TIME));
  });

  it("message list rows show relative dates in the UI locale", async () => {
    browserLanguages(["en-US"]);
    const restore = stubElementSizes();
    try {
      const env = createEnv({ state: { activeFolderId: "acc-personal/Inbox" } });
      renderNode(
        widget("message-list", { options: { "page-size": 20 }, rows: ["{date | relative}"] }),
        env,
        { locale: "de" },
      );
      const rows = await settleUntil(() => {
        const found = screen.queryAllByRole("option");
        return found.length > 0 ? found : null;
      }, "message rows");
      const dates = rows.map(
        (row) => row.querySelector('[data-part="message-list.row.date"]')?.textContent ?? "",
      );
      expect(dates.length).toBeGreaterThan(0);
      for (const text of dates) {
        expect(text).toMatch(/^(vor |gestern|vorgestern|jetzt|heute|in |morgen|übermorgen)/);
        expect(text).not.toMatch(/ago|yesterday|now|tomorrow/);
      }
      expect(within(rows[0] as HTMLElement).getByText(dates[0] as string)).toBeInTheDocument();
    } finally {
      restore();
    }
  });
});

describe("source rule", () => {
  // Vitest runs with the ui/ package as working directory.
  const src = resolve(process.cwd(), "src");

  function sources(dir: string): string[] {
    return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) return entry.name === "generated" ? [] : sources(path);
      return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : [];
    });
  }

  it("formats dates only through i18n/format.ts (always in the UI locale)", () => {
    const direct =
      /new Intl\.(DateTimeFormat|RelativeTimeFormat)\(|\.toLocale(Date|Time)?String\(|DateTimeFormat\(\)/;
    const allowed = (path: string) => path === "i18n/format.ts";
    const offenders = sources(src)
      .filter((path) => !allowed(relative(src, path).replaceAll("\\", "/")))
      .filter((path) => direct.test(readFileSync(path, "utf8")))
      .map((path) => relative(src, path));
    expect(offenders).toEqual([]);
  });
});
