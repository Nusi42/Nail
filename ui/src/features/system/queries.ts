/**
 * Server data of the desktop settings (TanStack Query through
 * `NailClient`). Everything lives under `["system"]`; the core's events
 * keep it fresh (`handleSystemEvent` in `index.ts`).
 */
import { useQuery } from "@tanstack/react-query";
import { useClient } from "../../state/queries";

/** Query keys of this feature. */
export const systemKeys = {
  all: ["system"] as const,
  installation: ["system", "installation"] as const,
  updates: ["system", "updates"] as const,
  desktop: ["system", "desktop"] as const,
  defaultApps: ["system", "default-apps"] as const,
  firstRun: ["system", "first-run"] as const,
};

/** Installation facts (desktop only; `unsupported` elsewhere). */
export function useInstallation(enabled = true) {
  const client = useClient();
  return useQuery({
    queryKey: systemKeys.installation,
    enabled,
    staleTime: Number.POSITIVE_INFINITY,
    // `unsupported` outside the desktop app is final.
    retry: false,
    queryFn: async () => (await client.request({ type: "GetInstallation" }, "Installation")).info,
  });
}

/** Update settings and state (`UpdateStatusChanged` replaces it). */
export function useUpdateStatus() {
  const client = useClient();
  return useQuery({
    queryKey: systemKeys.updates,
    queryFn: async () => (await client.request({ type: "GetUpdateStatus" }, "UpdateStatus")).status,
  });
}

/** Tray / startup settings and the dev endpoint's URL. */
export function useDesktopSettings() {
  const client = useClient();
  return useQuery({
    queryKey: systemKeys.desktop,
    queryFn: async () => {
      const answer = await client.request({ type: "GetDesktopSettings" }, "DesktopSettings");
      return {
        settings: answer.settings,
        devEndpointUrl: answer.dev_endpoint_url,
        startWithWindowsManaged: answer.start_with_windows_managed,
      };
    },
  });
}

/**
 * Default-app state. Windows does not announce changes made in Settings,
 * so it is read again whenever the window gets the focus back.
 */
export function useDefaultApps() {
  const client = useClient();
  return useQuery({
    queryKey: systemKeys.defaultApps,
    refetchOnWindowFocus: "always",
    queryFn: async () => {
      const answer = await client.request({ type: "GetDefaultApps" }, "DefaultApps");
      return { status: answer.status, settingsUri: answer.settings_uri };
    },
  });
}

/** Whether the first-run screen was completed (served in every mode). */
export function useFirstRun(enabled = true) {
  const client = useClient();
  return useQuery({
    queryKey: systemKeys.firstRun,
    enabled,
    queryFn: async () => (await client.request({ type: "GetFirstRun" }, "FirstRun")).state,
  });
}
