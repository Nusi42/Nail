/**
 * Server data of the import / export / backup UI (TanStack Query through
 * `NailClient`). Jobs live under `["portability", "job", id]`; the core's
 * `PortabilityJobProgress` / `PortabilityJobFinished` events update them in
 * place (`applyJobEvent`), so a wizard shows progress without polling.
 */
import { useQuery, type QueryClient } from "@tanstack/react-query";
import type { Event as NailEvent, JobStatus } from "../../api/generated";
import { useClient } from "../../state/queries";

/** Query keys of this feature. */
export const portabilityKeys = {
  all: ["portability"] as const,
  jobs: ["portability", "jobs"] as const,
  job: (jobId: string) => ["portability", "job", jobId] as const,
  sources: ["portability", "sources"] as const,
  calendars: ["portability", "calendars"] as const,
  addressBooks: ["portability", "address-books"] as const,
};

/** Running and recently finished jobs, newest first. */
export function useJobs() {
  const client = useClient();
  return useQuery({
    queryKey: portabilityKeys.jobs,
    retry: false,
    queryFn: async () => {
      const answer = await client.request({ type: "ListPortabilityJobs" }, "PortabilityJobs");
      return [...answer.jobs].sort((a, b) => b.started_at.localeCompare(a.started_at));
    },
  });
}

/** One job (events keep it current). */
export function useJob(jobId: string | null) {
  const client = useClient();
  return useQuery({
    queryKey: portabilityKeys.job(jobId ?? ""),
    enabled: jobId !== null,
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "GetPortabilityJob", job_id: jobId ?? "" }, "PortabilityJob"))
        .job,
  });
}

/** Thunderbird profiles found on this computer. */
export function useImportSources() {
  const client = useClient();
  return useQuery({
    queryKey: portabilityKeys.sources,
    retry: false,
    staleTime: Number.POSITIVE_INFINITY,
    queryFn: async () =>
      (await client.request({ type: "FindImportSources" }, "ImportSources")).sources,
  });
}

/** Calendars (targets of imports, sources of exports). */
export function useCalendars() {
  const client = useClient();
  return useQuery({
    queryKey: portabilityKeys.calendars,
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "ListCalendars", source_id: null }, "Calendars")).calendars,
  });
}

/** Address books (targets of imports, sources of exports). */
export function useAddressBooks() {
  const client = useClient();
  return useQuery({
    queryKey: portabilityKeys.addressBooks,
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "ListAddressBooks", source_id: null }, "AddressBooks"))
        .address_books,
  });
}

/** Puts a job the core just answered into the cache. */
export function rememberJob(queryClient: QueryClient, job: JobStatus): void {
  queryClient.setQueryData(portabilityKeys.job(job.job_id), job);
  void queryClient.invalidateQueries({ queryKey: portabilityKeys.jobs });
}

/** Applies a job event to the cached job (progress, the final report). */
export function applyJobEvent(queryClient: QueryClient, event: NailEvent): void {
  if (event.type === "PortabilityJobProgress") {
    queryClient.setQueryData<JobStatus>(portabilityKeys.job(event.job_id), (old) =>
      old && old.state === "running" ? { ...old, progress: event.progress } : old,
    );
  } else if (event.type === "PortabilityJobFinished") {
    const report = event.report;
    queryClient.setQueryData<JobStatus>(portabilityKeys.job(report.job_id), (old) =>
      old ? { ...old, state: report.state, report } : old,
    );
    // A job this window did not start (another window, a restart) is read.
    if (!queryClient.getQueryData(portabilityKeys.job(report.job_id))) {
      void queryClient.invalidateQueries({ queryKey: portabilityKeys.job(report.job_id) });
    }
  }
}
