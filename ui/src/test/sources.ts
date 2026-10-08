/**
 * The UI's own source text (components and modules, without tests), for
 * tests that check that a part a skin or list names is rendered somewhere.
 */
export const uiSources: string = Object.values(
  import.meta.glob<string>(["../**/*.{ts,tsx}", "!../**/*.test.{ts,tsx}", "!../test/**"], {
    query: "?raw",
    import: "default",
    eager: true,
  }),
).join("\n");
