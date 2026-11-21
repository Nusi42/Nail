/**
 * R2 (ADR 0222): the redesigned rules window — Test into the Preview tab,
 * "Apply to these N", the footer's "Run on" with Run / Save and run and the
 * question before a run that moves mail, Recent runs with time, target and
 * why an older run cannot be undone, unsaved changes, the Editor / Text
 * radio group on an unfinished rule, the empty state's starters and the
 * list filter.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { listSelectionFor, NO_LIST_SCOPE } from "../../actions/mailTargets";
import type { Rule } from "../../api/generated";
import { renderWithProviders, type Env } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { INBOX } from "../../test/mockIds";
import { must } from "../../test/must";
import { guardRulesWindow } from "./closeGuard";
import { RuleManagerView } from "./RuleManager";
import { rulesStore } from "./store";
import { addFromMenu, addTagAllRule, footerOf, runsFinished, setup, showTextView } from "./testing";

/** Renders the window and opens the rule (or "New rule") named `button`. */
async function openEditor(env: Env, button: string) {
  const user = userEvent.setup({ delay: null });
  renderWithProviders(<RuleManagerView />, env);
  await user.click(await screen.findByRole("button", { name: button }));
  const editor = await screen.findByRole("form", { name: "Rule" });
  const footer = footerOf(editor);
  return { user, editor, footer, runOn: within(footer).getByRole("combobox", { name: "Run on" }) };
}

/** A manual rule that moves every message of the Inbox to Archive. */
function addArchiveAllRule(env: Env): Rule {
  const rule: Rule = {
    id: "archive-all",
    name: "Archive all",
    enabled: true,
    triggers: [{ type: "manual" }],
    scope: [],
    conditions: { type: "all", conditions: [] },
    actions: [{ type: "archive" }],
    source: null,
  };
  env.transport.data.features.rules.rules.push(rule);
  return rule;
}

const inInbox = { state: { activeAccountId: "acc-personal", activeFolderId: INBOX } };

