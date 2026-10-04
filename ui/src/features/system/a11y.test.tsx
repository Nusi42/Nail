/**
 * Accessibility of the desktop settings (axe-core in jsdom, like
 * `src/a11y.test.tsx`): the three widgets in their main states, "About
 * Nail", the first-run screen and the composer notice. Contrast needs a
 * real layout engine (Playwright).
 */
import { act, screen, waitFor } from "@testing-library/react";
import { violations } from "../../test/axe";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { systemStoreFor } from ".";
import { setUpdateState } from "./mock";
import { AboutNail } from "./AboutNail";
import { FirstRun } from "./FirstRun";
import { ProgramDraftNotice } from "./ProgramDraft";

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
  });
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

const update = {
  version: "1.1.0",
  channel: "stable",
  published_at: "2026-09-30T09:00:00Z",
  notes: "## New\n\n- Faster search\n\nMore fixes.",
  size_bytes: 48_000_000,
} as const;

function widgets() {
  return (
    <>
      <NodeView node={widget("update-settings")} path="t/0" />
      <NodeView node={widget("desktop-settings")} path="t/1" />
      <NodeView node={widget("default-apps")} path="t/2" />
    </>
  );
}

describe("axe: desktop settings", () => {
  it("the widgets have no violations (idle, downloading, ready, failed)", async () => {
    const env = createEnv();
    env.transport.data.features.system.autoProgress = false;
    const { container } = renderWithProviders(widgets(), env);
    await settle(env);
    expect(screen.getByRole("button", { name: "Check now" })).toBeVisible();
    expect(await violations(container)).toEqual([]);
    const states = [
      { state: "downloading", update, downloaded_bytes: 12_000_000, total_bytes: 48_000_000 },
      { state: "ready-to-install", update },
      { state: "failed", failure: "network", detail: null, update: null },
    ] as const;
    for (const state of states) {
      setUpdateState(env.transport, state);
      await act(async () => {
        await env.queryClient.invalidateQueries();
      });
      await settle(env);
      expect(await violations(container)).toEqual([]);
    }
  });

  it("the widgets have no violations when policy turns updates off", async () => {
    const env = createEnv();
    Object.assign(env.transport.data.features.system.installation, {
      kind: "msi",
      scope: "per-machine",
      updates_disabled: "msi-managed",
      forced_channel: "stable",
      mapi_installed: false,
    });
    const { container } = renderWithProviders(widgets(), env);
    await settle(env);
    expect(screen.getByText("Your organization installs updates for Nail.")).toBeVisible();
    expect(await violations(container)).toEqual([]);
  });

  it("About Nail, the first-run screen and the composer notice have no violations", async () => {
    const env = createEnv();
    setUpdateState(env.transport, { state: "available", update });
    systemStoreFor(env.store).getState().noteProgramDraft("draft-1", "WINWORD.EXE");
    const { container } = renderWithProviders(
      <>
        <AboutNail />
        <FirstRun close={() => undefined} />
        <ProgramDraftNotice draftId="draft-1" />
      </>,
      env,
    );
    await settle(env);
    expect(screen.getByRole("note")).toBeVisible();
    expect(screen.getByRole("button", { name: "Get started" })).toBeVisible();
    expect(await violations(container)).toEqual([]);
  });

  it("About Nail has no violations outside the desktop app", async () => {
    const env = createEnv();
    env.transport.data.features.system.desktop = false;
    const { container } = renderWithProviders(<AboutNail />, env);
    await settle(env);
    expect(await violations(container)).toEqual([]);
  });
});
