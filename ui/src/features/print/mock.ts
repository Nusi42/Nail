/**
 * The mock core's answers to the M4c print requests (ADR 0094): print jobs
 * kept like the core keeps them (an id, the print view's route, the ready
 * flag); agenda ranges checked; `SavePrintPdf` answers `unsupported`
 * (`desktop-unavailable`) — the mock has no desktop host. `../../api/mock.ts`
 * routes exactly these requests here.
 */
import type { PrintJob, PrintTarget, Request, Response } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";

/** The requests this feature answers. */
export const PRINT_REQUESTS = [
  "CreatePrintJob",
  "GetPrintJob",
  "PrintViewReady",
  "SavePrintPdf",
] as const satisfies readonly Request["type"][];

export type PrintRequest = Extract<Request, { type: (typeof PRINT_REQUESTS)[number] }>;

export function isPrintRequest(request: Request): request is PrintRequest {
  return (PRINT_REQUESTS as readonly string[]).includes(request.type);
}

/** `nail_api::print::print_view_route`. */
export function printViewRoute(jobId: string): string {
  return `#/print/${jobId}`;
}

/** `nail_api::print::MAX_AGENDA_DAYS`. */
export const MAX_AGENDA_DAYS = 366;

/** `PrintTarget::issues` (the first one), as a Fluent id; `null` = fine. */
export function targetIssue(target: PrintTarget): string | null {
  if (target.type !== "agenda") return null;
  const day = (d: string) =>
    /^\d{4}-\d{2}-\d{2}$/.test(d) && !Number.isNaN(Date.parse(`${d}T00:00:00Z`))
      ? Date.parse(`${d}T00:00:00Z`) / 86_400_000
      : null;
  const start = day(target.start);
  const end = day(target.end);
  if (start === null || end === null) return "print-target-issue-invalid-date";
  if (end < start) return "print-target-issue-end-before-start";
  if (end - start + 1 > MAX_AGENDA_DAYS) return "print-target-issue-range-too-long";
  return null;
}

/** Mock-core state of this feature. */
export interface PrintMockState {
  jobs: Record<string, PrintJob>;
}

export function createPrintMockState(): PrintMockState {
  return { jobs: {} };
}

function expired(jobId: string): Response {
  return {
    type: "Error",
    code: "not_found",
    message: `print job ${jobId} is gone`,
    localized: { id: "print-job-expired" },
  };
}

export function answerPrint(core: MockCore, request: PrintRequest): Response {
  const state = core.data.features.print;
  switch (request.type) {
    case "CreatePrintJob": {
      const issue = targetIssue(request.target);
      if (issue !== null) {
        return { type: "Error", code: "invalid_request", message: issue, localized: { id: issue } };
      }
      const jobId = core.nextId("print-");
      const job: PrintJob = {
        job_id: jobId,
        target: structuredClone(request.target),
        options: structuredClone(request.options),
        created_at: new Date().toISOString(),
        view: printViewRoute(jobId),
        ready: false,
      };
      state.jobs[jobId] = job;
      return { type: "PrintJob", job: structuredClone(job) };
    }
    case "GetPrintJob": {
      const job = state.jobs[request.job_id];
      return job ? { type: "PrintJob", job: structuredClone(job) } : expired(request.job_id);
    }
    case "PrintViewReady": {
      const job = state.jobs[request.job_id];
      if (!job) return expired(request.job_id);
      job.ready = true;
      return { type: "Done" };
    }
    case "SavePrintPdf":
      return state.jobs[request.job_id]
        ? {
            type: "Error",
            code: "unsupported",
            message: "only the desktop app writes PDF files",
            localized: { id: "desktop-unavailable" },
          }
        : expired(request.job_id);
  }
}
