/**
 * The mock core's answers to the M4c print requests (ADR 0094): print jobs
 * kept like the core keeps them (an id, the print view's route, the ready
 * flag); agenda ranges checked; `SavePrintPdf` answers `unsupported`
 * (`desktop-unavailable`) like `nail serve`, or with `desktop` on writes
 * a simulated PDF like the shell. `../../api/mock.ts` routes exactly these
 * requests here.
 */
import type { PdfFile, PrintJob, Request, Response } from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { printViewRoute, targetIssue } from "./model";

export { MAX_AGENDA_DAYS, printViewRoute, targetIssue } from "./model";

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

/** Mock-core state of this feature. */
export interface PrintMockState {
  jobs: Record<string, PrintJob>;
  /** Whether a desktop host writes PDF files (`false` = `nail serve`). */
  desktop: boolean;
  /** PDF files the simulated host wrote. */
  pdfs: PdfFile[];
}

export function createPrintMockState(): PrintMockState {
  return { jobs: {}, desktop: false, pdfs: [] };
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
    case "SavePrintPdf": {
      const job = state.jobs[request.job_id];
      if (!job) return expired(request.job_id);
      if (!state.desktop) {
        return {
          type: "Error",
          code: "unsupported",
          message: "only the desktop app writes PDF files",
          localized: { id: "desktop-unavailable" },
        };
      }
      // The shell lays the view out in a hidden window, then prints it.
      job.ready = true;
      const file: PdfFile = { path: request.path, size_bytes: 48_213 };
      state.pdfs.push(file);
      return { type: "PdfSaved", file };
    }
  }
}
