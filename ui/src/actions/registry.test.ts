import { QueryClient } from "@tanstack/react-query";
import { TransportClient } from "../api/client";
import { MockTransport } from "../api/mock";
import { defaultLayout } from "../api/mockData";
import { normalizeLayout } from "../api/normalize";
import { createUiStore } from "../state/uiStore";
import { createDispatcher, resolveAction, type ActionContext } from "./registry";
import { expectConsole } from "../test/console";

function context(layout = defaultLayout()): ActionContext & { transport: MockTransport } {
  const transport = new MockTransport();
  return {
    transport,
    client: new TransportClient(transport),
    store: createUiStore({ messageOrder: ["a", "b", "c"], activeAccountId: "acc-work" }),
    queryClient: new QueryClient(),
    layout,
    t: (id) => id,
    locale: "en",
  };
}

describe("actions", () => {
  it("moves the selection with mail.next / mail.previous, clamped at the ends", async () => {
    const ctx = context();
    const { dispatch } = createDispatcher(ctx);
    await dispatch("mail.previous");
    expect(ctx.store.getState().selectedMessageId).toBe("c");
    await dispatch("mail.next");
    expect(ctx.store.getState().selectedMessageId).toBe("c");
    await dispatch("mail.previous");
    await dispatch("mail.previous");
    await dispatch("mail.previous");
    expect(ctx.store.getState().selectedMessageId).toBe("a");
    ctx.store.getState().selectMessage(null);
    await dispatch("mail.next");
    expect(ctx.store.getState().selectedMessageId).toBe("a");
  });

  it("toggles nodes and syncs the active account", async () => {
    const ctx = context();
    const { dispatch } = createDispatcher(ctx);
    await dispatch("view.toggle", ["reader"]);
    expect(ctx.store.getState().hiddenNodes).toEqual(["reader"]);
    await dispatch("sync.now");
    expect(ctx.transport.requests).toContainEqual({ type: "SyncNow", account_id: "acc-work" });
  });

  it("reports availability", () => {
    const ctx = context();
    const d = createDispatcher(ctx);
    expect(d.isEnabled("mail.next")).toBe(true);
    expect(d.isEnabled("view.toggle")).toBe(false);
    expect(d.isEnabled("view.toggle", ["x"])).toBe(true);
    expect(d.isEnabled("mail.reply")).toBe(false);
    ctx.store.getState().setMessageOrder([]);
    expect(d.isEnabled("mail.next")).toBe(false);
  });

  it("follows aliases from the layout, with their arguments", async () => {
    const layout = normalizeLayout({
      actions: {
        "my.hide-reader": { alias_of: "view.toggle", args: ["reader"] },
        "my.loop": { alias_of: "my.loop" },
      },
    });
    expect(resolveAction("my.hide-reader", [], layout).args).toEqual(["reader"]);
    expect(resolveAction("my.loop", [], layout).handler).toBeUndefined();
    const ctx = context(layout);
    await createDispatcher(ctx).dispatch("my.hide-reader");
    expect(ctx.store.getState().hiddenNodes).toEqual(["reader"]);
  });

  it("ignores unknown actions and contains handler failures", async () => {
    const ctx = context();
    const info = vi.spyOn(console, "info").mockReturnValue(undefined);
    expectConsole('nail: action "boom" failed');
    await expect(createDispatcher(ctx).dispatch("no.such")).resolves.toBeUndefined();
    await createDispatcher(ctx, { boom: { run: () => Promise.reject(new Error("x")) } }).dispatch(
      "boom",
    );
    info.mockRestore();
  });
});
