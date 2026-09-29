import { act, screen, within } from "@testing-library/react";
import type { MessageBody } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget } from "../../test/harness";
import { securityDemoBySender } from "./mockSeed";
import { AttachmentWarningMark, MessageWarnings } from "./Warnings";

const warnings = securityDemoBySender()["noreply@shop.example"]?.warnings ?? [];

function body(extra: Partial<MessageBody> = {}): MessageBody {
  const env = createEnv();
  const first = [...env.transport.data.bodies.values()][0];
  if (!first) throw new Error("no body");
  return { ...first, ...extra };
}

describe("reader warnings", () => {
  it("lists every flagged link with its real target and every risky attachment", () => {
    renderWithProviders(<MessageWarnings message={body({ warnings })} />, createEnv());
    const region = screen.getByRole("region", { name: "Warnings" });
    expect(
      within(region).getByText("Nail found 3 things to be careful about in this message."),
    ).toBeInTheDocument();
    const items = within(region).getAllByRole("listitem");
    expect(items.map((i) => i.getAttribute("data-kind"))).toEqual([
      "link-mismatch",
      "lookalike-domain",
      "dangerous-attachment",
    ]);
    expect(
      within(items[0] as HTMLElement).getByText(
        "A link shows shop.example but goes to login.evil.example.",
      ),
    ).toBeInTheDocument();
    expect(
      within(items[0] as HTMLElement).getByText("Real target: https://login.evil.example/track"),
    ).toBeInTheDocument();
    expect(
      within(items[1] as HTMLElement).getByText(
        "A link goes to shоp.example (xn--shp-5cd.example), which imitates shop.example.",
      ),
    ).toBeInTheDocument();
    expect(
      within(items[2] as HTMLElement).getByText("“invoice.pdf.exe” hides its real file type."),
    ).toBeInTheDocument();
    expect(items[2]).toHaveAttribute("data-risk", "disguised-name");
    expect(within(region).getByText(/Hover over a link/)).toBeInTheDocument();
  });

  it("explains look-alikes without a known original and suspicious links", () => {
    renderWithProviders(
      <MessageWarnings
        message={body({
          warnings: [
            {
              kind: "lookalike-domain",
              link: 0,
              href: "https://xn--80ak6aa92e.com",
              ascii_host: "xn--80ak6aa92e.com",
              unicode_host: "аррӏе.com",
              resembles: null,
            },
            { kind: "suspicious-link", link: 1, href: "javascript:void(0)" },
          ],
        })}
      />,
      createEnv(),
    );
    expect(screen.getByText(/a name made of look-alike characters/)).toBeInTheDocument();
    expect(screen.getByText("A link looks suspicious.")).toBeInTheDocument();
    expect(
      screen.getByText("Nail found 2 things to be careful about in this message."),
    ).toBeInTheDocument();
  });

  it("renders nothing without warnings", () => {
    const { container } = renderWithProviders(
      <MessageWarnings message={body({ warnings: [] })} />,
      createEnv(),
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("marks one attachment", () => {
    renderWithProviders(<AttachmentWarningMark warnings={warnings} index={0} />, createEnv());
    const mark = screen.getByRole("note");
    expect(mark).toHaveTextContent("Careful: disguised file type");
    expect(mark).toHaveAttribute("data-risk", "disguised-name");
    const { container } = renderWithProviders(
      <AttachmentWarningMark warnings={warnings} index={3} />,
      createEnv(),
    );
    expect(container.querySelector('[data-part="security-attachment-warning"]')).toBeNull();
  });

  it("shows the warnings of the selected message as a widget", async () => {
    const env = createEnv();
    const first = [...env.transport.data.bodies.values()][0];
    if (!first) throw new Error("no body");
    first.warnings = warnings;
    renderNode(widget("security-warnings"), env);
    act(() => env.store.getState().selectMessage(first.id));
    expect(await screen.findByRole("region", { name: "Warnings" })).toBeInTheDocument();
  });
});
