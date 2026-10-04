// Zero warnings for `vite build` (Q1, ADRs 0123 and 0130): a warning that
// Vite or Rolldown prints during a production build (a chunk over the size
// limit, a circular chunk, `eval`, an unresolvable import, ...) fails the
// build instead of scrolling by in a green CI log.
//
// Vite's own notices (the chunk-size warning among them) go through its
// logger; Rolldown's logs reach the same logger through Vite's `onLog`
// handler. The logger records every warning and the plugin fails the build
// once the bundle is written, so all warnings of the run are shown first.
import { createLogger, type Logger, type LogLevel, type Plugin } from "vite";

/** The logger and plugin of one strict build. */
export interface StrictBuild {
  /** Pass as `customLogger`: prints as usual and records each warning. */
  readonly logger: Logger;
  /** Fails the build when the logger recorded a warning. */
  readonly plugin: Plugin;
  /** The warnings recorded so far. */
  readonly warnings: readonly string[];
}

/** Strips ANSI colour codes (Vite colours its warnings). */
function plain(message: string): string {
  // eslint-disable-next-line no-control-regex -- ANSI escape sequences start with ESC (U+001B)
  return message.replace(/\u001b\[[0-9;]*m/g, "").trim();
}

/**
 * A logger + plugin pair that turns build warnings into a build error.
 *
 * @param logLevel what the logger prints (warnings are recorded at any level)
 */
export function strictBuild(logLevel: LogLevel = "info"): StrictBuild {
  const logger = createLogger(logLevel);
  const warnings: string[] = [];
  const warn = logger.warn.bind(logger);
  const warnOnce = logger.warnOnce.bind(logger);
  logger.warn = (message, options) => {
    warnings.push(plain(message));
    warn(message, options);
  };
  logger.warnOnce = (message, options) => {
    warnings.push(plain(message));
    warnOnce(message, options);
  };
  const plugin: Plugin = {
    name: "nail:strict-build",
    apply: "build",
    enforce: "post",
    closeBundle: {
      order: "post",
      handler() {
        if (warnings.length > 0) {
          throw new Error(
            `the build printed ${String(warnings.length)} warning(s); Nail builds with zero ` +
              `warnings (ADR 0123):\n${warnings.map((w) => `- ${w}`).join("\n")}`,
          );
        }
      },
    },
  };
  return { logger, plugin, warnings };
}
