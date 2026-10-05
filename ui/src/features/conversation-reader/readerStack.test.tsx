import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { LayoutNode, ResolvedLayout } from "../../api/generated";
import { createMockData } from "../../api/mockData";
import { normalizeNode } from "../../api/normalize";
import { createEnv, renderWithProviders, settleQueries } from "../../test/harness";
import { must } from "../../test/must";
import { UI_STATE_LAYOUT_OFFER } from "../customization/LayoutMigration";
import { ReaderOffer } from "./ReaderOffer";
import {
  UI_STATE_READER_OFFER,
  feedWidget,
  needsReaderMigration,
  readerMigrationPatches,
  stripStacks,
} from "./readerStack";

const w = (
  kind: string,
  options: Record<string, unknown> = {},
  extra: Record<string, unknown> = {},
) => normalizeNode({ type: "widget", kind, id: null, options, ...extra });

/** The pre-P2 reader stack of the default layout. */
function oldReader(): LayoutNode {
  return normalizeNode({
    type: "stack",
    id: "reader",
    sizes: ["auto", "auto", "auto", "auto", "auto", "auto", "auto", "1fr"],
    children: [
      w("toolbar", { toolbar: "reader" }, { id: "reader-toolbar" }),
      w("thread-view", { expand: "none" }, { id: "thread" }),
      w("security-message"),
      w("security-warnings"),
      w("remote-content"),
      w("junk-verdict"),
      w("sync-message-body"),
      w("message-view", { headers: "from to cc date" }, { id: "message" }),
    ],
  });
}

/** Replaces the node with id `reader` in every breakpoint of the mail window. */
function withReader(layout: ResolvedLayout, reader: () => LayoutNode): ResolvedLayout {
  const swap = (node: LayoutNode): LayoutNode => {
    if (node.type === "widget") return node;
    if (node.id === "reader") return reader();
    return { ...node, children: node.children.map(swap) };
  };
  return {
    ...layout,
    windows: {
      ...layout.windows,
      mail: (layout.windows.mail ?? []).map((bp) => ({ ...bp, root: swap(bp.root) })),
    },
  };
}

function oldLayout(): ResolvedLayout {
  return withReader(createMockData(1).layout, oldReader);
}

describe("the reader stack's migration (P2 WP203)", () => {
  it("finds strip-era reader stacks only", () => {
    expect(needsReaderMigration(createMockData(1).layout)).toBe(false);
    const stacks = stripStacks(oldLayout());
    expect(stacks.map((s) => [s.window, s.breakpoint])).toEqual([
      ["mail", 0],
      ["mail", 1],
    ]);
    expect(needsReaderMigration(oldLayout())).toBe(true);
    // A strip without a message-view next to it is left alone.
    const lone = withReader(createMockData(1).layout, () =>
      normalizeNode({ type: "stack", id: "reader", children: [w("thread-view")] }),
    );
    expect(needsReaderMigration(lone)).toBe(false);
  });

  it("puts the feed where the strip was and removes the widgets it now shows", () => {
    // The last stack first (breakpoint 1), so earlier paths stay valid.
    const stack = must(stripStacks(oldLayout())[1]);
    const at = (path: number[]) => ({
      window: "mail",
      breakpoint: 1,
      path: [...stack.path, ...path],
    });
    expect(readerMigrationPatches(oldLayout()).slice(0, 8)).toEqual([
      {
        op: "add-widget",
        widget: { kind: "thread-view", id: "thread", label: null, options: { mode: "cards" } },
        to: { kind: "into", parent: at([]), index: 1 },
      },
      // From the end (the feed sits at 1, so the old ones moved by one).
      ...[8, 7, 6, 5, 4, 3, 2].map((i) => ({ op: "remove-node", node: at([i]) })),
    ]);
    expect(readerMigrationPatches(oldLayout())).toHaveLength(16);
  });

  it("keeps the user's choice and order of per-message widgets", () => {
    const custom = withReader(createMockData(1).layout, () =>
      normalizeNode({
        type: "stack",
        id: "reader",
        children: [
          w("message-view", { "mark-read": false }),
          w("remote-content"),
          w("junk-verdict", {}, { hidden: true }),
          w("thread-view"),
          w("text", { text: "hi" }),
        ],
      }),
    );
    const stack = must(stripStacks(custom)[0]);
    expect(feedWidget(stack)).toEqual({
      kind: "thread-view",
      id: null,
      label: null,
      options: { mode: "cards", card: "message-view remote-content", "mark-read": false },
    });
    const patches = readerMigrationPatches(custom).filter((p) =>
      p.op === "add-widget"
        ? p.to.kind === "into" && p.to.parent.breakpoint === 0
        : p.op === "remove-node" && p.node.breakpoint === 0,
    );
    const first = must(patches[0]);
    expect(first.op === "add-widget" && first.to.kind === "into" && first.to.index).toBe(3);
    expect(
      patches.slice(1, 5).map((p) => (p.op === "remove-node" ? (p.node.path.at(-1) ?? -1) : -1)),
    ).toEqual([4, 2, 1, 0]);
  });

  it("offers the change once and writes it as one edit", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    env.transport.data.layout = oldLayout();
    renderWithProviders(<ReaderOffer />, env, { layout: oldLayout() });
    const offer = await screen.findByRole("region", { name: "Read conversations as cards" });
    await user.click(screen.getByRole("button", { name: "Use cards" }));
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "UpdateLayout",
        patches: readerMigrationPatches(oldLayout()),
      });
    });
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual({
        type: "SetUiState",
        key: UI_STATE_READER_OFFER,
        value: "migrated",
      });
    });
    await waitFor(() => {
      expect(offer).not.toBeInTheDocument();
    });
    await settleQueries(env);
  });

  it("remembers Not now and waits for the M5b layout offer", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    // A pre-rail main window: the M5b offer comes first.
    const preRail: ResolvedLayout = {
      ...oldLayout(),
      windows: { ...oldLayout().windows, main: [{ ...must(oldLayout().windows.mail?.[0]) }] },
    };
    const first = renderWithProviders(<ReaderOffer />, env, { layout: preRail });
    await settleQueries(env);
    expect(screen.queryByRole("region")).toBeNull();
    first.unmount();
    await env.client.request(
      { type: "SetUiState", key: UI_STATE_LAYOUT_OFFER, value: "dismissed" },
      "UiState",
    );
    await env.queryClient.invalidateQueries();
    renderWithProviders(<ReaderOffer />, env, { layout: oldLayout() });
    await user.click(await screen.findByRole("button", { name: "Not now" }));
    await waitFor(() => {
      expect(screen.queryByRole("region")).toBeNull();
    });
    expect(env.transport.requests).toContainEqual({
      type: "SetUiState",
      key: UI_STATE_READER_OFFER,
      value: "dismissed",
    });
    expect(env.transport.requests.some((r) => r.type === "UpdateLayout")).toBe(false);
    await settleQueries(env);
  });
});
