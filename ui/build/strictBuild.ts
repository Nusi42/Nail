// Zero warnings for `vite build` (Q1, ADRs 0123 and 0130): a warning that
// Vite or Rolldown prints during a production build (a chunk over the size
// limit, a circular chunk, `eval`, an unresolvable import, ...) fails the
// build instead of scrolling by in a green CI log.
//
// Vite's own notices (the chunk-size warning among them) go through its
// logger; Rolldown's logs reach the same logger through Vite's `onLog`
// handler. The logger records every warning and the plugin fails the build
// once the bundle is written, so all warnings of the run are shown first.
//
// The chunk-size warning is checked here as well: Vite 8 reports it from
// Rolldown's native reporter, whose log reaches the JS logger
// asynchronously — on a busy machine only after `closeBundle` had passed,
// and the build succeeded (P1 WP193). The plugin measures the chunks itself
// in `generateBundle`, with the reporter's rule (minified client builds, not
// libraries; bytes / 1000 over `build.chunkSizeWarningLimit` kB).
import { createLogger, type Logger, type LogLevel, type Plugin, type ResolvedConfig } from "vite";

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

/** A chunk of the output bundle (the part of Rolldown's output this needs). */
interface OutputChunkLike {
  type: string;
  fileName: string;
  code?: string;
}

/**
 * The chunks over `build.chunkSizeWarningLimit` kB, with their sizes, when
 * Vite's reporter would warn about them at all.
 */
export function largeChunks(
  config: Pick<ResolvedConfig, "build"> | undefined,
  consumer: string,
  bundle: Record<string, OutputChunkLike>,
): string[] {
  const build = config?.build;
  if (!build || !build.minify || build.lib || consumer !== "client") return [];
  const limit = build.chunkSizeWarningLimit;
  const large: string[] = [];
  for (const chunk of Object.values(bundle)) {
    if (chunk.type !== "chunk" || chunk.code === undefined) continue;
    const kB = Buffer.byteLength(chunk.code) / 1000;
    if (kB > limit) large.push(`${chunk.fileName} (${kB.toFixed(2)} kB)`);
  }
  return large;
}

function chunkSizeWarning(config: Pick<ResolvedConfig, "build"> | undefined, large: string[]) {
  const limit = String(config?.build.chunkSizeWarningLimit ?? "");
  return `Some chunks are larger than ${limit} kB after minification: ${large.join(", ")}`;
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
  let config: ResolvedConfig | undefined;
  const plugin: Plugin = {
    name: "nail:strict-build",
    apply: "build",
    enforce: "post",
    configResolved(resolved) {
      config = resolved;
    },
    generateBundle: {
      order: "post",
      handler(_options, bundle) {
        const large = largeChunks(config, this.environment.config.consumer, bundle);
        if (large.length > 0) warnings.push(chunkSizeWarning(config, large));
      },
    },
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
