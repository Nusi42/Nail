/**
 * The shared dialog pattern (R6, ADR 0239): the footer's order per system,
 * Done, destructive buttons, the confirmation's focus and Enter, the
 * dialog form's Ctrl+Enter and the settings save bar.
 */
import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useRef, useState } from "react";
import { NailError } from "../../api/client";
import type { Os } from "../../api/generated";
import { violations } from "../../test/axe";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { ConfirmDialog } from "./ConfirmDialog";
import { DialogFooter } from "./DialogFooter";
import { DialogForm, isMultilineSubmit } from "./DialogForm";
import { DialogOsContext } from "./dialogOs";
import { footerOrder, osOfNavigator, viewerOs } from "./order";
import { askInWindow, pendingSlot } from "./pending";
import { askQuestion, questionOf, QuestionWindow } from "./QuestionWindow";
import { SettingsSaveBar } from "./SettingsSaveBar";
import {
  discardSettingsDrafts,
  saveSettingsDrafts,
  settingsCanSave,
  settingsDirty,
  SettingsDraftScope,
  useSettingsDraft,
  type SettingsDraft,
} from "./settingsDrafts";

/** `ui` with the providers, dialogs ordered for `os`. */
function renderOn(os: Os, ui: React.ReactElement, env: Env = createEnv()) {
  return {
    env,
    ...renderWithProviders(
      <DialogOsContext.Provider value={os}>{ui}</DialogOsContext.Provider>,
      env,
    ),
  };
}

/** The labels of the footer's end group, left to right. */
const endLabels = (root: HTMLElement) =>
  [...root.querySelectorAll('[data-part="dialog.footer.end"] button')].map((b) => b.textContent);

describe("footer order (R6)", () => {
  it("puts the primary first on Windows and Linux and last on macOS", () => {
    expect(footerOrder("windows")).toEqual(["primary", "secondary", "cancel"]);
    expect(footerOrder("linux")).toEqual(["primary", "secondary", "cancel"]);
    expect(footerOrder("other")).toEqual(["primary", "secondary", "cancel"]);
    expect(footerOrder("macos")).toEqual(["cancel", "secondary", "primary"]);
  });

  it("reads the viewer's system from the browser", () => {
    expect(osOfNavigator({ userAgentData: { platform: "macOS" } })).toBe("macos");
    expect(osOfNavigator({ platform: "MacIntel" })).toBe("macos");
    expect(osOfNavigator({ platform: "Win32" })).toBe("windows");
    expect(osOfNavigator({ userAgentData: { platform: "Windows" }, platform: "" })).toBe("windows");
    expect(osOfNavigator({ platform: "Linux x86_64" })).toBe("linux");
    expect(osOfNavigator({ userAgent: "Mozilla/5.0 (X11; Linux x86_64)" })).toBe("linux");
    // The most precise source decides.
    expect(
      osOfNavigator({
        userAgentData: { platform: "Linux" },
        platform: "Win32",
        userAgent: "Mozilla/5.0 (Windows NT 10.0)",
      }),
    ).toBe("linux");
    expect(osOfNavigator({})).toBe("other");
    expect(osOfNavigator(undefined)).toBe("other");
    // jsdom says nothing of a system the rules know as Windows or macOS.
    expect(["windows", "macos", "linux", "other"]).toContain(viewerOs());
  });
});

