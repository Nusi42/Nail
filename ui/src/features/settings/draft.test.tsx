/**
 * The settings widgets' drafts and questions (R6, ADR 0239): edits wait for
 * the page's Save, Discard takes them back, a failed save keeps them; the
 * questions ask in the shared confirmation window.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { NailError } from "../../api/client";
import { createEnv, type Env } from "../../test/harness";
import { askSettingsQuestion, settingsQuestionOf, type SettingsQuestion } from "./confirm";
import { useSettingsForm } from "./draft";
import {
  confirmSettingsQuestion,
  discardSettings,
  renderSettings,
  saveBar,
  saveSettings,
  saveSettingsDone,
  settingsQuestion,
} from "./testing";
import { ACCOUNT_REMOVE_DIALOG } from "./builtins";

/** What the probe's save does. */
let write: (value: string) => Promise<unknown>;

/** A settings widget with one text field over a stored value. */
function Probe() {
  const [stored, setStored] = useState("saved");
  const [failed, setFailed] = useState(false);
  const [saves, setSaves] = useState(0);
  const form = useSettingsForm(stored, {
    canSave: (value) => value !== "",
    save: async (value) => {
      const result = await write(value);
      if (result !== false) setStored(value);
      return result;
    },
    onError: () => {
      setFailed(true);
    },
    onSaved: () => {
      setSaves((n) => n + 1);
    },
  });
  return (
    <div>
      <label>
        Value
        <input
          value={form.value}
          onChange={(e) => {
            form.set(e.target.value);
          }}
        />
      </label>
      <p data-testid="state">
        {`${form.dirty ? "dirty" : "clean"} ${String(form.revision)} ${String(saves)}${
          failed ? " failed" : ""
        }`}
      </p>
    </div>
  );
}

function setup() {
  const env = createEnv();
  renderSettings(env, <Probe />);
  return { env, user: userEvent.setup(), field: screen.getByRole("textbox", { name: "Value" }) };
}

const state = () => screen.getByTestId("state").textContent;

beforeEach(() => {
  write = vi.fn(() => Promise.resolve());
});

describe("useSettingsForm", () => {
  it("keeps edits until Save, then writes once", async () => {
    const { user, field } = setup();
    expect(saveBar()).toBeNull();
    await user.type(field, "!");
    expect(state()).toBe("dirty 0 0");
    expect(write).not.toHaveBeenCalled();
    await saveSettingsDone(user);
    expect(write).toHaveBeenCalledExactlyOnceWith("saved!");
    expect(field).toHaveValue("saved!");
    expect(state()).toBe("clean 1 1");
  });

  it("is clean again when the edit returns to the saved value", async () => {
    const { user, field } = setup();
    await user.type(field, "!");
    expect(saveBar()).not.toBeNull();
    await user.type(field, "{Backspace}");
    expect(state()).toBe("clean 0 0");
    expect(saveBar()).toBeNull();
  });

  it("discards the edits", async () => {
    const { user, field } = setup();
    await user.type(field, "!");
    await discardSettings(user);
    expect(field).toHaveValue("saved");
    expect(state()).toBe("clean 1 0");
    expect(write).not.toHaveBeenCalled();
  });

  it("keeps the edits and the bar when saving fails", async () => {
    write = vi.fn(() => Promise.reject(new NailError("network", "offline")));
    const { user, field } = setup();
    await user.type(field, "!");
    await saveSettings(user);
    await waitFor(() => {
      expect(state()).toBe("dirty 0 0 failed");
    });
    expect(field).toHaveValue("saved!");
    expect(saveBar()).not.toBeNull();
  });

  it("treats a save that answers false as failed (the widget showed why)", async () => {
    write = vi.fn(() => Promise.resolve(false));
    const { user, field } = setup();
    await user.type(field, "!");
    await saveSettings(user);
    await waitFor(() => {
      expect(write).toHaveBeenCalledOnce();
    });
    expect(state()).toBe("dirty 0 0");
    expect(saveBar()).not.toBeNull();
  });

  it("cannot save an invalid value", async () => {
    const { user, field } = setup();
    await user.clear(field);
    const bar = saveBar();
    expect(bar && within(bar).getByRole("button", { name: "Save" })).toBeDisabled();
  });
});

/** Asks `question` as the account-removal confirmation. */
function ask(env: Env, question: SettingsQuestion) {
  act(() => {
    askSettingsQuestion(env.store, ACCOUNT_REMOVE_DIALOG, question);
  });
}

describe("settings questions", () => {
  const Nothing = () => null;

  it("run the verb, close, and forget the question", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderSettings(env, <Nothing />);
    const run = vi.fn(() => Promise.resolve());
    ask(env, { question: "Remove it?", action: "Remove it", run });
    const question = await settingsQuestion();
    expect(question).toHaveAccessibleName("Remove account");
    expect(within(question).getByRole("button", { name: "Remove it" })).toHaveAttribute(
      "data-variant",
      "danger",
    );
    await confirmSettingsQuestion(user, "Remove it");
    await waitFor(() => {
      expect(env.store.getState().windows).toEqual([]);
    });
    expect(run).toHaveBeenCalledOnce();
    expect(settingsQuestionOf(env.store, ACCOUNT_REMOVE_DIALOG)).toBeNull();
  });

  it("stay open with the error when the verb fails", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderSettings(env, <Nothing />);
    ask(env, {
      question: "Remove it?",
      action: "Remove it",
      run: () => Promise.reject(new NailError("network", "offline")),
    });
    await confirmSettingsQuestion(user, "Remove it");
    const question = await settingsQuestion();
    expect(await within(question).findByRole("alert")).toBeInTheDocument();
    expect(env.store.getState().windows).toEqual([ACCOUNT_REMOVE_DIALOG]);
  });

  it("need the tick of a required check box", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderSettings(env, <Nothing />);
    const run = vi.fn();
    ask(env, {
      question: "Delete it?",
      action: "Delete it",
      confirmCheck: "Also the secret part",
      risk: "normal",
      run,
    });
    const question = await settingsQuestion();
    const verb = within(question).getByRole("button", { name: "Delete it" });
    expect(verb).toBeDisabled();
    await user.click(within(question).getByRole("checkbox", { name: "Also the secret part" }));
    expect(verb).toBeEnabled();
    await user.click(verb);
    expect(run).toHaveBeenCalledOnce();
  });

  it("offer only Cancel when nothing was asked", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderSettings(env, <Nothing />);
    act(() => {
      env.store.getState().openWindow(ACCOUNT_REMOVE_DIALOG);
    });
    const question = await settingsQuestion();
    expect(within(question).getByRole("button", { name: "Remove account" })).toBeDisabled();
    // Esc means Cancel.
    await user.keyboard("{Escape}");
    await waitFor(() => {
      expect(env.store.getState().windows).not.toContain(ACCOUNT_REMOVE_DIALOG);
    });
  });
});
