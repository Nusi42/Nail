/** Node-side tools of `@nail/plugin-api` (packing `.nailplugin` archives). */
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

/** Permissions of the preview API (plus `network:<host>`). */
export declare const PREVIEW_PERMISSIONS: string[];
/** Permissions of the full API, refused in the preview. */
export declare const FUTURE_PERMISSIONS: string[];
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
