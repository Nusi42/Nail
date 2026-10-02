import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";
import type { Event as NailEvent, JobStatus } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { features } from "../registry";
import type { FeatureEventContext } from "../types";
import { PORTABILITY_EVENTS, handlePortabilityEvent, portabilityKeys } from ".";
import { runtimeIds } from "./messages";
import {
  PORTABILITY_REQUESTS,
  advancePortabilityJobs,
  finishPortabilityJobs,
  isPortabilityRequest,
} from "./mock";
import {
  backupContentsIssues,
  defaultBackupContents,
  defaultDiagnosticsOptions,
  diagnosticsIssues,
  exportAccepts,
  formatBytes,
  passphraseIssues,
  progressFraction,
} from "./model";

function transport(): { t: MockTransport; events: NailEvent[] } {
  const t = new MockTransport();
  t.data.features.portability.autoProgress = false;
  const events: NailEvent[] = [];
  t.onEvent((e) => events.push(e));
  return { t, events };
}

async function job(t: MockTransport, id: string): Promise<JobStatus> {
  const answer = await t.send({ type: "GetPortabilityJob", job_id: id });
  if (answer.type !== "PortabilityJob") throw new Error(answer.type);
  return answer.job;
}

const importOptions = {
  mail: { account_id: null, parent_folder_id: null, folder_name: "Import" },
  folders: null,
  collections: null,
  calendar_id: null,
  address_book_id: null,
  skip_duplicates: true,
  keep_flags: true,
};

describe("portability model", () => {
  it("mirrors the contract's checks", () => {
    expect(defaultBackupContents().items).not.toContain("keys");
    expect(backupContentsIssues({ items: [] })).toEqual(["nothing-chosen"]);
    expect(backupContentsIssues({ items: ["theme", "theme"] })).toEqual(["duplicate-item"]);
    expect(passphraseIssues("short")).toEqual(["passphrase-too-short"]);
    expect(passphraseIssues("long enough pass")).toEqual([]);
    expect(diagnosticsIssues(defaultDiagnosticsOptions())).toEqual([]);
    expect(defaultDiagnosticsOptions().items).not.toContain("mail-content");
    expect(
      diagnosticsIssues({ items: ["mail-headers"], message_ids: [], redact_addresses: true }),
    ).toEqual(["no-messages"]);
    expect(
      diagnosticsIssues({ items: ["logs"], message_ids: ["m"], redact_addresses: true }),
    ).toEqual(["messages-without-mail-item"]);
    expect(exportAccepts("mbox", "folders")).toBe(true);
    expect(exportAccepts("ics", "folders")).toBe(false);
    expect(exportAccepts("vcard", "address-books")).toBe(true);
    expect(
      progressFraction({
        phase: "reading",
        done: 5,
        total: 10,
        bytes_done: 0,
        bytes_total: null,
        current: null,
      }),
    ).toBe(0.5);
    expect(formatBytes(6_400_000, "en")).toBe("6.4 MB");
  });
});