describe("DialogFooter (R6)", () => {
  const buttons = (onSave = vi.fn(), onCancel = vi.fn()) => (
    <DialogFooter
      primary={{ label: "Save", onClick: onSave, action: "save" }}
      secondary={{ label: "Save as…" }}
      cancel={{ onClick: onCancel }}
      start={{ label: "Delete…", destructive: true }}
    />
  );

  it("orders the buttons by role for the viewer's system", () => {
    const win = renderOn("windows", buttons());
    expect(endLabels(win.container)).toEqual(["Save", "Save as…", "Cancel"]);
    win.unmount();
    const mac = renderOn("macos", buttons());
    expect(endLabels(mac.container)).toEqual(["Cancel", "Save as…", "Save"]);
    const footer = must(mac.container.querySelector('[data-part="dialog.footer"]'));
    expect(footer).toHaveAttribute("data-os", "macos");
    // Delete… stays alone at the start on every system.
    const start = must(footer.querySelector('[data-part="dialog.footer.start"]'));
    expect(within(start as HTMLElement).getByRole("button")).toHaveTextContent("Delete…");
  });

  it("places several secondary buttons as one group by role (R8)", () => {
    const several = (
      <DialogFooter
        primary={{ label: "Go" }}
        secondary={[
          { label: "Later", action: "later" },
          { label: "Skip", action: "skip" },
        ]}
        cancel={{ onClick: () => undefined }}
      />
    );
    const win = renderOn("windows", several);
    expect(endLabels(win.container)).toEqual(["Go", "Later", "Skip", "Cancel"]);
    expect(
      [...win.container.querySelectorAll('[data-role="secondary"]')].map((b) => b.textContent),
    ).toEqual(["Later", "Skip"]);
    win.unmount();
    const mac = renderOn("macos", several);
    expect(endLabels(mac.container)).toEqual(["Cancel", "Skip", "Later", "Go"]);
  });

  it("marks each button's role and variant; Cancel runs the dialog's close", async () => {
    const user = userEvent.setup();
    const onSave = vi.fn();
    const onCancel = vi.fn();
    const { container } = renderOn("windows", buttons(onSave, onCancel));
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toHaveAttribute("data-role", "primary");
    expect(save).toHaveAttribute("data-variant", "primary");
    expect(save).toHaveAttribute("data-action", "save");
    expect(screen.getByRole("button", { name: "Save as…" })).toHaveAttribute(
      "data-role",
      "secondary",
    );
    const remove = screen.getByRole("button", { name: "Delete…" });
    expect(remove).toHaveAttribute("data-role", "start");
    expect(remove).toHaveAttribute("data-variant", "danger");
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onCancel).toHaveBeenCalledOnce();
    await user.click(save);
    expect(onSave).toHaveBeenCalledOnce();
    expect(await violations(container)).toEqual([]);
  });

  it("never makes a destructive primary the submit button", () => {
    renderOn(
      "windows",
      <form>
        <DialogFooter
          primary={{ label: "Delete folder", destructive: true, submit: true }}
          cancel={{ onClick: () => undefined }}
        />
      </form>,
    );
    const remove = screen.getByRole("button", { name: "Delete folder" });
    expect(remove).toHaveAttribute("type", "button");
    expect(remove).toHaveAttribute("data-variant", "danger");
  });

  it("disables everything but Cancel while busy", () => {
    const { container } = renderOn(
      "windows",
      <DialogFooter
        primary={{ label: "Save", submit: true }}
        cancel={{ onClick: () => undefined }}
        start={{ label: "Delete…" }}
        busy
      />,
    );
    expect(screen.getByRole("button", { name: "Save" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Save" })).toHaveAttribute("type", "submit");
    expect(screen.getByRole("button", { name: "Delete…" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeEnabled();
    expect(container.querySelector('[data-part="dialog.footer"]')).toHaveAttribute(
      "aria-busy",
      "true",
    );
  });

  it("marks a button unavailable but focusable and names what explains it", async () => {
    const user = userEvent.setup();
    const onClick = vi.fn();
    renderOn(
      "windows",
      <>
        <p id="why">Give the rule a name.</p>
        <DialogFooter
          primary={{ label: "Save", onClick, ariaDisabled: true, describedBy: "why" }}
        />
      </>,
    );
    const save = screen.getByRole("button", { name: "Save" });
    expect(save).toBeEnabled();
    expect(save).toHaveAttribute("aria-disabled", "true");
    expect(save).toHaveAccessibleDescription("Give the rule a name.");
    await user.click(save);
    expect(onClick).toHaveBeenCalledOnce();
  });

  it("shows only Done when the task is finished", async () => {
    const user = userEvent.setup();
    const onDone = vi.fn();
    const { container } = renderOn(
      "macos",
      <DialogFooter
        primary={{ label: "Save" }}
        cancel={{ onClick: () => undefined }}
        done={{ onDone }}
      />,
    );
    expect(endLabels(container)).toEqual(["Done"]);
    expect(container.querySelector('[data-part="dialog.footer"]')).toHaveAttribute(
      "data-state",
      "done",
    );
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(onDone).toHaveBeenCalledOnce();
  });
});

describe("ConfirmDialog (R6)", () => {
  it("focuses Cancel for a destructive verb, and Enter never runs it", async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    const { container } = renderOn(
      "windows",
      <ConfirmDialog
        kind="delete-folder"
        risk="destructive"
        question="Delete the folder “Customers”?"
        detail={<p data-part="dialog.note">It is empty.</p>}
        action="Delete folder"
        onConfirm={onConfirm}
        cancel={{ onClick: onCancel }}
      />,
    );
    const cancel = screen.getByRole("button", { name: "Cancel" });
    await waitFor(() => {
      expect(cancel).toHaveFocus();
    });
    const remove = screen.getByRole("button", { name: "Delete folder" });
    expect(remove).toHaveAttribute("data-variant", "danger");
    expect(endLabels(container)).toEqual(["Delete folder", "Cancel"]);
    const group = screen.getByRole("group", { name: "Delete the folder “Customers”?" });
    expect(group).toHaveAttribute("data-risk", "destructive");
    fireEvent.keyDown(group, { key: "Enter" });
    expect(onConfirm).not.toHaveBeenCalled();
    expect(await violations(container)).toEqual([]);
  });

  it("focuses the verb of a normal confirmation and runs it on Enter", async () => {
    const onConfirm = vi.fn();
    renderOn(
      "macos",
      <ConfirmDialog
        kind="move-to-drafts"
        question="Move 3 messages to Drafts?"
        detail={
          <label>
            Note
            <input />
          </label>
        }
        action="Move"
        onConfirm={onConfirm}
        cancel={{ onClick: () => undefined }}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Move" })).toHaveFocus();
    });
    fireEvent.keyDown(screen.getByRole("textbox", { name: "Note" }), { key: "Enter" });
    expect(onConfirm).toHaveBeenCalledOnce();
  });

  it("gives the focus back after a failed attempt and shows the error", async () => {
    function Failing() {
      const [busy, setBusy] = useState(false);
      const [error, setError] = useState<string | null>(null);
      return (
        <ConfirmDialog
          kind="delete-folder"
          risk="destructive"
          question="Delete it?"
          action="Delete folder"
          busy={busy}
          error={error}
          onConfirm={() => {
            setBusy(true);
            queueMicrotask(() => {
              setBusy(false);
              setError("The server refused.");
            });
          }}
          cancel={{ onClick: () => undefined }}
        />
      );
    }
    const user = userEvent.setup();
    renderOn("windows", <Failing />);
    await user.click(screen.getByRole("button", { name: "Delete folder" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The server refused.");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    });
  });

  it("orders a second choice between the verb and Cancel", () => {
    const { container } = renderOn(
      "windows",
      <ConfirmDialog
        kind="unsaved"
        question="Save changes?"
        action="Save"
        onConfirm={() => undefined}
        secondary={{ label: "Don't save", destructive: true }}
        cancel={{ onClick: () => undefined }}
      />,
    );
    expect(endLabels(container)).toEqual(["Save", "Don't save", "Cancel"]);
  });
});

describe("DialogForm (R6)", () => {
  function Form({ onSubmit, disabled = false }: { onSubmit: () => void; disabled?: boolean }) {
    return (
      <DialogForm part="test-dialog" onSubmit={onSubmit}>
        <label>
          Text
          <textarea />
        </label>
        <DialogFooter
          primary={{ label: "Send", submit: true, disabled }}
          cancel={{ onClick: () => undefined }}
        />
      </DialogForm>
    );
  }

  it("submits with Ctrl+Enter in a multi-line field, not with Enter", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderOn("windows", <Form onSubmit={onSubmit} />);
    const text = screen.getByRole("textbox", { name: "Text" });
    await user.click(text);
    await user.keyboard("{Enter}");
    expect(onSubmit).not.toHaveBeenCalled();
    expect(text).toHaveValue("\n");
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(onSubmit).toHaveBeenCalledOnce();
  });

  it("does not submit through a disabled primary", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn();
    renderOn("windows", <Form onSubmit={onSubmit} disabled />);
    await user.click(screen.getByRole("textbox", { name: "Text" }));
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("knows Ctrl+Enter and Cmd+Enter in a text area only", () => {
    const area = document.createElement("textarea");
    const input = document.createElement("input");
    const key = (target: HTMLElement, mods: { ctrlKey?: boolean; metaKey?: boolean }) =>
      ({ key: "Enter", altKey: false, ctrlKey: false, metaKey: false, ...mods, target }) as never;
    expect(isMultilineSubmit(key(area, { ctrlKey: true }))).toBe(true);
    expect(isMultilineSubmit(key(area, { metaKey: true }))).toBe(true);
    expect(isMultilineSubmit(key(area, {}))).toBe(false);
    expect(isMultilineSubmit(key(input, { ctrlKey: true }))).toBe(false);
  });
});

describe("SettingsSaveBar (R6)", () => {
  /** A settings widget with a draft value. */
  function Widget({
    id,
    save,
    canSave = true,
  }: {
    id: string;
    save: SettingsDraft["save"];
    canSave?: boolean;
  }) {
    const [saved, setSaved] = useState("a");
    const [value, setValue] = useState("a");
    useSettingsDraft(id, {
      dirty: value !== saved,
      canSave,
      save: async () => {
        const ok = await save();
        if (ok) setSaved(value);
        return ok;
      },
      discard: () => {
        setValue(saved);
      },
    });
    return (
      <label>
        {id}
        <input
          value={value}
          onChange={(event) => {
            setValue(event.target.value);
          }}
        />
      </label>
    );
  }

  function Page({ saves }: { saves: Record<string, SettingsDraft["save"]> }) {
    return (
      <>
        {Object.entries(saves).map(([id, save]) => (
          <Widget key={id} id={id} save={save} />
        ))}
        <SettingsSaveBar name="Mail" />
      </>
    );
  }

  it("shows while a widget has unsaved changes and saves them all", async () => {
    const user = userEvent.setup();
    const sync = vi.fn(() => true);
    const junk = vi.fn(() => Promise.resolve(true));
    const { env, container } = renderOn("windows", <Page saves={{ sync, junk }} />);
    expect(container.querySelector('[data-part="settings-save-bar"]')).toBeNull();
    await user.type(screen.getByRole("textbox", { name: "sync" }), "b");
    await user.type(screen.getByRole("textbox", { name: "junk" }), "c");
    const bar = await screen.findByRole("group", { name: "Unsaved changes to “Mail”" });
    expect(settingsDirty(env.store)).toBe(true);
    expect(endLabels(bar)).toEqual(["Save", "Discard"]);
    expect(await violations(container)).toEqual([]);
    await user.click(within(bar).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(container.querySelector('[data-part="settings-save-bar"]')).toBeNull();
    });
    expect(sync).toHaveBeenCalledOnce();
    expect(junk).toHaveBeenCalledOnce();
    expect(settingsDirty(env.store)).toBe(false);
  });

  it("keeps the drafts of a draft scope apart from the page's (R8)", async () => {
    const user = userEvent.setup();
    const sync = vi.fn(() => true);
    const plugin = vi.fn(() => true);
    const { env } = renderOn(
      "windows",
      <>
        <Page saves={{ sync }} />
        <SettingsDraftScope>
          <Widget id="plugin" save={plugin} />
          <SettingsSaveBar name="Plug-ins" />
        </SettingsDraftScope>
      </>,
    );
    await user.type(screen.getByRole("textbox", { name: "plugin" }), "b");
    const scoped = await screen.findByRole("group", { name: "Unsaved changes to “Plug-ins”" });
    // The page's bar and drafts do not see the window's change.
    expect(screen.queryByRole("group", { name: "Unsaved changes to “Mail”" })).toBeNull();
    expect(settingsDirty(env.store)).toBe(false);
    await user.type(screen.getByRole("textbox", { name: "sync" }), "c");
    const page = await screen.findByRole("group", { name: "Unsaved changes to “Mail”" });
    await user.click(within(page).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(screen.queryByRole("group", { name: "Unsaved changes to “Mail”" })).toBeNull();
    });
    expect(sync).toHaveBeenCalledOnce();
    expect(plugin).not.toHaveBeenCalled();
    expect(scoped).toBeInTheDocument();
    await user.click(within(scoped).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(plugin).toHaveBeenCalledOnce();
    });
  });

  it("stays while a save fails, and Discard drops the changes", async () => {
    const user = userEvent.setup();
    const failing = vi.fn(() => false);
    const { env } = renderOn("windows", <Page saves={{ vacation: failing }} />);
    const field = screen.getByRole("textbox", { name: "vacation" });
    await user.type(field, "b");
    const bar = await screen.findByRole("group", { name: "Unsaved changes to “Mail”" });
    await user.click(within(bar).getByRole("button", { name: "Save" }));
    expect(failing).toHaveBeenCalledOnce();
    expect(await act(() => saveSettingsDrafts(env.store))).toBe(false);
    expect(screen.getByRole("group", { name: "Unsaved changes to “Mail”" })).toBeInTheDocument();
    await user.click(within(bar).getByRole("button", { name: "Discard" }));
    expect(field).toHaveValue("a");
    await waitFor(() => {
      expect(settingsDirty(env.store)).toBe(false);
    });
  });

  it("disables Save while a change cannot be saved, and forgets unmounted widgets", async () => {
    const env = createEnv();
    const draft: SettingsDraft = {
      dirty: true,
      canSave: false,
      save: () => true,
      discard: vi.fn(),
    };
    function Probe() {
      useSettingsDraft("probe", draft);
      return null;
    }
    const view = renderOn(
      "windows",
      <>
        <Probe />
        <SettingsSaveBar name="Accounts" />
      </>,
      env,
    );
    expect(await screen.findByRole("button", { name: "Save" })).toBeDisabled();
    expect(settingsCanSave(env.store)).toBe(false);
    discardSettingsDrafts(env.store);
    expect(draft.discard).toHaveBeenCalledOnce();
    view.unmount();
    expect(settingsDirty(env.store)).toBe(false);
  });
});

describe("SettingsSaveBar for screen readers (R6 review)", () => {
  /** One draft widget, its save failing while `fail` is set, and the section's title. */
  function Section({ fail }: { fail: () => boolean }) {
    const [saved, setSaved] = useState("a");
    const [value, setValue] = useState("a");
    const title = useRef<HTMLHeadingElement>(null);
    useSettingsDraft("probe", {
      dirty: value !== saved,
      save: () => {
        if (fail()) return false;
        setSaved(value);
        return true;
      },
      discard: () => {
        setValue(saved);
      },
    });
    return (
      <>
        <h2 ref={title} tabIndex={-1}>
          Mail
        </h2>
        <label>
          probe
          <input
            value={value}
            onChange={(event) => {
              setValue(event.target.value);
            }}
          />
        </label>
        <SettingsSaveBar name="Mail" focusAfter={title} />
      </>
    );
  }

  const status = (container: HTMLElement) =>
    must(container.querySelector('[data-part="settings-save-bar.status"]'));

  it("announces unsaved, saved and discarded changes politely", async () => {
    const user = userEvent.setup();
    const { container } = renderOn("windows", <Section fail={() => false} />);
    expect(status(container)).toHaveAttribute("aria-live", "polite");
    expect(status(container)).toHaveTextContent("");
    await user.type(screen.getByRole("textbox", { name: "probe" }), "b");
    expect(status(container)).toHaveTextContent("Unsaved changes to “Mail”");
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(status(container)).toHaveTextContent("Changes saved.");
    });
    await user.type(screen.getByRole("textbox", { name: "probe" }), "c");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    expect(status(container)).toHaveTextContent("Changes discarded.");
  });

  it("moves the focus to the section's title when the bar goes, back to Save when it fails", async () => {
    const user = userEvent.setup();
    let failing = true;
    renderOn("windows", <Section fail={() => failing} />);
    await user.type(screen.getByRole("textbox", { name: "probe" }), "b");
    const save = screen.getByRole("button", { name: "Save" });
    save.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Save" })).toHaveFocus();
    });
    failing = false;
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(screen.getByRole("heading", { name: "Mail" })).toHaveFocus();
    });
    await user.type(screen.getByRole("textbox", { name: "probe" }), "c");
    screen.getByRole("button", { name: "Discard" }).focus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("heading", { name: "Mail" })).toHaveFocus();
  });
});

