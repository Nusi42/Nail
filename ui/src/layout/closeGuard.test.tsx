/**
 * The window layer's close guard (R6, ADR 0239): ×, Esc and Cancel go
 * through it; a window with unsaved changes stays and the shared prompt
 * asks (Save / Don't save / Cancel, or Discard changes / Keep editing when
 * saving is impossible); leaving inside a window asks the same way.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { THEME_NEW_DIALOG } from "../features/appearance/editorStore";
import { useT } from "../i18n/I18nProvider";
import { violations } from "../test/axe";
import { withCloseGuard } from "../test/closeGuard";
import { createEnv, renderWithProviders, settleQueries, type Env } from "../test/harness";
import { DialogFooter, DialogForm } from "../widgets/dialog";
import { UnsavedPrompt } from "../widgets/dialog/UnsavedPrompt";
import {
  MAIN_GUARD,
  UNSAVED_PROMPT,
  askToLeave,
  closeGuardOf,
  closeWindowNow,
  confirmLeave,
  guardedWindowOf,
  promptWindowFor,
  useCloseGuard,
} from "./closeGuard";
import { DialogRegistryContext, type DialogProps } from "./dialogRegistry";
import { appRegistry } from "../features/registry";
import { WindowTitlesContext } from "./windowSpecs";
import { MAIN_OWNER, WindowLayer, modalOwner } from "./WindowLayer";

/** The test editor's window. */
const EDITOR = "test-editor";

/** What the test editor's Save does (set per test). */
let saveResult: () => Promise<boolean> | boolean = () => true;

/** An editor with one field; valid while the field holds no "!". */
function TestEditor({ close }: DialogProps) {
  const t = useT();
  const [value, setValue] = useState("");
  const [saved, setSaved] = useState("");
  useCloseGuard({
    name: () => "Flag releases",
    isDirty: () => value !== saved,
    canSave: () => !value.includes("!"),
    save: async () => {
      const ok = await saveResult();
      if (ok) setSaved(value);
      return ok;
    },
    discard: () => {
      setValue(saved);
    },
  });
  return (
    <DialogForm part="test-editor" onSubmit={close}>
      <label>
        Rule name
        <input
          value={value}
          onChange={(event) => {
            setValue(event.target.value);
          }}
        />
      </label>
      <DialogFooter
        primary={{ label: t("common-save"), submit: true }}
        cancel={{ onClick: close }}
      />
    </DialogForm>
  );
}

const dialogs = { ...appRegistry.dialogs, [EDITOR]: TestEditor };
const titles = { ...appRegistry.windowTitles, [EDITOR]: () => "Rule editor" };

function setup(windows: string[] = [EDITOR]): Env {
  const env = createEnv();
  withCloseGuard(env);
  env.store.setState({ windows });
  renderWithProviders(
    <DialogRegistryContext.Provider value={dialogs}>
      <WindowTitlesContext.Provider value={titles}>
        <WindowLayer layout={env.transport.data.layout} />
      </WindowTitlesContext.Provider>
    </DialogRegistryContext.Provider>,
    env,
  );
  return env;
}

const editor = () => screen.getByRole("dialog", { name: "Rule editor" });
const prompt = () => screen.getByRole("dialog", { name: "Unsaved changes" });

beforeEach(() => {
  saveResult = () => true;
});

