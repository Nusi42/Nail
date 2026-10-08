/** Node-side tools of `@nail/plugin-api` (packing `.nailplugin` archives). */
import type { BuildOptions } from "esbuild";
import type { PluginManifest } from "../src/generated";

/** A problem found in `manifest.json` (same kinds as Nail's `PluginManifestIssue`). */
export interface ManifestIssue {
  /** The issue kind (`invalid-id`, `unknown-command`, …). */
  kind: string;
  /** Where, as a JSON path. */
  path: string;
  /** The offending value. */
  detail?: string;
}

/** Every permission Nail offers (plus `network:<host>`). */
export declare const PERMISSIONS: string[];
/** Permissions of the design this version refuses (`permission-not-in-preview`). */
export declare const FUTURE_PERMISSIONS: string[];
/** Permissions another one needs in the same manifest. */
export declare const REQUIRED_PERMISSIONS: Record<string, string[]>;
/** Permissions a grant can limit to accounts. */
export declare const ACCOUNT_PERMISSIONS: string[];
/** Permissions that change the user's data. */
export declare const WRITE_PERMISSIONS: string[];
/** Hook kinds and their permissions. */
export declare const HOOK_PERMISSIONS: Record<"arrival" | "beforeSend" | "open", string>;
/** Events a background job can subscribe to. */
export declare const JOB_EVENTS: string[];
/** MCP scopes an exposed tool may require. */
export declare const MCP_SCOPES: string[];
/** A MIME type pattern of a contribution (`text/csv`, `image/*`). */
export declare function isMimePattern(mime: unknown): boolean;
/** A file extension of a contribution (`csv`). */
export declare function isExtension(ext: unknown): boolean;
/** kebab-case, 1–64 characters, starting with a letter. */
export declare function isKebabName(s: unknown): boolean;
/** A semantic version. */
export declare function isVersion(s: unknown): boolean;
/** A relative `/`-separated path inside a package. */
export declare function isPackagePath(s: unknown): boolean;
/** A `network:` host pattern. */
export declare function isNetworkHost(pattern: unknown): boolean;
/** Checks a parsed manifest; empty when it looks fine. */
export declare function checkManifest(manifest: unknown): ManifestIssue[];

/** Largest `manifest.json` (bytes). */
export declare const MAX_MANIFEST_BYTES: number;
/** Largest package (bytes). */
export declare const MAX_PACKAGE_BYTES: number;
/** `nailplugin`. */
export declare const PACKAGE_EXTENSION: string;

/** A packing failure with the manifest issues (if any). */
export declare class PackError extends Error {
  constructor(message: string, issues?: ManifestIssue[]);
  /** The manifest issues. */
  issues: ManifestIssue[];
}

/** Reads and checks `<dir>/manifest.json`; throws {@link PackError}. */
export declare function readManifest(dir: string): { manifest: PluginManifest; bytes: Buffer };
/** The files a package contains, in archive order (manifest first). */
export declare function packageFiles(dir: string, manifest: PluginManifest): string[];

/** The result of {@link packPlugin}. */
export interface PackResult {
  /** The written archive. */
  file: string;
  /** Its SHA-256 (hex), as the install dialog shows it. */
  sha256: string;
  /** Its size in bytes. */
  size: number;
  /** The archived files. */
  files: string[];
  /** The manifest. */
  manifest: PluginManifest;
}

/** The esbuild options of the plug-in in `dir` (`src/index.tsx` into the manifest's `main`, or `outfile`). */
export declare function pluginBuildOptions(
  dir: string,
  options?: { outfile?: string },
): BuildOptions;
/** Builds the plug-in in `dir` once, or on every change with `watch` (esbuild is a peer dependency). */
export declare function buildPlugin(options: {
  dir: string;
  watch?: boolean;
  outfile?: string;
}): Promise<void>;

/** Whether the module at `moduleUrl` (its `import.meta.url`) is the program. */
export declare function isProgram(moduleUrl: string): boolean;
/** Runs `main` with the program's arguments when the module is the program; sets the exit code. */
export declare function runAsProgram(
  moduleUrl: string,
  main: (argv: string[]) => number | Promise<number>,
): Promise<void>;

/** Packs `dir` into `out` (default `<dir>/dist/<id>.nailplugin`). */
export declare function packPlugin(options: { dir: string; out?: string | undefined }): PackResult;

/** A zip entry. */
export interface ZipEntry {
  /** `/`-separated name. */
  name: string;
  /** Contents. */
  data: Uint8Array;
}
/** Builds a deterministic zip archive. */
export declare function createZip(entries: ZipEntry[]): Buffer;
/** Reads a zip archive (stored / deflated entries). */
export declare function readZip(bytes: Uint8Array): { name: string; data: Buffer }[];

/** A parsed background job schedule. */
export interface CronSchedule {
  minutes: Set<number>;
  hours: Set<number>;
  days: Set<number>;
  months: Set<number>;
  /** 0–6, Sunday = 0. */
  weekdays: Set<number>;
  dayRestricted: boolean;
  weekdayRestricted: boolean;
}
/** Why a schedule is refused (Nail's `CronIssue`). */
export declare class CronError extends Error {
  constructor(issue: "field-count" | "invalid-field" | "too-frequent");
  issue: "field-count" | "invalid-field" | "too-frequent";
}
/** Shortest interval between two runs (minutes). */
export declare const MIN_JOB_INTERVAL_MINUTES: number;
/** Parses a schedule (five cron fields or `@hourly` / `@daily` / `@weekly`); throws {@link CronError}. */
export declare function parseCron(text: string): CronSchedule;
/** Whether a schedule is valid. */
export declare function isCron(text: unknown): boolean;
/** Whether the schedule fires at this local time. */
export declare function cronMatches(schedule: CronSchedule, date: Date): boolean;
/** The next local time after `after` the schedule fires. */
export declare function nextCronRun(schedule: CronSchedule, after: Date): Date | null;

/** The plug-in's Fluent catalogs (language tag → `.ftl` text) for the test host's `nail.l10n`. */
export declare function loadCatalogs(dir: string, manifest?: unknown): Record<string, string>;
