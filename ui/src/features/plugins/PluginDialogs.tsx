/**
 * Plug-in dialogs (`nail.ui.showDialog`, ADR 0050): the core announces a
 * dialog with `PluginDialogOpened`; this UI shows the ones meant for it
 * (its `client_id` or `null`) and answers with `RespondPluginDialog` — the
 * button and the values of the dialog's inputs. Escape and the close
 * button dismiss it (`button: null`).
 *
 * - Every dialog is a window of the window manager (M5c, `./windows.ts`):
 *   modal dialogs (the default) are confirmations that block the flow they
 *   came from; non-modal ones float like any tool window. The window's
 *   title is the dialog's; closing the window (× or Escape) dismisses it.
 * - A primary button (also Enter in a field) needs every `required` value
 *   and valid formats; invalid fields are marked, described and focused.
 * - `button` nodes inside the dialog send `PluginUiAction` with the
 *   current values; the plug-in may answer with an updated dialog (same id).
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { errorMessageOf, NailError } from "../../api/client";
import type { JsonValue, LocalizedMessage, PluginDialog } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { clientIdOf } from "./context";
import { partAttrs } from "./parts";
import { usePlugins } from "./queries";
import { usePluginsState } from "./store";
import { pluginDialogWindow, isPluginDialogWindow, setPluginWindowTitle } from "./windows";
import { PluginTree } from "./tree/PluginTree";
import { mergeValues, validate, type FieldIssue, type Values } from "./tree/values";

const FOCUSABLE =
  'input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not([disabled]), a[href], [tabindex="0"]';

/** The plug-in's display name (its id until the list is loaded). */
export function usePluginName(pluginId: string): string {
  const plugins = usePlugins();
  return plugins.data?.find((p) => p.id === pluginId)?.manifest.name ?? pluginId;
}

function DialogView({ dialog }: { dialog: PluginDialog }) {
  const t = useT();
  const client = useClient();
  const closeDialog = usePluginsState((s) => s.closeDialog);
  const pluginName = usePluginName(dialog.plugin_id);
  const ref = useRef<HTMLDivElement>(null);
  const { spec } = dialog;
  const modal = spec.modal !== false;
  const plugin = dialog.plugin_id;
  const [values, setValues] = useState<Values>(() => mergeValues(spec.body, {}, new Date()));
  const [issues, setIssues] = useState<Record<string, FieldIssue>>({});
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const [lastBody, setLastBody] = useState(spec.body);

  // An updated dialog (same id) keeps what the user entered.
  if (lastBody !== spec.body) {
    setLastBody(spec.body);
    setValues((previous) => mergeValues(spec.body, previous, new Date()));
    setIssues({});
  }

  // Focus the first field on open; give focus back on close.
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const body = ref.current?.querySelector('[data-part="plugin.dialog.body"]');
    const first = body?.querySelector<HTMLElement>(FOCUSABLE) ?? ref.current;
    first?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);

  const respond = async (button: string | null) => {
    setBusy(true);
    setProblem(null);
    try {
      await client.request(
        { type: "RespondPluginDialog", dialog_id: dialog.dialog_id, button, values },
        "Done",
      );
      closeDialog(dialog.dialog_id);
    } catch (error) {
      // The dialog is gone in the core (plug-in stopped): nothing to answer.
      if (error instanceof NailError && error.code === "not_found") {
        closeDialog(dialog.dialog_id);
        return;
      }
      setProblem(errorMessageOf(error));
      setBusy(false);
    }
  };

  const submit = (button: string, primary: boolean) => {
    if (primary) {
      const found = validate(spec.body, values);
      setIssues(found);
      if (Object.keys(found).length > 0) {
        // Focus the first invalid field once it is marked.
        queueMicrotask(() =>
          ref.current?.querySelector<HTMLElement>('[aria-invalid="true"]')?.focus(),
        );
        return;
      }
    }
    void respond(button);
  };

  const action = async (componentId: string) => {
    setProblem(null);
    try {
      await client.request(
        {
          type: "PluginUiAction",
          target: { kind: "dialog", dialog_id: dialog.dialog_id },
          component_id: componentId,
          values,
          client_id: clientIdOf(client),
        },
        "Done",
      );
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };

  const change = (id: string, value: JsonValue) => {
    setValues((previous) => ({ ...previous, [id]: value }));
    setIssues((previous) => {
      if (!(id in previous)) return previous;
      return Object.fromEntries(Object.entries(previous).filter(([key]) => key !== id));
    });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === "Escape" && !event.defaultPrevented) {
      event.preventDefault();
      event.stopPropagation();
      if (!busy) void respond(null);
      return;
    }
  };

  const buttons = spec.buttons ?? [];
  const primary = buttons.find((b) => b.primary);
  const invalidCount = Object.keys(issues).length;

  const frame = (
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- the dialog's keys (Enter, Escape) arrive from the controls inside it.
    <div
      ref={ref}
      {...partAttrs("plugin.dialog", plugin)}
      data-dialog={dialog.dialog_id}
      data-size={spec.size ?? "medium"}
      data-modal={modal ? "" : undefined}
      data-state={invalidCount > 0 ? "invalid" : undefined}
      data-key-context="plugin-dialog"
      aria-busy={busy}
      onKeyDown={onKeyDown}
    >
      <div {...partAttrs("plugin.dialog.header", plugin)}>
        <span {...partAttrs("plugin.dialog.plugin", plugin)}>
          {t("plugin-dialog-from", { plugin: pluginName })}
        </span>
      </div>
      <form
        {...partAttrs("plugin.dialog.body", plugin)}
        noValidate
        onSubmit={(event) => {
          event.preventDefault();
          if (primary && !busy) submit(primary.id, true);
        }}
      >
        {invalidCount > 0 && (
          <p {...partAttrs("plugin.dialog.error", plugin)} role="alert">
            {t("plugin-dialog-invalid", { count: invalidCount })}
          </p>
        )}
        {problem && (
          <p {...partAttrs("plugin.dialog.error", plugin)} role="alert">
            {t(problem.id, problem.args)}
          </p>
        )}
        <PluginTree
          plugin={plugin}
          nodes={spec.body}
          values={values}
          issues={issues}
          busy={busy}
          onChange={change}
          onAction={(id) => void action(id)}
        />
        {buttons.length > 0 && (
          <div {...partAttrs("plugin.dialog.buttons", plugin)}>
            {buttons.map((button) => (
              <button
                key={button.id}
                type={button.primary ? "submit" : "button"}
                {...partAttrs("plugin.dialog.button", plugin, button.id)}
                data-variant={button.primary ? "primary" : "default"}
                disabled={busy}
                onClick={
                  button.primary
                    ? (event) => {
                        event.preventDefault();
                        submit(button.id, true);
                      }
                    : () => {
                        submit(button.id, false);
                      }
                }
              >
                {button.label}
              </button>
            ))}
          </div>
        )}
      </form>
    </div>
  );
  return frame;
}

