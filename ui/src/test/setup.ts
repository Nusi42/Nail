import "@testing-library/jest-dom/vitest";
import { cleanup, configure } from "@testing-library/react";
import { afterAll, afterEach, beforeEach } from "vitest";
import { consoleProblems, installConsoleRecorder, resetConsole } from "./console";

// `findBy…` / `waitFor` give up after 1 s by default; when cargo builds run
// on the same machine, the larger widget suites (PIM calendar, editors)
// need longer. Passing tests are not slowed down.
configure({ asyncUtilTimeout: 5_000 });

// Console output fails tests (`./console.ts`): a test that logs
// `console.error` / `console.warn` — React's act(...) warning, a missing
// Fluent message, a failed action — fails unless it calls `expectConsole`
// or `allowConsole`. Vitest hides console output in piped runs, so these
// warnings would otherwise go unnoticed.
installConsoleRecorder();

// DOM methods that every browser (and WebView2) has but jsdom does not
// implement: the components call them unconditionally, so tests get no-op
// stand-ins. A test that checks a call replaces them with a spy.
function polyfill(target: object, name: string, value: () => unknown): void {
  if (!(name in target))
    Object.defineProperty(target, name, { value, configurable: true, writable: true });
}
const nothing = () => undefined;
// Suites that run in the `node` environment have no DOM at all.
if (typeof Element !== "undefined") {
  polyfill(Element.prototype, "scrollIntoView", nothing);
  polyfill(Element.prototype, "scrollTo", nothing);
  polyfill(Element.prototype, "setPointerCapture", nothing);
  polyfill(Element.prototype, "releasePointerCapture", nothing);
  polyfill(Element.prototype, "hasPointerCapture", () => false);
  polyfill(Document.prototype, "elementFromPoint", () => null);
}

function fail(problems: string[], when: string): void {
  if (problems.length > 0) throw new Error(`${when}:\n\n${problems.join("\n\n")}`);
}

beforeEach(() => {
  // Lines logged after the previous test's checks (its async tail).
  fail(consoleProblems(false), "Console output after the previous test ended");
  resetConsole();
});

afterEach(() => {
  // Unmount first (Testing Library's own cleanup is a no-op afterwards), so
  // what the unmount logs counts for this test.
  cleanup();
  fail(consoleProblems(true), "Console output during the test");
});

afterAll(() => {
  fail(consoleProblems(false), "Console output after the last test ended");
});