describe("portability mock", () => {
  it("is registered and routes its requests", () => {
    expect(features.portability).toBeDefined();
    expect(PORTABILITY_REQUESTS).toContain("StartImport");
    expect(isPortabilityRequest({ type: "ListPortabilityJobs" })).toBe(true);
    expect(isPortabilityRequest({ type: "Ping" })).toBe(false);
    expect(runtimeIds()).toContain("m4c-not-ready");
    expect(runtimeIds()).toContain("window-import-wizard");
  });

  it("answers the file dialog only with a desktop host", async () => {
    const { t } = transport();
    const pick = {
      kind: "open-folder" as const,
      title_id: "portability-import-pick-title",
      filters: [],
      default_path: null,
    };
    expect(await t.send({ type: "PickPath", pick })).toMatchObject({
      code: "unsupported",
      localized: { id: "desktop-unavailable" },
    });
    t.data.features.portability.desktop = true;
    t.data.features.portability.pickAnswers = ["C:\\Mail", null];
    expect(await t.send({ type: "PickPath", pick })).toEqual({
      type: "PathPicked",
      path: "C:\\Mail",
    });
    expect(await t.send({ type: "PickPath", pick })).toEqual({ type: "PathPicked", path: null });
    expect(await t.send({ type: "FindImportSources" })).toMatchObject({
      sources: [{ source: { format: "thunderbird-profile" }, is_default: true }],
    });
  });

  it("previews a Thunderbird profile with proposals and refuses bad paths", async () => {
    const { t } = transport();
    const answer = await t.send({
      type: "PreviewImport",
      source: { format: "thunderbird-profile", path: "C:\\TB\\profile" },
    });
    if (answer.type !== "ImportPreview") throw new Error(answer.type);
    expect(answer.preview.accounts).toHaveLength(2);
    expect(answer.preview.accounts[0]?.duplicate_of).toBe("acc-personal");
    expect(answer.preview.rules.map((r) => r.complete)).toEqual([true, false]);
    expect(
      await t.send({ type: "PreviewImport", source: { format: "mbox", path: "relative.mbox" } }),
    ).toMatchObject({ code: "invalid_request" });
    t.data.features.portability.missing = ["C:\\gone.mbox"];
    expect(
      await t.send({ type: "PreviewImport", source: { format: "mbox", path: "C:\\gone.mbox" } }),
    ).toMatchObject({ code: "not_found", localized: { id: "portability-path-missing" } });
  });

  it("runs an import job with progress events and a report with issues", async () => {
    const { t, events } = transport();
    const started = await t.send({
      type: "StartImport",
      source: { format: "mbox", path: "C:\\Mail\\archive.mbox" },
      options: importOptions,
    });
    if (started.type !== "PortabilityJob") throw new Error(started.type);
    expect(started.job.state).toBe("running");
    advancePortabilityJobs(t, 3);
    const progress = events.filter((e) => e.type === "PortabilityJobProgress");
    expect(progress.length).toBe(3);
    expect((await job(t, started.job.job_id)).progress.phase).not.toBe("queued");
    finishPortabilityJobs(t);
    const finished = events.find((e) => e.type === "PortabilityJobFinished");
    if (finished?.type !== "PortabilityJobFinished") throw new Error("no report");
    expect(finished.report.state).toBe("completed-with-issues");
    expect(finished.report.issues.map((i) => i.code)).toEqual([
      "malformed-message",
      "duplicate",
      "truncated",
    ]);
    expect(finished.report.counts).toMatchObject({ messages: 118, skipped: 1, duplicates: 1 });
    expect((await job(t, started.job.job_id)).report?.state).toBe("completed-with-issues");
  });

  it("cancels a running job and keeps it listed", async () => {
    const { t } = transport();
    const started = await t.send({
      type: "StartImport",
      source: { format: "maildir", path: "C:\\Maildir" },
      options: importOptions,
    });
    if (started.type !== "PortabilityJob") throw new Error(started.type);
    const cancelled = await t.send({ type: "CancelPortabilityJob", job_id: started.job.job_id });
    expect(cancelled).toMatchObject({ job: { state: "cancelled" } });
    expect(await t.send({ type: "ListPortabilityJobs" })).toMatchObject({
      jobs: [{ state: "cancelled" }],
    });
    expect(await t.send({ type: "GetPortabilityJob", job_id: "nope" })).toMatchObject({
      code: "not_found",
    });
  });

  it("refuses exports the format cannot write and existing targets", async () => {
    const { t } = transport();
    const folder = t.data.folders[0]?.id ?? "";
    const source = { type: "folders" as const, folder_ids: [folder], include_subfolders: false };
    const options = {
      overwrite: false,
      mbox_variant: "mboxrd" as const,
      allow_headers_only: false,
    };
    expect(
      await t.send({ type: "StartExport", source, format: "ics", path: "C:\\x.ics", options }),
    ).toMatchObject({ code: "invalid_request" });
    expect(
      await t.send({ type: "StartExport", source, format: "mbox", path: "C:\\x.mbox", options }),
    ).toMatchObject({ type: "PortabilityJob" });
    expect(
      await t.send({ type: "StartExport", source, format: "mbox", path: "C:\\x.mbox", options }),
    ).toMatchObject({ code: "conflict", localized: { id: "portability-target-exists" } });
    expect(
      await t.send({
        type: "StartExport",
        source,
        format: "mbox",
        path: "C:\\x.mbox",
        options: { ...options, overwrite: true },
      }),
    ).toMatchObject({ type: "PortabilityJob" });
  });

  it("keeps backups behind their passphrase and plans restores", async () => {
    const { t } = transport();
    const path = "C:\\Backups\\nail.nailbackup";
    expect(
      await t.send({
        type: "CreateBackup",
        path,
        contents: { items: ["theme"] },
        passphrase: "short",
      }),
    ).toMatchObject({
      code: "invalid_request",
      localized: { id: "backup-issue-passphrase-too-short" },
    });
    const created = await t.send({
      type: "CreateBackup",
      path,
      contents: defaultBackupContents(),
      passphrase: "correct horse battery",
    });
    expect(created).toMatchObject({ type: "PortabilityJob", job: { kind: "backup" } });
    finishPortabilityJobs(t);
    expect(await t.send({ type: "InspectBackup", path, passphrase: "wrong one!!!" })).toMatchObject(
      {
        code: "auth_failed",
        localized: { id: "backup-wrong-passphrase" },
      },
    );
    const manifest = await t.send({
      type: "InspectBackup",
      path,
      passphrase: "correct horse battery",
    });
    expect(manifest).toMatchObject({ type: "BackupManifest" });
    const plan = await t.send({
      type: "PreviewRestore",
      path,
      passphrase: "correct horse battery",
      contents: { items: ["theme", "accounts", "local-mail"] },
    });
    if (plan.type !== "RestorePlan") throw new Error(plan.type);
    const effects = Object.fromEntries(plan.plan.actions.map((a) => [a.item, a.effect]));
    expect(effects).toMatchObject({ theme: "replace", "local-mail": "merge", settings: "skip" });
    expect(plan.plan.accounts_needing_secrets).toEqual(["acc-personal", "acc-work"]);
    t.data.features.portability.backups[path]?.damaged.push("config/theme.kdl");
    const verify = await t.send({
      type: "VerifyBackup",
      path,
      passphrase: "correct horse battery",
    });
    if (verify.type !== "PortabilityJob") throw new Error(verify.type);
    finishPortabilityJobs(t);
    expect((await job(t, verify.job.job_id)).report).toMatchObject({
      state: "completed-with-issues",
      issues: [{ code: "checksum-mismatch" }],
    });
  });

  it("previews exactly what a diagnostics bundle holds", async () => {
    const { t } = transport();
    const options = defaultDiagnosticsOptions();
    const preview = await t.send({ type: "PreviewDiagnostics", options });
    if (preview.type !== "DiagnosticsPreview") throw new Error(preview.type);
    expect(preview.manifest.entries.every((e) => e.redactions.includes("secrets"))).toBe(true);
    expect(preview.manifest.entries.some((e) => e.item === "mail-content")).toBe(false);
    expect(
      await t.send({
        type: "PreviewDiagnostics",
        options: { ...options, items: ["mail-content"] },
      }),
    ).toMatchObject({
      code: "invalid_request",
      localized: { id: "diagnostics-issue-no-messages" },
    });
    const bundle = await t.send({ type: "CreateDiagnosticsBundle", options, path: "C:\\d.zip" });
    if (bundle.type !== "DiagnosticsBundle") throw new Error(bundle.type);
    expect(bundle.bundle.manifest.entries.map((e) => e.name)).toEqual(
      preview.manifest.entries.map((e) => e.name),
    );
  });

  it("keeps the job cache current from events", () => {
    const queryClient = new QueryClient();
    const spy = vi.spyOn(queryClient, "invalidateQueries");
    const ctx = { queryClient } as unknown as FeatureEventContext;
    queryClient.setQueryData(portabilityKeys.job("j1"), {
      job_id: "j1",
      kind: "import",
      state: "running",
      started_at: "2026-10-02T09:00:00Z",
      path: "C:\\a.mbox",
      progress: {
        phase: "queued",
        done: 0,
        total: null,
        bytes_done: 0,
        bytes_total: null,
        current: null,
      },
      report: null,
    } satisfies JobStatus);
    handlePortabilityEvent(
      {
        type: "PortabilityJobProgress",
        job_id: "j1",
        progress: {
          phase: "reading",
          done: 1,
          total: 2,
          bytes_done: 0,
          bytes_total: null,
          current: null,
        },
      },
      ctx,
    );
    expect(queryClient.getQueryData<JobStatus>(portabilityKeys.job("j1"))?.progress.phase).toBe(
      "reading",
    );
    expect(spy).toHaveBeenCalledWith({ queryKey: portabilityKeys.jobs });
    expect(PORTABILITY_EVENTS).toEqual(["PortabilityJobProgress", "PortabilityJobFinished"]);
    spy.mockClear();
    handlePortabilityEvent({ type: "AccountsChanged" }, ctx);
    expect(spy).not.toHaveBeenCalled();
  });
});
