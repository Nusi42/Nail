/**
 * Fluent-based localization (design.md, "Languages").
 *
 * Each key resolves as selected language → English. A key missing in both
 * yields the key itself (and a warning in development), so a gap never
 * breaks the UI.
 */
import { FluentBundle, FluentResource, type FluentVariable } from "@fluent/bundle";
import { negotiateLanguages } from "@fluent/langneg";

/** The language every catalog falls back to. */
export const FALLBACK_LOCALE = "en";

/** Fluent source text per locale: `locale → [file contents]`. */
export type Catalogs = Record<string, string[]>;

export type MessageArgs = Record<string, FluentVariable>;

export interface Localizer {
  /** The selected locale (e.g. `"de"`). */
  readonly locale: string;
  /** Formats message `id`; falls back to English per key. */
  t(id: string, args?: MessageArgs): string;
  /** Whether `id` exists in the selected language or English. */
  has(id: string): boolean;
}

function buildBundle(locale: string, sources: readonly string[]): FluentBundle {
  const bundle = new FluentBundle(locale, { useIsolating: false });
  for (const source of sources) {
    const errors = bundle.addResource(new FluentResource(source));
    for (const error of errors) {
      console.warn(`nail: Fluent error in ${locale}`, error);
    }
  }
  return bundle;
}

/** Picks the best available locale for the requested ones. */
export function negotiateLocale(
  requested: readonly string[],
  available: readonly string[],
): string {
  const [best] = negotiateLanguages([...requested], [...available], {
    strategy: "lookup",
    defaultLocale: FALLBACK_LOCALE,
  });
  return best ?? FALLBACK_LOCALE;
}

/** Whether a locale's catalogs define at least one message or term. */
function hasMessages(sources: readonly string[] | undefined): boolean {
  return (sources ?? []).some((source) => new FluentResource(source).body.length > 0);
}

/**
 * The locales the browser's languages are negotiated against: English, every
 * locale whose catalogs hold at least one message (an empty `locales/de`
 * folder must not make dates German while every text is English), and the
 * explicit override's locale, which is honoured even when its catalogs are
 * still empty.
 */
export function availableLocales(catalogs: Catalogs, override?: string | null): string[] {
  return Object.keys(catalogs).filter(
    (locale) =>
      locale === FALLBACK_LOCALE ||
      hasMessages(catalogs[locale]) ||
      (override != null && negotiateLocale([override], [locale]) === locale),
  );
}

/**
 * Creates a localizer over `catalogs` for the requested locales. `override`
 * is the explicit choice (settings / `?lang=`), see `availableLocales`.
 */
export function createLocalizer(
  catalogs: Catalogs,
  requested: readonly string[],
  warnMissing = false,
  override: string | null = null,
): Localizer {
  const locale = negotiateLocale(requested, availableLocales(catalogs, override));
  const chain: FluentBundle[] = [];
  if (locale !== FALLBACK_LOCALE) {
    chain.push(buildBundle(locale, catalogs[locale] ?? []));
  }
  chain.push(buildBundle(FALLBACK_LOCALE, catalogs[FALLBACK_LOCALE] ?? []));
  const warned = new Set<string>();

  const find = (id: string) => {
    for (const bundle of chain) {
      const message = bundle.getMessage(id);
      if (message?.value) {
        return { bundle, pattern: message.value };
      }
    }
    return null;
  };

  return {
    locale,
    has: (id) => find(id) !== null,
    t(id, args) {
      const found = find(id);
      if (!found) {
        if (warnMissing && !warned.has(id)) {
          warned.add(id);
          console.warn(`nail: missing message "${id}"`);
        }
        return id;
      }
      const errors: Error[] = [];
      const text = found.bundle.formatPattern(found.pattern, args ?? null, errors);
      if (errors.length > 0 && warnMissing) {
        console.warn(`nail: formatting "${id}"`, errors);
      }
      return text;
    },
  };
}

/**
 * Resolves a layout label: `@mail.action.reply` → message `mail-action-reply`
 * (ADR 0003); anything else is literal text from the user's layout file.
 */
export function resolveLabel(localizer: Localizer, label: string): string {
  if (label.startsWith("@")) {
    return localizer.t(labelMessageId(label));
  }
  return label;
}

/** Fluent id for an `@a.b.c` label. */
export function labelMessageId(label: string): string {
  return label.replace(/^@/, "").replaceAll(".", "-");
}
