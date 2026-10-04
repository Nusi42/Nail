/**
 * The mock core's undo histories (M5d, ADR 0131) behave like the core
 * (`crates/nail-core/src/editing.rs` tests): writes are recorded, undo /
 * redo restore them, stale revisions and changes "on disk" are conflicts,
 * and the editors' patches answer `edit-issue-not-ready` until their
 * packages land. The shared hooks follow the events.
 */
import { QueryClient } from "@tanstack/react-query";
import type { EditHistory, Event as NailEvent, LayoutPatch } from "./generated";
import { mockEditHistory, simulateExternalEdit } from "./editHistoryMock";
import { createEnv } from "../test/harness";
import { editKindOf } from "../features/customization/mock";
import { applyEditHistory, editHistoryKey, nextRedo, nextUndo } from "../state/editHistory";

const divider = (first: string): LayoutPatch => ({
  op: "split-sizes",
  node: { window: "mail", breakpoint: 0, path: [] },
  sizes: [first, "1fr", "1.4fr"],
});

function recorder(env: ReturnType<typeof createEnv>): EditHistory[] {
  const seen: EditHistory[] = [];
  env.client.subscribe((event: NailEvent) => {
    if (event.type === "EditHistoryChanged") seen.push(event.history);
  });
  return seen;
}

function sizesOf(env: ReturnType<typeof createEnv>): string[] {
  const root = env.transport.data.layout.windows.mail?.[0]?.root;
  return root?.type === "split" ? root.sizes : [];
}

describe("edit history (M5d)", () => {
  it("records layout writes and undoes / redoes them", async () => {
    const env = createEnv();
    const seen = recorder(env);
    const start = await env.client.request(
      { type: "GetEditHistory", scope: "layout" },
      "EditHistory",
    );
    expect(start.history).toEqual({ scope: "layout", revision: "none", undo: [], redo: [] });
    const original = sizesOf(env);

    await env.client.request(
      { type: "UpdateLayout", patches: [divider("200px")], base_revision: "none" },
      "Done",
    );
    expect(sizesOf(env)[0]).toBe("200px");
    expect(seen.at(-1)?.undo.map((e) => e.kind)).toEqual(["split-sizes"]);
    expect(seen.at(-1)?.undo[0]?.label).toEqual({ id: "edit-kind-split-sizes" });

    // A stale revision changes nothing.
    expect(
      await env.client.call({
        type: "UpdateLayout",
        patches: [divider("300px")],
        base_revision: "none",
      }),
    ).toMatchObject({ type: "Error", code: "conflict", localized: { id: "edit-issue-conflict" } });
    expect(sizesOf(env)[0]).toBe("200px");

    const undone = await env.client.request({ type: "UndoEdit", scope: "layout" }, "EditHistory");
    expect(sizesOf(env)).toEqual(original);
    expect(nextUndo(undone.history)).toBeNull();
    expect(nextRedo(undone.history)?.kind).toBe("split-sizes");
    await env.client.request({ type: "RedoEdit", scope: "layout" }, "EditHistory");
    expect(sizesOf(env)[0]).toBe("200px");
    expect(await env.client.call({ type: "RedoEdit", scope: "layout" })).toMatchObject({
      code: "invalid_request",
      localized: { id: "edit-issue-nothing-to-redo" },
    });
  });

  it("refuses undo after a change on disk unless forced", async () => {
    const env = createEnv();
    await env.client.request({ type: "UpdateLayout", patches: [divider("210px")] }, "Done");
    simulateExternalEdit(env.transport, "layout");
    expect(await env.client.call({ type: "UndoEdit", scope: "layout" })).toMatchObject({
      type: "Error",
      code: "conflict",
    });
    expect(mockEditHistory(env.transport, "layout").undo).toHaveLength(1);
    const forced = await env.client.request(
      { type: "UndoEdit", scope: "layout", force: true },
      "EditHistory",
    );
    expect(forced.history.undo).toHaveLength(0);
  });

  it("records appearance writes in their own history", async () => {
    const env = createEnv();
    await env.client.request({ type: "SetTheme", theme_id: "sage" }, "Themes");
    expect(mockEditHistory(env.transport, "appearance").undo.map((e) => e.kind)).toEqual([
      "set-theme",
    ]);
    expect(mockEditHistory(env.transport, "layout").undo).toHaveLength(0);
    await env.client.request({ type: "UndoEdit", scope: "appearance" }, "EditHistory");
    const themes = await env.client.request({ type: "ListThemes" }, "Themes");
    expect(themes.appearance.settings.theme).not.toBe("sage");
  });

  it("leaves the editors' patches to their packages (not ready in the contract)", async () => {
    const env = createEnv();
    // The layout editor's patches are served (WP181: features/layout-editor/mock.ts).
    for (const patch of [
      { op: "remove-toolbar-item", toolbar: "reader", from: { area: "bar", index: 0 } },
    ] satisfies LayoutPatch[]) {
      expect(await env.client.call({ type: "UpdateLayout", patches: [patch] })).toMatchObject({
        type: "Error",
        code: "unsupported",
        localized: { id: "edit-issue-not-ready" },
      });
    }
    expect(mockEditHistory(env.transport, "layout").undo).toHaveLength(0);
  });

  it("answers the theme editor's requests like the contract's core", async () => {
    const env = createEnv();
    expect(await env.client.call({ type: "GetThemeDetails", theme_id: "nope" })).toMatchObject({
      code: "not_found",
      localized: { id: "edit-issue-theme-not-found" },
    });
    expect(await env.client.call({ type: "DeleteTheme", theme_id: "frost" })).toMatchObject({
      code: "invalid_request",
      localized: { id: "edit-issue-not-editable" },
    });
    expect(
      await env.client.call({ type: "DuplicateTheme", theme_id: "frost", name: "Mine" }),
    ).toMatchObject({ code: "unsupported", localized: { id: "edit-issue-not-ready" } });
  });

  it("names an update's undo entry like LayoutPatch::kind_of", () => {
    expect(editKindOf([])).toBe("layout-edits");
    expect(editKindOf([divider("1px"), divider("2px")])).toBe("split-sizes");
    expect(editKindOf([divider("1px"), { op: "reset-toolbar", toolbar: "reader" }])).toBe(
      "layout-edits",
    );
  });

  it("keeps the cached history current", () => {
    const queryClient = new QueryClient();
    const history: EditHistory = { scope: "appearance", revision: "r1", undo: [], redo: [] };
    applyEditHistory(queryClient, history);
    expect(queryClient.getQueryData(editHistoryKey("appearance"))).toEqual(history);
    expect(nextUndo(undefined)).toBeNull();
  });
});
