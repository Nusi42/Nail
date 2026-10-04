/**
 * Accessibility of the M4a plug-in extension UI (axe-core in jsdom):
 * banners and badges, preview panes (tree and image), open action menus,
 * the composer bar, the veto dialog, and the plug-in manager with the
 * activity and confirmation sections open, plus the install dialog's list
 * of contributions. Contrast needs a real layout engine (Playwright).
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
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
import { tick } from "./testUtil";
import { WindowLayer } from "../../layout/WindowLayer";

async function settle(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.queryClient.isFetching()).toBe(0);
  });
  await act(() => tick());
}

function setup(): Env {
  const env = createEnv();
  installDemoExtensions(env.transport);
  return env;
}

describe("axe: plug-in extensions", () => {
  it("reader parts (banners, badges, previews, open menus) have no violations", async () => {
    const env = setup();
    const m = env.transport.data.messages.find(
      (x) => (env.transport.data.bodies.get(x.id)?.attachments.length ?? 0) > 1,
    );
    const body = m ? env.transport.data.bodies.get(m.id) : undefined;
    if (!m || !body?.attachments[0] || !body.attachments[1]) throw new Error("no attachments");
    const user = userEvent.setup({ delay: null });
    const { container } = renderWithProviders(
      <article>
        <PluginBadges messageId={m.id} />
        <PluginBanners messageId={m.id} />
        <AttachmentPreviewPane messageId={m.id} attachment={body.attachments[0]} />
        <AttachmentPreviewPane messageId={m.id} attachment={body.attachments[1]} />
        <AttachmentActionMenu messageId={m.id} attachment={body.attachments[1]} />
        <LinkActionMenu url="https://docs.example.org" />
      </article>,
      env,
    );
    await settle(env);
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
    await settle(env);
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
    await settle(env);
    for (const id of ["digest", "attachment-check"]) {
      const row = container.querySelector<HTMLElement>(`[data-plugin="${id}"]`);
      if (!row) throw new Error(id);
      await user.click(within(row).getByRole("button", { name: "Activity" }));
    }
    const row = container.querySelector<HTMLElement>('[data-plugin="snippets"]');
    if (!row) throw new Error("snippets");
    await user.click(within(row).getByRole("button", { name: "Confirmations" }));
    await settle(env);
    expect(await violations(container)).toEqual([]);
    await user.click(within(row).getByRole("button", { name: "Permissions" }));
    await settle(env);
    expect(await violations(container)).toEqual([]);
  });
});
