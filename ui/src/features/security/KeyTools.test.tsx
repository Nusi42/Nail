import { screen, within } from "@testing-library/react";
import { createEnv, renderWithProviders } from "../../test/harness";
import { ImportResult } from "./KeyTools";

describe("ImportResult", () => {
  it("shows the core's localized reason for skipped keys, English only as a fallback", () => {
    renderWithProviders(
      <ImportResult
        result={{
          keys: [],
          created: 0,
          updated: 0,
          issues: [
            {
              item: "#1",
              code: "invalid_request",
              message: "no OpenPGP key found",
              localized: { id: "pgp-import-no-key" },
            },
            {
              item: "#2",
              code: "crypto",
              message: "openpgp: bad signature",
              localized: {
                id: "pgp-import-unusable",
                args: { detail: "bad signature" },
              },
            },
            { item: "#3", code: "crypto", message: "an older core's text" },
          ],
        }}
      />,
      createEnv(),
    );
    const issues = within(screen.getByRole("list", { name: "Skipped" })).getAllByRole("listitem");
    expect(issues[0]).toHaveTextContent(/#1.*no OpenPGP key found/);
    expect(issues[1]).toHaveTextContent(/the key cannot be used \(.?bad signature.?\)/);
    expect(issues[1]).not.toHaveTextContent("openpgp: bad signature");
    expect(issues[2]).toHaveTextContent("an older core's text");
  });
});
