/**
 * `@nail/plugin-api/globals` — the globals of Nail's plug-in sandbox
 * besides `nail`: `console` (written to the plug-in's log), `setTimeout`,
 * `clearTimeout` and `queueMicrotask`. There is no `fetch` (use
 * `nail.http.fetch`), no `window`, `document`, `process` or `require`.
 *
 * Add it to a plug-in's tsconfig.json as `"types": ["@nail/plugin-api/globals"]`
 * (without DOM or Node types, which would declare what does not exist).
 */
export {};

declare global {
  /** Writes to the plug-in's log (`log` / `info` → info, `debug` / `trace` → debug). */
  var console: {
    log(...args: unknown[]): void;
    info(...args: unknown[]): void;
    debug(...args: unknown[]): void;
    warn(...args: unknown[]): void;
    error(...args: unknown[]): void;
    trace(...args: unknown[]): void;
  };
  /** Runs `fn` after `ms` milliseconds (timers stop with the plug-in). */
  function setTimeout<A extends unknown[]>(
    fn: (...args: A) => void,
    ms?: number,
    ...args: A
  ): number;
  /** Cancels a {@link setTimeout}. */
  function clearTimeout(id: number | undefined): void;
  /** Runs `fn` after the current task. */
  function queueMicrotask(fn: () => void): void;
}
