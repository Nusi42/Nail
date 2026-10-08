/**
 * Accessibility of the M4a plug-in extension UI (axe-core in jsdom):
 * banners and badges, preview panes (tree and image), open action menus,
 * the composer bar, the veto dialog, and the plug-in manager with the
 * activity and confirmation sections open, plus the install dialog's list
 * of contributions. Contrast needs a real layout engine (Playwright).
 */
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import { createEnv, renderWithProviders, type Env, settleQueries } from "../../test/harness";
import {
  AttachmentActionMenu,
  AttachmentPreviewPane,
  ComposerExtensionBar,
  LinkActionMenu,
  PluginBadges,
  PluginBanners,
  PluginMarkdownBlock,
  SendVetoDialog,
} from ".";
import { installDemoExtensions } from "./extensionsSeed";
import { PluginManagerDialog } from "./PluginManager";
import { pluginsStoreFor } from "./store";
import { withAttachments } from "./testUtil";
import { WindowLayer } from "../../layout/WindowLayer";

function setup(): Env {
  const env = createEnv();
  installDemoExtensions(env.transport);
  return env;
}

describe("axe: plug-in extensions", () => {
  it("reader parts (banners, badges, previews, open menus) have no violations", async () => {
    const env = setup();
    const { messageId, pdf, csv } = withAttachments(env);
    const user = userEvent.setup({ delay: null });
    const { container } = renderWithProviders(
      <article>
        <PluginBadges messageId={messageId} />
        <PluginBanners messageId={messageId} />
        <AttachmentPreviewPane messageId={messageId} attachment={pdf} />
        <AttachmentPreviewPane messageId={messageId} attachment={csv} />
        <AttachmentActionMenu messageId={messageId} attachment={csv} />
        <LinkActionMenu url="https://docs.example.org" />
      </article>,
      env,
    );
    await settleQueries(env, 20);
    expect(await violations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Plug-in actions for this link" }));
    expect(screen.getByRole("menu")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("composer parts and the veto dialog have no violations", async () => {
    const env = setup();
    const user = userEvent.setup({ delay: null });
    const { container } = renderWithProviders(
      <>
        <ComposerExtensionBar
          draftId="d1"
          getSelection={() => ({ text: "", start: 0, end: 0 })}
          onInsert={() => undefined}
        />
        <PluginMarkdownBlock info="snippets.ticket" body="id: 1" />
        <SendVetoDialog />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
    );
    await settleQueries(env, 20);
    await user.click(screen.getByRole("button", { name: "Insert block" }));
    expect(await violations(container)).toEqual([]);
    act(() => {
      pluginsStoreFor(env.store)
        .getState()
        .showVeto({
          draftId: "d1",
          vetoId: "veto-1",
          vetoes: [
            {
              plugin_id: "attachment-check",
              plugin_name: "Attachment check",
              reason: "No attachment.",
            },
            { plugin_id: "policy", plugin_name: "Policy", reason: "External recipient." },
          ],
          settle: () => undefined,
        });
    });
    expect(
      await screen.findByRole("dialog", { name: "2 plug-ins stopped this message" }),
    ).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("the manager with activity, confirmations and permissions open has no violations", async () => {
    const env = setup();
    const snippets = env.transport.data.features.plugins.plugins.find((p) => p.id === "snippets");
    if (snippets) snippets.manifest.permissions = ["composer", "tasks.write"];
    const user = userEvent.setup({ delay: null });
    const { container } = renderWithProviders(<PluginManagerDialog />, env);
    await settleQueries(env, 20);
    for (const id of ["digest", "attachment-check"]) {
      const row = container.querySelector<HTMLElement>(`[data-plugin="${id}"]`);
      if (!row) throw new Error(id);
      await user.click(within(row).getByRole("button", { name: "Activity" }));
    }
    const row = container.querySelector<HTMLElement>('[data-plugin="snippets"]');
    if (!row) throw new Error("snippets");
    await user.click(within(row).getByRole("button", { name: "Confirmations" }));
    await settleQueries(env, 20);
    expect(await violations(container)).toEqual([]);
    await user.click(within(row).getByRole("button", { name: "Permissions" }));
    await settleQueries(env, 20);
    expect(await violations(container)).toEqual([]);
  });
});
