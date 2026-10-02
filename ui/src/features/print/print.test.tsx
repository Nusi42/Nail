/**
 * Printing against the mock core: the actions open the print dialog with
 * the selection, Print creates a job and opens the print view (the theme's
 * `print` mode, `PrintViewReady`, the system print dialog, back to the app
 * afterwards), Save as PDF through the desktop host or the browser's
 * fallback, agendas, and axe on the dialog and the view.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useActions } from "../../actions/ActionsProvider";
import { WindowLayer } from "../../layout/WindowLayer";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import {
  PRINT_AGENDA_ACTION,
  PRINT_DIALOG,
  PRINT_MESSAGE_ACTION,
  PRINT_SAVE_PDF_ACTION,
  PRINT_THREAD_ACTION,
  PrintHost,
  closePrintView,
} from ".";
import { PRINT_BASE_CSS } from "./parts";
import {
  defaultAgenda,
  jobIdOfHash,
  occurrenceOf,
  pageName,
  pageRules,
  splitQuoted,
  targetIssue,
} from "./model";
import { pdfName } from "./PrintDialog";

let print: ReturnType<typeof vi.fn>;
beforeEach(() => {
  print = vi.fn(() => window.dispatchEvent(new Event("afterprint")));
  window.print = print as unknown as typeof window.print;
});
afterEach(() => {
  act(() => closePrintView());
  delete document.documentElement.dataset.nailMode;
});

describe("print model", () => {
  it("reads routes, names pages and folds quotes", () => {
    expect(jobIdOfHash("#/print/print-7")).toBe("print-7");
    expect(jobIdOfHash("#/inbox")).toBeNull();
    expect(pageName("letter", "landscape")).toBe("letter-landscape");
    expect(pageRules()).toContain("@page a5-landscape { size: A5 landscape;");
    expect(PRINT_BASE_CSS).toContain("@media print");
    expect(splitQuoted("Hi\n> old\n> older\nBye")).toEqual([
      { quoted: false, text: "Hi" },
      { quoted: true, text: "> old\n> older" },
      { quoted: false, text: "Bye" },
    ]);
    expect(occurrenceOf({ type: "date", date: "2026-10-02" })).toBe("2026-10-02");
    expect(occurrenceOf(null)).toBeNull();
    const agenda = defaultAgenda(new Date(2026, 9, 2));
    expect(agenda).toMatchObject({ start: "2026-10-02", end: "2026-10-08", layout: "list" });
    expect(targetIssue(agenda)).toBeNull();
    expect(pdfName('Re: "Q3" numbers?', "Nail print")).toBe("Re Q3 numbers.pdf");
    expect(pdfName("", "Nail print")).toBe("Nail print.pdf");
  });
});

function Run({ id }: { id: string }) {
  const actions = useActions();
  return (
    <button type="button" onClick={() => void actions.dispatch(id)}>
      {`run ${id}`}
    </button>
  );
}

function selected(): Env {
  const env = createEnv({ state: { activeAccountId: "acc-personal" } });
  const first = env.transport.data.messages.find((m) => m.folder_id === "acc-personal/Inbox");
  act(() => {
    env.store.getState().selectMessage(first?.id ?? null);
    env.store.getState().selectThread(first?.thread_id ?? null);
  });
  return env;
}

function render(env: Env, ids: string[]) {
  return renderWithProviders(
    <>
      {ids.map((id) => (
        <Run key={id} id={id} />
      ))}
      <WindowLayer layout={env.transport.data.layout} />
      <PrintHost />
    </>,
    env,
  );
}

describe("printing", () => {
  it("prints the selected message through the print view", async () => {
    const env = selected();
    const user = userEvent.setup();
    render(env, [PRINT_MESSAGE_ACTION]);
    await user.click(screen.getByRole("button", { name: `run ${PRINT_MESSAGE_ACTION}` }));
    const dialog = await screen.findByRole("dialog", { name: "Print" });
    expect(await within(dialog).findByText(/^Message: /)).toBeVisible();
    await user.selectOptions(within(dialog).getByLabelText("Paper size"), "letter");
    await user.selectOptions(within(dialog).getByLabelText("Headers"), "full");
    await user.click(within(dialog).getByRole("button", { name: "Print" }));
    const job = env.transport.requests.find((r) => r.type === "CreatePrintJob");
    expect(job).toMatchObject({
      target: { type: "message" },
      options: { page_size: "letter", headers: "full" },
    });
    await waitFor(() => expect(print).toHaveBeenCalled());
    expect(env.transport.requests.some((r) => r.type === "PrintViewReady")).toBe(true);
    // The print dialog closed; afterprint closed the view again.
    expect(env.store.getState().windows).not.toContain(PRINT_DIALOG);
    await waitFor(() => expect(jobIdOfHash(window.location.hash)).toBeNull());
  });

  it("shows the print view with the print mode while it is open", async () => {
    const env = selected();
    const id = env.store.getState().selectedMessageId ?? "";
    const created = await env.transport.send({
      type: "CreatePrintJob",
      target: { type: "message", message_id: id },
      options: {
        page_size: "a4",
        orientation: "landscape",
        headers: "basic",
        quoted_text: false,
        attachment_list: true,
        remote_images: false,
        backgrounds: false,
      },
    });
    if (created.type !== "PrintJob") throw new Error(created.type);
    const { container } = render(env, []);
    const before = document.documentElement.dataset.nailMode;
    expect(before).toBe("light");
    act(() => {
      window.location.hash = created.job.view.slice(1);
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    const view = await screen.findByRole("document", { name: "Print preview" });
    expect(view).toHaveAttribute("data-page", "a4-landscape");
    expect(document.documentElement.dataset.nailMode).toBe("print");
    await waitFor(() => expect(view).toHaveAttribute("data-state", "ready"));
    expect(within(view).getByText("From")).toBeVisible();
    expect(print).not.toHaveBeenCalled();
    const result = await axe.run(document.body, {
      rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
    });
    expect(result.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
    await userEvent.setup().click(within(view).getByRole("button", { name: "Close" }));
    await waitFor(() => expect(screen.queryByRole("document")).toBeNull());
    expect(document.documentElement.dataset.nailMode).toBe(before);
    expect(container).toBeTruthy();
  });

  it("prints the whole conversation", async () => {
    const env = selected();
    const user = userEvent.setup();
    render(env, [PRINT_THREAD_ACTION]);
    print.mockImplementation(() => undefined);
    await user.click(screen.getByRole("button", { name: `run ${PRINT_THREAD_ACTION}` }));
    const dialog = await screen.findByRole("dialog", { name: "Print" });
    await user.click(within(dialog).getByRole("button", { name: "Print" }));
    const view = await screen.findByRole("document", { name: "Print preview" });
    expect(view).toHaveAttribute("data-target", "thread");
    await waitFor(() => expect(print).toHaveBeenCalled());
    expect(within(view).getAllByRole("article").length).toBeGreaterThan(0);
  });

  it("saves a PDF through the desktop host", async () => {
    const env = selected();
    env.transport.data.features.print.desktop = true;
    env.transport.data.features.portability.desktop = true;
    env.transport.data.features.portability.pickAnswers = ["C:\\Users\\Ada\\Documents\\mail.pdf"];
    const user = userEvent.setup();
    render(env, [PRINT_SAVE_PDF_ACTION]);
    await user.click(screen.getByRole("button", { name: `run ${PRINT_SAVE_PDF_ACTION}` }));
    const dialog = await screen.findByRole("dialog", { name: "Print" });
    await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
    expect(within(dialog).getByLabelText("PDF file")).toHaveValue(
      "C:\\Users\\Ada\\Documents\\mail.pdf",
    );
    await user.click(within(dialog).getByRole("button", { name: "Save PDF" }));
    expect(
      await within(dialog).findByText(/Saved to C:\\Users\\Ada\\Documents\\mail\.pdf/),
    ).toBeVisible();
    expect(env.transport.data.features.print.pdfs).toHaveLength(1);
    // The Save dialog asked before replacing an existing file.
    expect(env.transport.requests.find((r) => r.type === "SavePrintPdf")).toMatchObject({
      path: "C:\\Users\\Ada\\Documents\\mail.pdf",
      overwrite: true,
    });
    expect(print).not.toHaveBeenCalled();
  });

  it("falls back to the print dialog's PDF printer without a desktop", async () => {
    const env = selected();
    const user = userEvent.setup();
    render(env, [PRINT_SAVE_PDF_ACTION]);
    await user.click(screen.getByRole("button", { name: `run ${PRINT_SAVE_PDF_ACTION}` }));
    const dialog = await screen.findByRole("dialog", { name: "Print" });
    await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
    // No host: the field says to type the path and Browse goes away.
    expect(await within(dialog).findByText(/Type the full path of the file/)).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: "Browse…" })).toBeNull();
    await user.type(within(dialog).getByLabelText("PDF file"), "C:\\x.pdf");
    await user.click(within(dialog).getByRole("button", { name: "Save PDF" }));
    expect(await within(dialog).findByText(/choose “Save as PDF” as the printer/)).toBeVisible();
    expect(within(dialog).queryByRole("button", { name: "Save PDF" })).toBeNull();
  });

  it("prints an agenda and refuses a bad range", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    const { container } = render(env, [PRINT_AGENDA_ACTION]);
    await user.click(screen.getByRole("button", { name: `run ${PRINT_AGENDA_ACTION}` }));
    const dialog = await screen.findByRole("dialog", { name: "Print" });
    const end = within(dialog).getByLabelText("To");
    await user.clear(end);
    await user.type(end, "2020-01-01");
    expect(within(dialog).getByText("The end is before the start.")).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "Print" })).toBeDisabled();
    const result = await axe.run(container, {
      rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
    });
    expect(result.violations.map((v) => `${v.id}: ${v.help}`)).toEqual([]);
    const start = within(dialog).getByLabelText("From");
    await user.clear(start);
    await user.type(start, "2019-12-25");
    await user.selectOptions(within(dialog).getByLabelText("Layout"), "day");
    print.mockImplementation(() => undefined);
    await user.click(within(dialog).getByRole("button", { name: "Print" }));
    const view = await screen.findByRole("document", { name: "Print preview" });
    expect(view).toHaveAttribute("data-target", "agenda");
    await waitFor(() => expect(print).toHaveBeenCalled());
    expect(env.transport.requests).toContainEqual(
      expect.objectContaining({ type: "ListEventInstances" }),
    );
    expect(within(view).getAllByText("No events.").length).toBe(8);
  });

  it("offers an agenda when nothing is selected", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    render(env, [PRINT_MESSAGE_ACTION]);
    act(() => env.store.getState().openWindow(PRINT_DIALOG));
    const dialog = await screen.findByRole("dialog", { name: "Print" });
    expect(within(dialog).getByText(/Select a message or an event first/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Print an agenda instead" }));
    expect(within(dialog).getByText(/^Agenda: /)).toBeVisible();
    expect(screen.getByRole("button", { name: `run ${PRINT_MESSAGE_ACTION}` })).toBeVisible();
  });
});
