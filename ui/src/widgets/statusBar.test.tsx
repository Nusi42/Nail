import { act, screen } from "@testing-library/react";
import { createEnv, renderNode, widget } from "../test/harness";

describe("status-bar sync status", () => {
  it("shows the core's localized reason for a failed synchronization", () => {
    const env = createEnv();
    renderNode(widget("status-bar", { options: { items: "sync-progress" } }), env);
    act(() =>
      env.store.getState().setSyncError({
        accountId: "work",
        code: "network",
        localized: { id: "sync-error-folders", args: { count: 2, folders: "Lists, Receipts" } },
      }),
    );
    expect(screen.getByRole("status", { name: "Status" })).toHaveTextContent(
      "Synchronization failed: 2 folders could not be synchronized: Lists, Receipts",
    );
  });

  it("falls back to the text of the error code", () => {
    const env = createEnv();
    renderNode(widget("status-bar", { options: { items: "sync-progress" } }), env);
    act(() => env.store.getState().setSyncError({ accountId: "work", code: "auth_failed" }));
    expect(screen.getByRole("status", { name: "Status" })).toHaveTextContent(
      /^Synchronization failed: /,
    );
    expect(screen.getByRole("status", { name: "Status" })).not.toHaveTextContent("auth_failed");
  });
});
