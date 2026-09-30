import { screen } from "@testing-library/react";
import { QueryClient } from "@tanstack/react-query";
import { createEnv, renderNode, widget } from "../../test/harness";
import { appRegistry } from "../registry";
import { handlePluginsEvent, pluginKeys } from ".";
import { permissionMessageId } from "./messages";

describe("plugins feature slot", () => {
  it("registers the plug-in panel widget", () => {
    expect(appRegistry.widgets["plugin-panel"]).toBeDefined();
  });

  it("renders a placeholder for a panel until plug-ins run", () => {
    const env = createEnv();
    renderNode(widget("plugin-panel", { options: { panel: "follow-up.due" } }), env);
    const panel = screen.getByRole("region", { name: "Plug-in panel" });
    expect(panel.getAttribute("data-part")).toBe("plugin-panel");
    expect(panel.getAttribute("data-panel")).toBe("follow-up.due");
    expect(panel.textContent).toContain("not installed or not enabled");
  });

  it("refetches plug-ins when they change", () => {
    const env = createEnv();
    const queryClient = new QueryClient();
    const spy = vi.spyOn(queryClient, "invalidateQueries");
    handlePluginsEvent({ type: "PluginsChanged" }, { ...env, queryClient });
    expect(spy).toHaveBeenCalledWith({ queryKey: pluginKeys.all });
    handlePluginsEvent({ type: "SnoozeChanged" }, { ...env, queryClient });
    expect(spy).toHaveBeenCalledTimes(1);
  });

  it("names the permission labels like the core", () => {
    expect(permissionMessageId("mail.read.body")).toBe("plugin-permission-mail-read-body");
    expect(permissionMessageId("network:api.example.com")).toBe("plugin-permission-network");
  });
});
