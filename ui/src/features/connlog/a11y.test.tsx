/**
 * ADR 0190: the connection log viewer, the error details window, the
 * application log window and the settings parts are axe-clean in every
 * built-in theme (light, dark, High contrast) — with a warning line and the
 * error's entry highlighted — and the skin compiles without warnings.
 */
import { act, waitFor } from "@testing-library/react";
import { defaultTheme } from "../../api/mockData";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { violations } from "../../test/axe";
import { createEnv, renderWithProviders, settleQueries, widget } from "../../test/harness";
import { must } from "../../test/must";
import { compileTheme } from "../../theme/compile";
import { BUILTIN_THEMES } from "../appearance";
import { resolveMockTheme } from "../appearance/themes";
import { failSyncWith, selectFailure } from "./mock";
import { CONNLOG_SKIN_CSS } from "./skin";
import { openConnectionLog, openErrorDetails } from "./store";

const THEMES = BUILTIN_THEMES.map(([id]) => id);

describe("connection log accessibility (ADR 0190)", () => {
  it("the skin is token-only", () => {
    expect(CONNLOG_SKIN_CSS).not.toMatch(/#[0-9a-f]{3,8}\b|rgb\(/i);
  });

  it.each(THEMES)(
    "%s: the viewer, details, application log and settings have no violations",
    async (id) => {
      const theme = resolveMockTheme(defaultTheme(), id, {
        density: "comfortable",
        overrides: {},
      });
      expect(compileTheme(theme).warnings).toEqual([]);
      const env = createEnv();
      failSyncWith(env.transport, "acc-work", "Archive");
      await env.client.request({ type: "SyncNow", account_id: "acc-work" }, "SyncStarted");
      await waitFor(() => {
        expect(env.transport.data.features.connlog.errors).toHaveLength(1);
      });
      const details = must(env.transport.data.features.connlog.errors[0]).details;
      act(() => {
        openConnectionLog(env.store, { kind: "account", account_id: "acc-work" }, details.log_seq);
        openErrorDetails(env.store, {
          accountId: "acc-work",
          code: details.code,
          localized: null,
          details: { ...selectFailure("Archive", null), ...details },
        });
        env.store.getState().openWindow("application-log");
      });
      const { container, unmount } = renderWithProviders(
        <>
          <NodeView node={widget("account-settings")} path="a" />
          <NodeView node={widget("diagnostics")} path="d" />
          <WindowLayer layout={env.transport.data.layout} />
        </>,
        env,
        { theme },
      );
      await waitFor(() => {
        expect(container.ownerDocument.querySelector("[data-highlight]")).not.toBeNull();
        expect(container.ownerDocument.querySelector('[data-level="warning"]')).not.toBeNull();
      });
      await settleQueries(env);
      expect(await violations(container.ownerDocument.body)).toEqual([]);
      unmount();
    },
    120_000,
  );
});
