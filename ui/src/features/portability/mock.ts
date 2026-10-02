/**
 * The mock core's answers to the M4c import / export / backup / restore /
 * diagnostics requests and the desktop file dialog (ADR 0094), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * Contract stage (WP120): like the core, every request answers
 * `unsupported` (`m4c-not-ready`) and `PickPath` answers `unsupported`
 * (`desktop-unavailable`). WP124 replaces this with a simulation of the
 * jobs (progress events, reports, Thunderbird proposals, backups) on
 * `data.features.portability`.
 */
import type { Request, Response } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const PORTABILITY_REQUESTS = [
  "FindImportSources",
  "PreviewImport",
  "StartImport",
  "StartExport",
  "CreateBackup",
  "InspectBackup",
  "VerifyBackup",
  "PreviewRestore",
  "StartRestore",
  "ListPortabilityJobs",
  "GetPortabilityJob",
  "CancelPortabilityJob",
  "PreviewDiagnostics",
  "CreateDiagnosticsBundle",
  "PickPath",
] as const satisfies readonly Request["type"][];

export type PortabilityRequest = Extract<Request, { type: (typeof PORTABILITY_REQUESTS)[number] }>;

export function isPortabilityRequest(request: Request): request is PortabilityRequest {
  return (PORTABILITY_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature (WP124 fills it). */
export interface PortabilityMockState {
  /** Whether a desktop host exists (file dialogs). */
  desktop: boolean;
}

export function createPortabilityMockState(): PortabilityMockState {
  return { desktop: false };
}

/** `unsupported` with a Fluent id, as the core answers. */
export function notReady(request: Request, id = "m4c-not-ready"): Response {
  return {
    type: "Error",
    code: "unsupported",
    message: `${request.type} is not available yet`,
    localized: { id },
  };
}

export function answerPortability(_core: MockCore, request: PortabilityRequest): Response {
  if (request.type === "PickPath") return notReady(request, "desktop-unavailable");
  return notReady(request);
}