describe("the preview", () => {
  it("tests on the Run on target, focuses the summary and explains each match", async () => {
    const env = setup();
    const { user, editor, footer } = await openEditor(env, "Rust announcements");
    const tabs = within(editor).getByRole("tablist", { name: "Preview and recent runs" });
    expect(within(tabs).getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    await user.click(within(tabs).getByRole("tab", { name: /^Recent runs/ }));
    await user.click(within(footer).getByRole("button", { name: "Test rule" }));
    // Test switches to Preview and puts the focus on what it found.
    const results = await within(editor).findByRole("region", { name: "Test results" });
    expect(within(tabs).getByRole("tab", { name: "Preview" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    const summary = within(results).getByRole("status");
    expect(summary).toHaveTextContent(/^2 messages of \d+ match \(Where the rule applies\)\.$/);
    await waitFor(() => {
      expect(summary).toHaveFocus();
    });
    const first = must(results.querySelector<HTMLElement>('[data-part="rules.test.match"]'));
    expect(within(first).getByRole("list", { name: "What would happen" })).toHaveTextContent(
      "Add tag: rust",
    );
    // Why it matched is folded.
    const why = within(first).getByRole("list", { name: "Why it matches", hidden: true });
    expect(why.closest("details")).not.toHaveAttribute("open");
    expect(requestsOf(env, "TestRule")).toMatchObject([{ folder_id: null, limit: 50 }]);

    // A click on the subject opens the message in the main window.
    await user.click(within(first).getByRole("button", { name: /Rust 1\.97 released/ }));
    const opened = env.transport.data.messages.find((m) => m.subject.includes("Rust 1.97"));
    expect(env.store.getState().selectedMessageId).toBe(must(opened).id);
    expect(env.store.getState().activeFolderId).toBe(must(opened).folder_id);

    // An edit makes the preview stale: it goes.
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "!");
    expect(within(editor).queryByRole("region", { name: "Test results" })).toBeNull();
  });

  it("applies the rule to exactly the messages in the preview", async () => {
    const env = setup();
    const { user, editor, footer } = await openEditor(env, "Rust announcements");
    await user.click(within(footer).getByRole("button", { name: "Test rule" }));
    const results = await within(editor).findByRole("region", { name: "Test results" });
    const ids = [...results.querySelectorAll('[data-part="rules.test.match"]')].map(
      (li) => must(li.querySelector("button")).textContent,
    );
    await user.click(within(results).getByRole("button", { name: "Apply to these 2" }));
    await within(footer).findByText(/^The rule ran:/);
    const [request] = requestsOf(env, "RunRules");
    expect(request?.rule_ids).toEqual(["rust-announcements"]);
    expect(request?.message_ids).toHaveLength(2);
    const subjects = must(request).message_ids.map(
      (id) => env.transport.data.messages.find((m) => m.id === id)?.subject,
    );
    expect(subjects).toEqual(ids);
  });
});

describe("Run on and Run in the footer", () => {
  it("runs on where the rule applies, the folder or the selection, and undoes the click", async () => {
    const env = setup(inInbox);
    const [a, b] = env.transport.data.messages.filter((m) => m.folder_id === INBOX);
    listSelectionFor(env.store).getState().setScope(NO_LIST_SCOPE);
    listSelectionFor(env.store)
      .getState()
      .setPicked(
        [must(a), must(b)].map((m) => ({
          key: m.id,
          messageId: m.id,
          threadId: null,
          child: false,
        })),
      );
    env.store.getState().selectMessage(must(a).id);
    addTagAllRule(env);
    const { user, footer, runOn } = await openEditor(env, "Tag all");
    expect(
      within(runOn)
        .getAllByRole("option")
        .map((o) => o.textContent),
    ).toEqual(["Selected messages (2)", "Current folder (Inbox)", "Where the rule applies"]);
    expect(runOn).toHaveValue("scope");

    await user.selectOptions(runOn, "messages");
    await user.click(within(footer).getByRole("button", { name: "Run" }));
    expect(await within(footer).findByRole("status")).toHaveTextContent(
      "The rule ran: 2 messages matched, 2 changes queued.",
    );
    expect(requestsOf(env, "RunRules")).toEqual([
      {
        type: "RunRules",
        rule_ids: ["tag-all"],
        message_ids: [must(a).id, must(b).id],
        folder_id: null,
      },
    ]);
    await runsFinished(env);
    await user.click(within(footer).getByRole("button", { name: "Undo" }));
    expect(await within(footer).findByText("Everything the run did was undone.")).toBeVisible();
    expect(env.transport.data.messages.some((m) => m.tags.includes("picked"))).toBe(false);

    await user.selectOptions(runOn, "folder");
    await user.click(within(footer).getByRole("button", { name: "Run" }));
    await within(footer).findByText(/^The rule ran:/);
    expect(requestsOf(env, "RunRules").at(-1)).toMatchObject({
      message_ids: [],
      folder_id: INBOX,
    });
  });

  it("offers no selection without one and starts on the folder when opened from a message", async () => {
    const env = setup(inInbox);
    const message = must(env.transport.data.messages.find((m) => m.folder_id === INBOX));
    rulesStore(env.client).getState().setOrigin({ messageId: message.id, folderId: INBOX });
    const { runOn } = await openEditor(env, "Rust announcements");
    expect(runOn).toHaveValue("folder");
    const selected = within(runOn).getByRole("option", { name: "Selected messages" });
    expect(selected).toBeDisabled();
    expect(selected).toHaveAttribute("title", "Select messages in the list first.");
  });

  it("offers the mail's folder beside a move action and puts the picked path in", async () => {
    const env = setup(inInbox);
    const message = must(env.transport.data.messages.find((m) => m.folder_id === INBOX));
    rulesStore(env.client).getState().setOrigin({ messageId: message.id, folderId: INBOX });
    const { user, editor } = await openEditor(env, "New rule");
    await addFromMenu(user, editor, "Add action", "Move to folder");
    const action = must(editor.querySelector<HTMLElement>('[data-part="rules.action"]'));
    await user.click(await within(action).findByRole("button", { name: "Values from this mail" }));

    await user.click(await screen.findByRole("menuitem", { name: /Inbox/ }));
    expect(within(action).getByRole("combobox", { name: "Folder path" })).toHaveValue("Inbox");
  });

  it("saves an edited rule before running it, never the old saved one", async () => {
    const env = setup(inInbox);
    const { user, editor, footer, runOn } = await openEditor(env, "Rust announcements");
    expect(within(footer).getByRole("button", { name: "Run" })).toBeEnabled();
    const name = within(editor).getByRole("textbox", { name: "Name" });
    await user.clear(name);
    await user.type(name, "Rust news");
    await user.selectOptions(runOn, "folder");
    await user.click(within(footer).getByRole("button", { name: "Save and run" }));
    await within(footer).findByText(/^The rule ran:/);
    expect(must(requestsOf(env, "SaveRule")[0]).rule.name).toBe("Rust news");
    expect(requestsOf(env, "RunRules")).toEqual([
      { type: "RunRules", rule_ids: ["rust-announcements"], message_ids: [], folder_id: INBOX },
    ]);
    expect(within(footer).getByRole("button", { name: "Run" })).toBeEnabled();
  });

  it("asks before a rule that moves mail runs, with the number of messages", async () => {
    const env = setup(inInbox);
    addArchiveAllRule(env);
    const inboxCount = env.transport.data.messages.filter((m) => m.folder_id === INBOX).length;
    const { user, footer, runOn } = await openEditor(env, "Archive all");
    await user.selectOptions(runOn, "folder");
    const ask = async () => {
      await user.click(within(footer).getByRole("button", { name: "Run" }));
      return screen.findByRole("alertdialog");
    };
    expect(await ask()).toHaveTextContent(
      `“Archive all” moves or deletes mail. It matches ${String(inboxCount)} messages in Current folder (Inbox). Run it now?`,
    );
    await user.click(
      within(screen.getByRole("alertdialog")).getByRole("button", { name: "Cancel" }),
    );
    expect(requestsOf(env, "RunRules")).toEqual([]);
    // Escape is the safe answer, and goes no further.
    const asked = await ask();
    // Tab stays inside the question (modal).
    await waitFor(() => {
      expect(within(asked).getByRole("button", { name: "Cancel" })).toHaveFocus();
    });
    await user.tab();
    expect(within(asked).getByRole("button", { name: "Run" })).toHaveFocus();
    await user.tab();
    expect(within(asked).getByRole("button", { name: "Cancel" })).toHaveFocus();
    await user.tab({ shift: true });
    expect(within(asked).getByRole("button", { name: "Run" })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("alertdialog")).toBeNull();
    expect(requestsOf(env, "RunRules")).toEqual([]);
    // The focus goes back to the button that asked.
    await waitFor(() => {
      expect(within(footer).getByRole("button", { name: "Run" })).toHaveFocus();
    });

    const question = await ask();
    await user.click(within(question).getByRole("button", { name: "Run" }));
    await within(footer).findByText(/^The rule ran:/);
    expect(requestsOf(env, "RunRules")).toMatchObject([{ folder_id: INBOX }]);
  });

  it("counts exactly the selected messages before a run on them moves mail", async () => {
    // A search hit outside the listed folder and one in it: the run acts on
    // both, so the count tests both (not the folder or the Inboxes).
    const search = { text: "from:x", accountId: null, folderId: null, error: null };
    const env = setup({ state: { ...inInbox.state, search } });
    const inbox = must(env.transport.data.messages.find((m) => m.folder_id === INBOX));
    const elsewhere = must(env.transport.data.messages.find((m) => m.folder_id !== INBOX));
    listSelectionFor(env.store).getState().setScope(NO_LIST_SCOPE);
    listSelectionFor(env.store)
      .getState()
      .setPicked(
        [elsewhere, inbox].map((m) => ({
          key: m.id,
          messageId: m.id,
          threadId: null,
          child: false,
        })),
      );
    env.store.getState().selectMessage(inbox.id);
    addArchiveAllRule(env);
    const { user, footer, runOn } = await openEditor(env, "Archive all");
    await user.selectOptions(runOn, "messages");
    await user.click(within(footer).getByRole("button", { name: "Run" }));
    expect(await screen.findByRole("alertdialog")).toHaveTextContent(
      "“Archive all” moves or deletes mail. It matches 2 messages in Selected messages (2). Run it now?",
    );
    expect(requestsOf(env, "TestRule")).toMatchObject([
      { message_ids: [elsewhere.id, inbox.id], folder_id: null },
    ]);
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Run" }));
    await within(footer).findByText(/^The rule ran:/);
    expect(requestsOf(env, "RunRules")).toMatchObject([
      { message_ids: [elsewhere.id, inbox.id], folder_id: null },
    ]);
  });

  it("waits with Run and Save while something is missing", async () => {
    const env = setup(inInbox);
    const { user, editor, footer } = await openEditor(env, "New rule");
    const run = within(footer).getByRole("button", { name: "Save and run" });
    expect(run).toHaveAttribute("aria-disabled", "true");
    await user.click(run);
    expect(requestsOf(env, "RunRules")).toEqual([]);
    expect(requestsOf(env, "SaveRule")).toEqual([]);
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Fresh");
    await user.click(within(editor).getByRole("button", { name: "Add action" }));
    await user.click(within(editor).getByRole("menuitem", { name: "Archive" }));
    expect(run).not.toHaveAttribute("aria-disabled");
  });
});

describe("recent runs", () => {
  it("shows when, why and on what each run acted, and why an older one cannot be undone", async () => {
    const env = setup(inInbox);
    const { user, editor, footer } = await openEditor(env, "Rust announcements");
    await user.click(within(footer).getByRole("button", { name: "Run" }));
    await within(footer).findByText(/^The rule ran: 2 messages/);
    await runsFinished(env);
    const tab = await within(editor).findByRole("tab", { name: "Recent runs (1)" });
    await user.click(tab);
    const runs = within(editor).getByRole("region", { name: "Recent runs" });
    const row = must(runs.querySelector<HTMLElement>('[data-part="rules.run"]'));
    expect(row.querySelector("time")).toHaveAttribute("dateTime");
    expect(row).toHaveTextContent("By hand: 2 messages, done");
    expect(row).toHaveTextContent("where the rule applies");
    await user.click(within(row).getByRole("button", { name: "Undo this run" }));
    await waitFor(() => {
      expect(footer).toHaveTextContent("The run was undone.");
    });
    // Undone: the button stays focusable and says why it does nothing now.
    const again = await within(runs).findByRole("button", { name: "Undo this run" });
    await waitFor(() => {
      expect(again).toHaveAttribute("aria-disabled", "true");
    });
    expect(again).toHaveAccessibleDescription("This run was undone already.");
  });
});

describe("unsaved changes", () => {
  it("asks before another rule opens; Keep editing keeps the changes", async () => {
    const env = setup();
    addTagAllRule(env);
    const { user, editor } = await openEditor(env, "Rust announcements");
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), " 2");
    await user.click(screen.getByRole("button", { name: "Tag all" }));
    const question = await screen.findByRole("alertdialog");
    expect(question).toHaveTextContent("Discard your changes to “Rust announcements 2”?");
    expect(within(question).getByRole("button", { name: "Keep editing" })).toHaveFocus();
    await user.click(within(question).getByRole("button", { name: "Keep editing" }));
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue(
      "Rust announcements 2",
    );

    await user.click(screen.getByRole("button", { name: "Tag all" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Discard changes",
      }),
    );
    expect(
      await within(await screen.findByRole("form", { name: "Rule" })).findByDisplayValue("Tag all"),
    ).toBeInTheDocument();
  });

  it("asks before the window closes (its close button or Escape)", async () => {
    const env = setup();
    // As the app does with the feature's background components.
    const stop = guardRulesWindow(env.store, env.client);
    act(() => {
      env.store.getState().openWindow("rules");
    });
    const { user, editor } = await openEditor(env, "Rust announcements");
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), " 2");
    await act(async () => {
      env.store.getState().closeWindow("rules");
      await Promise.resolve();
    });
    expect(env.store.getState().windows).toContain("rules");
    const question = await screen.findByRole("alertdialog");
    await user.click(within(question).getByRole("button", { name: "Discard changes" }));
    expect(env.store.getState().windows).not.toContain("rules");
    stop();
  });
});