describe("pending questions (R6 review)", () => {
  it("keep one question per kind and store, and forget only their own", () => {
    const env = createEnv();
    const other = createEnv();
    const slot = pendingSlot<{ id: number }>();
    const first = { id: 1 };
    askInWindow(slot, env.store, "probe-question", first);
    expect(env.store.getState().windows).toContain("probe-question");
    expect(slot.get(env.store)).toBe(first);
    expect(slot.get(other.store)).toBeNull();
    // A newer question stays when the window of the older one goes.
    const second = { id: 2 };
    slot.set(env.store, second);
    slot.forget(env.store, first);
    expect(slot.get(env.store)).toBe(second);
    slot.forget(env.store, second);
    expect(slot.get(env.store)).toBeNull();
  });

  it("a question window runs the verb, keeps the window with the reason when it fails, and forgets on close", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    let fails = true;
    const run = vi.fn(() => {
      if (fails) throw new NailError("conflict", "busy", { id: "common-cancel" });
    });
    askQuestion(env.store, "probe-question", { question: "Drop it?", action: "Drop it", run });
    const close = vi.fn();
    const { unmount } = renderOn(
      "windows",
      <QuestionWindow name="probe-question" close={close} />,
      env,
    );
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    });
    await user.click(screen.getByRole("button", { name: "Drop it" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Cancel");
    expect(close).not.toHaveBeenCalled();
    fails = false;
    await user.click(screen.getByRole("button", { name: "Drop it" }));
    await waitFor(() => {
      expect(close).toHaveBeenCalledOnce();
    });
    unmount();
    expect(questionOf(env.store, "probe-question")).toBeNull();
  });
});
