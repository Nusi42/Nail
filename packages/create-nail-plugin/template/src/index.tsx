/**
 * A starter Nail plug-in: the `hello` command (see manifest.json) opens a
 * dialog and shows a notification with what you typed.
 *
 * Plug-ins run inside Nail in a sandbox (QuickJS): no Node or browser
 * APIs, only `nail.*`, and each capability needs its permission in
 * manifest.json.
 */
import { definePlugin, nail } from "@nail/plugin-api";

/** The plug-in's name, used as the dialog title. */
const TITLE = "__PLUGIN_NAME__";

export default definePlugin({
  activate(ctx) {
    ctx.commands.register("hello", async (command) => {
      const selected = command.selection.messageIds.length;
      const result = await nail.ui.showDialog({
        title: TITLE,
        body: (
          <stack>
            <text>Hello from a Nail plug-in!</text>
            <text style="muted">Messages selected: {selected}</text>
            <input id="greeting" label="Your greeting" default="Hi" required />
          </stack>
        ),
        buttons: [
          { id: "ok", label: "Say it", primary: true },
          { id: "cancel", label: "Cancel" },
        ],
      });
      if (result.button !== "ok") return;
      const greeting = typeof result.values.greeting === "string" ? result.values.greeting : "";
      await nail.ui.notify({ title: greeting, level: "success" });
      nail.log.info("greeted", { selected });
    });
  },
});