describe("Editor and Text", () => {
  it("is a radio group, and keeps an unfinished new rule across the switch", async () => {
    const env = setup();
    const { user, editor } = await openEditor(env, "New rule");
    const views = within(editor).getByRole("radiogroup", { name: "View" });
    const visual = within(views).getByRole("radio", { name: "Editor" });
    expect(visual).toHaveAttribute("aria-checked", "true");
    // Arrow keys move and switch.
    visual.focus();
    await user.keyboard("{ArrowRight}");
    const source = await within(editor).findByRole("textbox", {
      name: "The rule in rules.kdl syntax",
    });
    expect((source as HTMLTextAreaElement).value).toContain("rule");
    // Unchanged text is not read back.
    await user.click(within(views).getByRole("radio", { name: "Editor" }));
    expect(await within(editor).findByRole("textbox", { name: "Name" })).toHaveValue("");
    expect(requestsOf(env, "RuleFromText")).toEqual([]);

    // Text with an empty name reads back as a draft: the rule stays, the gap is a problem.
    const text = await showTextView(user, editor);
    await user.clear(text);
    await user.paste('rule "" {\n    then {\n        flag\n    }\n}\n');
    await user.click(within(views).getByRole("radio", { name: "Editor" }));
    expect(await within(editor).findByRole("textbox", { name: "Name" })).toHaveValue("");
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveAttribute(
      "aria-invalid",
      "true",
    );
    expect(editor.querySelectorAll('[data-part="rules.action"]')).toHaveLength(1);
    expect(requestsOf(env, "RuleFromText").at(-1)).toMatchObject({ draft: true });
  });
});

describe("the list and the empty state", () => {
  it("starts a rule from a template, and from the selected message only with one", async () => {
    const env = setup();
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    const empty = await screen.findByRole("region", { name: "Start from a template" });
    expect(screen.getByRole("button", { name: "Create rule from message" })).toBeDisabled();
    await user.click(within(empty).getByRole("button", { name: "Archive old read mail" }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue(
      "Archive old read mail",
    );
    expect(within(editor).getByRole("checkbox", { name: "When mail gets old" })).toBeChecked();
    expect(editor).toHaveAttribute("data-new", "true");
  });

  it("filters the list by name", async () => {
    const env = setup();
    addTagAllRule(env);
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    const filter = await screen.findByRole("searchbox", { name: "Find a rule" });
    await screen.findByRole("button", { name: "Tag all" });
    await user.type(filter, "rust");
    expect(screen.queryByRole("button", { name: "Tag all" })).toBeNull();
    expect(screen.getByRole("button", { name: "Rust announcements" })).toBeInTheDocument();
    await user.clear(filter);
    await user.type(filter, "zzz");
    expect(screen.getByText("No rule matches.")).toBeInTheDocument();
  });
});