describe("prompt windows (R6)", () => {
  it("name the window they ask about", () => {
    expect(promptWindowFor("rules")).toBe("unsaved-changes:rules");
    expect(promptWindowFor("composer:w1")).toBe("unsaved-changes:composer_w1");
    expect(promptWindowFor("composer:d_42")).toBe("unsaved-changes:composer_d_42");
    expect(promptWindowFor(MAIN_GUARD)).toBe("unsaved-changes:main");
    for (const w of ["rules", "composer:w1", "composer:d_42", MAIN_GUARD]) {
      expect(guardedWindowOf(promptWindowFor(w))).toBe(w);
    }
    expect(guardedWindowOf("composer-close:w1")).toBeNull();
    expect(guardedWindowOf(UNSAVED_PROMPT)).toBeNull();
  });

  it("block the window they ask about, else the main window", () => {
    const isModal = (id: string) => id.startsWith(UNSAVED_PROMPT);
    const open = ["rules", "composer:w1", promptWindowFor("composer:w1")];
    expect(modalOwner(promptWindowFor("composer:w1"), open, isModal, "rules")).toBe("composer:w1");
    expect(modalOwner(promptWindowFor("settings"), open, isModal, "rules")).toBe(MAIN_OWNER);
    expect(modalOwner(promptWindowFor(MAIN_GUARD), open, isModal, null)).toBe(MAIN_OWNER);
  });
});

describe("the close guard (R6)", () => {
  it("closes a window without changes at once", async () => {
    const user = userEvent.setup();
    const env = setup();
    await user.click(within(editor()).getByRole("button", { name: "Cancel" }));
    expect(env.store.getState().windows).toEqual([]);
  });

  it("keeps a changed window open on ×, Esc and Cancel and asks", async () => {
    const user = userEvent.setup();
    const env = setup();
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Releases");
    for (const close of [
      () => user.click(within(editor()).getByRole("button", { name: "Cancel" })),
      () => user.click(within(editor()).getByRole("button", { name: "Close" })),
      () => user.keyboard("{Escape}"),
    ]) {
      await close();
      const question = await screen.findByRole("dialog", { name: "Unsaved changes" });
      expect(env.store.getState().windows).toEqual([EDITOR, promptWindowFor(EDITOR)]);
      expect(within(question).getByText("Save changes to “Flag releases”?")).toBeInTheDocument();
      // Windows order; Enter saves (the verb has the focus); Cancel goes back.
      expect(
        within(question)
          .getAllByRole("button")
          .filter((b) => b.closest('[data-part="dialog.footer.end"]'))
          .map((b) => b.textContent),
      ).toEqual(["Save", "Don't save", "Cancel"]);
      await waitFor(() => {
        expect(within(question).getByRole("button", { name: "Save" })).toHaveFocus();
      });
      await user.click(within(question).getByRole("button", { name: "Cancel" }));
      await waitFor(() => {
        expect(env.store.getState().windows).toEqual([EDITOR]);
      });
      expect(within(editor()).getByRole("textbox", { name: "Rule name" })).toHaveValue("Releases");
    }
  });

  it("closes after Save, and stays open when Save fails", async () => {
    const user = userEvent.setup();
    const env = setup();
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Releases");
    saveResult = () => false;
    await user.click(within(editor()).getByRole("button", { name: "Cancel" }));
    await user.click(within(prompt()).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([EDITOR]);
    });
    saveResult = () => Promise.resolve(true);
    await user.click(within(editor()).getByRole("button", { name: "Cancel" }));
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
  });

  it("drops the changes and closes on Don't save", async () => {
    const user = userEvent.setup();
    const env = setup();
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Releases");
    await user.keyboard("{Escape}");
    const question = await screen.findByRole("dialog", { name: "Unsaved changes" });
    expect(await violations(question)).toEqual([]);
    await user.click(within(question).getByRole("button", { name: "Don't save" }));
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(closeGuardOf(env.store, EDITOR)).toBeNull();
  });

  it("offers only Discard changes and Keep editing when saving is impossible", async () => {
    const user = userEvent.setup();
    const env = setup();
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Oops!");
    await user.keyboard("{Escape}");
    const question = await screen.findByRole("dialog", { name: "Unsaved changes" });
    expect(within(question).getByText("Discard changes to “Flag releases”?")).toBeInTheDocument();
    expect(within(question).queryByRole("button", { name: "Save" })).toBeNull();
    const keep = within(question).getByRole("button", { name: "Keep editing" });
    await waitFor(() => {
      expect(keep).toHaveFocus();
    });
    // Enter on the focused safe button keeps editing.
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([EDITOR]);
    });
    await user.keyboard("{Escape}");
    await user.click(
      within(await screen.findByRole("dialog", { name: "Unsaved changes" })).getByRole("button", {
        name: "Discard changes",
      }),
    );
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
  });

  it("asks before moving on inside a window, and closes without asking when told", async () => {
    const user = userEvent.setup();
    const env = setup();
    const moved = vi.fn();
    await act(async () => {
      askToLeave(env.store, EDITOR, moved);
      await Promise.resolve();
    });
    expect(moved).toHaveBeenCalledOnce();
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Releases");
    await act(async () => {
      askToLeave(env.store, EDITOR, moved);
      await Promise.resolve();
    });
    await user.click(
      within(await screen.findByRole("dialog", { name: "Unsaved changes" })).getByRole("button", {
        name: "Don't save",
      }),
    );
    expect(moved).toHaveBeenCalledTimes(2);
    // The editor stays (moving on is not closing) with its saved value back.
    expect(env.store.getState().windows).toEqual([EDITOR]);
    expect(within(editor()).getByRole("textbox", { name: "Rule name" })).toHaveValue("");
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Again");
    await act(async () => {
      closeWindowNow(env.store, EDITOR);
      await Promise.resolve();
    });
    expect(env.store.getState().windows).toEqual([]);
  });

  it("guards New theme: a typed name asks before it is lost", async () => {
    const user = userEvent.setup();
    const env = setup([THEME_NEW_DIALOG]);
    await settleQueries(env);
    const dialog = screen.getByRole("dialog", { name: "New theme" });
    await user.type(within(dialog).getByRole("textbox", { name: "Name" }), "Deep Sea");
    await user.click(within(dialog).getByRole("button", { name: "Close" }));
    const question = await screen.findByRole("dialog", { name: "Unsaved changes" });
    expect(within(question).getByText("Save changes to “Deep Sea”?")).toBeInTheDocument();
    await user.click(within(question).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(env.store.getState().windows).not.toContain(THEME_NEW_DIALOG);
    });
    expect(env.transport.data.features.appearance.themes.some((t) => t.id === "deep-sea")).toBe(
      true,
    );
    await settleQueries(env);
  });
});

