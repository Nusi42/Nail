/**
 * R2 contract (ADR 0222): the shared picker's interface and the values it
 * asks the core for (answered by the mock core).
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { rankValues } from "../../api/mockValues";
import { createEnv } from "../../test/harness";
import { Picker, PICKER_KINDS, suggestKindOf, suggestValues, usePickerOptions } from ".";

const LABEL = "Folder";

describe("shared pickers (contract)", () => {
  it("is a text field that takes free text", () => {
    const changes: [string, unknown][] = [];
    render(
      <Picker
        kind="folder"
        value=""
        label={LABEL}
        invalid
        describedBy="problems"
        onChange={(value, option) => changes.push([value, option])}
      />,
    );
    const input = screen.getByRole("textbox", { name: LABEL });
    expect(input).toHaveAttribute("data-part", "picker.input");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAttribute("aria-describedby", "problems");
    expect(input.closest('[data-part="picker"]')).toHaveAttribute("data-kind", "folder");
    fireEvent.change(input, { target: { value: "Archive/2026" } });
    expect(changes).toEqual([["Archive/2026", null]]);
  });

  it("maps each kind to its source", () => {
    expect(PICKER_KINDS.map(suggestKindOf)).toEqual([
      null,
      null,
      "tag",
      null,
      "mailing-list",
      "header-name",
    ]);
    const scope = { accountId: null, folderValue: "path" } as const;
    expect(usePickerOptions({ kind: "tag", query: "", scope })).toEqual({
      options: [],
      loading: false,
    });
  });

  it("asks the core for tags and header names", async () => {
    const env = createEnv();
    const [message] = env.transport.data.messages;
    if (message) message.tags = ["project"];
    const tags = await suggestValues(env.client, "tag", "PRO");
    expect(tags.map((v) => v.value)).toContain("project");
    const headers = await suggestValues(env.client, "header-name", "subj");
    expect(headers.map((v) => v.value)).toEqual(["Subject"]);
    expect(await suggestValues(env.client, "mailing-list", "")).toEqual([]);
  });

  it("ranks values by use, case-insensitively", () => {
    expect(rankValues(["Work", "home", "work", "x"], "", 2)).toEqual([
      { value: "Work", label: null, count: 2 },
      { value: "home", label: null, count: 1 },
    ]);
    expect(rankValues(["Work", "home"], "WO", 10).map((v) => v.value)).toEqual(["Work"]);
  });
});