/** The content of a plug-in dialog's window (`plugin-dialog-*` / `plugin-modal-*`). */
export function PluginDialogWindow({ name }: DialogProps) {
  const dialog = usePluginsState((s) => s.dialogs.find((d) => pluginDialogWindow(d) === name));
  return dialog ? <DialogView key={dialog.dialog_id} dialog={dialog} /> : null;
}

/**
 * Keeps the plug-in dialogs of this UI and their windows in step (an
 * effect in the app shell): a new dialog opens its window (titled by the
 * dialog), a dialog the core or the plug-in closed closes it, and a window
 * the user closed (× or Escape) dismisses its dialog (`button: null`).
 */
export function PluginDialogs() {
  const client = useClient();
  const store = useUiStoreApi();
  const dialogs = usePluginsState((s) => s.dialogs);
  const closeDialog = usePluginsState((s) => s.closeDialog);
  const windows = useUi((s) => s.windows);
  const opened = useRef(new Set<string>());

  // Dialogs → windows.
  useEffect(() => {
    const wanted = new Set(dialogs.map(pluginDialogWindow));
    for (const dialog of dialogs) {
      const id = pluginDialogWindow(dialog);
      setPluginWindowTitle(id, dialog.spec.title);
      if (!opened.current.has(id)) {
        opened.current.add(id);
        store.getState().openWindow(id);
      }
    }
    for (const id of [...opened.current]) {
      if (wanted.has(id)) continue;
      opened.current.delete(id);
      setPluginWindowTitle(id, null);
      if (store.getState().windows.includes(id)) store.getState().closeWindow(id);
    }
  }, [dialogs, store]);

  // A window the user closed: dismiss its dialog.
  useEffect(() => {
    for (const dialog of dialogs) {
      const id = pluginDialogWindow(dialog);
      // The store's current windows: this render's may predate the opening.
      const open = store.getState().windows.includes(id);
      if (!opened.current.has(id) || open || !isPluginDialogWindow(id)) continue;
      opened.current.delete(id);
      setPluginWindowTitle(id, null);
      closeDialog(dialog.dialog_id);
      void client
        .call({
          type: "RespondPluginDialog",
          dialog_id: dialog.dialog_id,
          button: null,
          values: {},
        })
        .catch(() => undefined);
    }
  }, [windows, dialogs, client, closeDialog, store]);

  return null;
}
