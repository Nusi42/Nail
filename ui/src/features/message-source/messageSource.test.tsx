/**
 * ADR 0194: "Show source" — the action opens one window per message, the
 * window shows the original and the decoded view, wraps lines on request
 * and copies the shown view; the decoded view names parts it leaves out.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { MessageBody, MessageSource } from "../../api/generated";
import { WindowLayer } from "../../layout/WindowLayer";
import { appRegistry } from "../registry";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { mockSourceOf } from "./mock";
import { isTruncated, sourceText } from "./model";
import { MESSAGE_SOURCE_KIND, SHOW_SOURCE_ACTION } from "./names";
import { openMessageSource, sourceStoreFor } from "./store";

/** A mock message with an HTML part. */
function htmlBody(env: Env): MessageBody {
  return must([...env.transport.data.bodies.values()].find((b) => b.sanitized_html !== null));
}

function windowOf(id: string): HTMLElement {
  return must(document.querySelector<HTMLElement>(`[data-part="window"][data-window="${id}"]`));
}

const SOURCE: MessageSource = {
  size: 42,
  original: "Subject: Hi\r\n\r\nSGk=\r\n",
  original_truncated: false,
  decoded: [
    { type: "text", text: "Subject: Hi\n\n" },
    { type: "omitted", mime: "image/png", filename: "leaf.png", size: 8 },
    { type: "text", text: "--b--\n" },
  ],
  decoded_truncated: true,
  encrypted: false,
};

describe("message source model", () => {
  it("joins the decoded chunks with one line per part left out", () => {
    const line = (c: { mime: string; filename: string | null }) =>
      `[${c.filename ?? ""} ${c.mime}]`;
    expect(sourceText(SOURCE, "original", line)).toBe(SOURCE.original);
    expect(sourceText(SOURCE, "decoded", line)).toBe(
      "Subject: Hi\n\n[leaf.png image/png]\n--b--\n",
    );
    expect(isTruncated(SOURCE, "original")).toBe(false);
    expect(isTruncated(SOURCE, "decoded")).toBe(true);
  });

  it("writes a mock source from the parsed body", () => {
    const body = htmlBody(createEnv());
    const text = mockSourceOf(body);
    expect(text).toContain(`Subject: ${body.headers.subject}\r\n`);
    expect(text).toContain("Content-Type: multipart/alternative;");
    expect(text).toContain(must(body.sanitized_html));
  });
});

describe("message source window", () => {
  it("registers the action, its palette entry and a window per message", () => {
    expect(appRegistry.actionInfo[SHOW_SOURCE_ACTION]?.title).toBe("source-action-message");
    expect(appRegistry.actionInfo[SHOW_SOURCE_ACTION]?.icon).toBe("file-code");
    expect(appRegistry.windows[MESSAGE_SOURCE_KIND]?.instances).toBe("multiple");
    const env = createEnv();
    const ctx = { store: env.store } as unknown as Parameters<
      NonNullable<(typeof appRegistry.actions)[typeof SHOW_SOURCE_ACTION]>["run"]
    >[0];
    const action = must(appRegistry.actions[SHOW_SOURCE_ACTION]);
    expect(action.enabled?.(ctx, [])).toBe(false);
    act(() => {
      env.store.getState().selectMessage(htmlBody(env).id);
    });
    expect(action.enabled?.(ctx, [])).toBe(true);
    void action.run(ctx, []);
    const [opened] = env.store.getState().windows.filter((w) => w.startsWith(MESSAGE_SOURCE_KIND));
    expect(sourceStoreFor(env.store).getState().messages[must(opened)]).toBe(htmlBody(env).id);
  });

  it("shows the original and the decoded view, wraps and copies", async () => {
    const env = createEnv();
    const user = userEvent.setup({ delay: null });
    const body = htmlBody(env);
    renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
    let id = "";
    act(() => {
      id = openMessageSource(env.store, body.id);
    });
    const win = windowOf(id);
    const text = await within(win).findByLabelText("Source of the message");
    expect(text.textContent).toContain(`Subject: ${body.headers.subject}`);
    expect(text.textContent).toContain("Content-Type: text/html; charset=utf-8");
    const root = must(win.querySelector<HTMLElement>('[data-part="message-source"]'));
    expect(root.dataset.state).toBe("wrapped");
    await user.click(within(win).getByRole("checkbox", { name: "Wrap lines" }));
    expect(root.dataset.state).toBeUndefined();

    await user.click(within(win).getByRole("button", { name: "Decoded" }));
    expect(within(win).getByRole("button", { name: "Decoded" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
    expect(root.dataset.view).toBe("decoded");
    await user.click(within(win).getByRole("button", { name: "Copy all" }));
    await waitFor(() => {
      expect(within(win).getByText("Copied to the clipboard.")).toBeInTheDocument();
    });
    const copied = await navigator.clipboard.readText();
    expect(copied).toContain(`Subject: ${body.headers.subject}\n`);
    expect(copied).not.toContain("\r");

    // The same message raises its window instead of opening a second one.
    act(() => {
      expect(openMessageSource(env.store, body.id)).toBe(id);
    });
    expect(env.store.getState().windows.filter((w) => w.startsWith(MESSAGE_SOURCE_KIND))).toEqual([
      id,
    ]);
  });

  it("says when a window has lost its message", async () => {
    const env = createEnv();
    renderWithProviders(<WindowLayer layout={env.transport.data.layout} />, env);
    act(() => {
      env.store.getState().openWindow(`${MESSAGE_SOURCE_KIND}:w9`);
    });
    expect(
      await screen.findByText(
        "The message of this window is no longer known. Open its source again.",
      ),
    ).toBeInTheDocument();
  });
});