describe("the prompt's answers (R6 review)", () => {
  it("Cancel while Save runs keeps the window, also once the save ends", async () => {
    const user = userEvent.setup();
    const env = setup();
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Releases");
    let finish: (ok: boolean) => void = () => undefined;
    saveResult = () =>
      new Promise<boolean>((resolve) => {
        finish = resolve;
      });
    await user.keyboard("{Escape}");
    await user.click(within(prompt()).getByRole("button", { name: "Save" }));
    await user.click(within(prompt()).getByRole("button", { name: "Cancel" }));
    await act(async () => {
      finish(true);
      await Promise.resolve();
    });
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([EDITOR]);
    });
  });

  it("confirmLeave resolves at once without changes, else with the prompt's answer", async () => {
    const user = userEvent.setup();
    const env = setup();
    await expect(confirmLeave(env.store, EDITOR)).resolves.toBe(true);
    await user.type(within(editor()).getByRole("textbox", { name: "Rule name" }), "Releases");
    const answers: boolean[] = [];
    const ask = async () => {
      await act(async () => {
        void confirmLeave(env.store, EDITOR).then((ok) => answers.push(ok));
        await Promise.resolve();
      });
      return screen.findByRole("dialog", { name: "Unsaved changes" });
    };
    await user.click(within(await ask()).getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(answers).toEqual([false]);
    });
    await user.click(within(await ask()).getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(answers).toEqual([false, true]);
    });
    // Moving out was the caller's business: the window is still open.
    expect(env.store.getState().windows).toEqual([EDITOR]);
  });
});

describe("the prompt without a question (R6)", () => {
  it("says so and offers Done", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const close = vi.fn();
    renderWithProviders(<UnsavedPrompt name={promptWindowFor(EDITOR)} close={close} />, env);
    expect(screen.getByText("There are no unsaved changes any more.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(close).toHaveBeenCalledOnce();
  });
});
