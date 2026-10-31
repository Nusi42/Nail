import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { MessageBody, MessageWarning } from "../../api/generated";
import { createEnv, renderNode, renderWithProviders, widget } from "../../test/harness";
import { securityDemoBySender } from "./mockSeed";
import { MAX_SHOWN_WARNINGS, shownWarningEntries, warningEntries } from "./model";
import { AttachmentWarningMark, MessageWarnings } from "./Warnings";
import { must } from "../../test/must";

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
      within(must(items[0])).getByText("A link shows shop.example but goes to login.evil.example."),
    ).toBeInTheDocument();
    expect(
      within(must(items[0])).getByText("Real target: https://login.evil.example/track"),
    ).toBeInTheDocument();
    expect(
      within(must(items[1])).getByText(
        "A link goes to shоp.example (xn--shp-5cd.example), which imitates shop.example.",
      ),
    ).toBeInTheDocument();
    expect(
      within(must(items[2])).getByText("“invoice.pdf.exe” hides its real file type."),
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

  it("puts a disguised sender first: its look-alike domain and name", () => {
    renderWithProviders(
      <MessageWarnings
        message={body({
          warnings: [
            { kind: "suspicious-link", link: 0, href: "http://192.0.2.1/" },
            {
              kind: "lookalike-sender",
              address: "noreply@xn--ovabank-2jb.xn--om-6la",
              unicode_domain: "ńovabank.ċom",
              resembles: "novabank.com",
              name: "Kundenservice NOVABANK",
            },
          ],
        })}
      />,
      createEnv(),
    );
    const items = screen.getAllByRole("listitem");
    expect(items.map((i) => i.getAttribute("data-kind"))).toEqual([
      "lookalike-sender",
      "suspicious-link",
    ]);
    const sender = must(items[0]);
    // One text: the two sentences with a space between them.
    expect(
      within(sender).getByText(
        `The sender's address noreply@xn--ovabank-2jb.xn--om-6la uses look-alike characters: ńovabank.ċom imitates novabank.com. The sender's name is written with look-alike or invisible characters; it reads "Kundenservice NOVABANK".`,
      ),
    ).toBeInTheDocument();
    expect(
      within(sender).getByText("Do not trust who this message claims to be from."),
    ).toBeInTheDocument();
  });

  it("names a disguised sender's domain without a known original, or only its name", () => {
    renderWithProviders(
      <MessageWarnings
        message={body({
          warnings: [
            {
              kind: "lookalike-sender",
              address: "a@xn--80ak6aa92e.com",
              unicode_domain: "аррӏе.com",
              resembles: null,
              name: null,
            },
            {
              kind: "lookalike-sender",
              address: "news@shop.example",
              unicode_domain: null,
              resembles: null,
              name: "PayPal",
            },
          ],
        })}
      />,
      createEnv(),
    );
    expect(screen.getByText(/uses look-alike characters: аррӏе\.com\.$/)).toBeInTheDocument();
    expect(screen.getByText(/it reads "PayPal"/)).toBeInTheDocument();
    expect(screen.queryByText(/news@shop\.example uses/)).toBeNull();
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
    act(() => {
      env.store.getState().selectMessage(first.id);
    });
    expect(await screen.findByRole("region", { name: "Warnings" })).toBeInTheDocument();
  });

  describe("many link warnings (ADR 0206)", () => {
    const TRACKER = "https://tracker.example.net/c/" + "x".repeat(400);
    /** `n` links that show `shown` but go to the tracker. */
    const mismatches = (shown: string, n: number, from = 0): MessageWarning[] =>
      Array.from({ length: n }, (_, i) => ({
        kind: "link-mismatch" as const,
        link: from + i,
        href: `${TRACKER}${String(i)}`,
        shown_host: shown,
        target_host: "tracker.example.net",
      }));

    it("groups identical warnings and caps the entries", () => {
      const all = [
        ...mismatches("www.shop.example.com", 3),
        ...mismatches("help.example.org", 2, 3),
        ...mismatches("a.example.com", 1, 5),
        ...mismatches("b.example.com", 1, 6),
        ...mismatches("c.example.com", 1, 7),
      ];
      const entries = warningEntries(all);
      expect(entries.map((e) => (e.type === "link" ? e.links : 0))).toEqual([3, 2, 1, 1, 1]);
      const collapsed = shownWarningEntries(entries, false);
      expect(collapsed.shown).toHaveLength(MAX_SHOWN_WARNINGS);
      expect(collapsed.hidden).toBe(entries.length - MAX_SHOWN_WARNINGS);
      expect(shownWarningEntries(entries, true)).toEqual({ shown: entries, hidden: 0 });
      expect(shownWarningEntries(entries.slice(0, 2), false).hidden).toBe(0);
    });

    it("shows a few entries with their link counts, one-line targets and a Show all toggle", async () => {
      const all = [
        ...mismatches("www.shop.example.com", 4),
        ...mismatches("a.example.com", 2, 4),
        ...mismatches("b.example.com", 1, 6),
        ...mismatches("c.example.com", 1, 7),
        ...mismatches("d.example.com", 1, 8),
      ];
      renderWithProviders(<MessageWarnings message={body({ warnings: all })} />, createEnv());
      const region = screen.getByRole("region", { name: "Warnings" });
      // The total stays visible.
      expect(
        within(region).getByText("Nail found 9 things to be careful about in this message."),
      ).toBeInTheDocument();
      expect(within(region).getAllByRole("listitem")).toHaveLength(MAX_SHOWN_WARNINGS);
      const first = must(within(region).getAllByRole("listitem")[0]);
      expect(first).toHaveTextContent(
        "A link shows www.shop.example.com but goes to tracker.example.net. (4 links)",
      );
      const target = must(first.querySelector('[data-part="security-warnings.target"]'));
      expect(target).toHaveAttribute("title", `${TRACKER}0`);
      expect(target).toHaveTextContent(`Real target: ${TRACKER}0`);

      const toggle = within(region).getByRole("button", { name: "Show all 5 warnings (2 more)" });
      expect(toggle).toHaveAttribute("aria-expanded", "false");
      await userEvent.click(toggle);
      expect(within(region).getAllByRole("listitem")).toHaveLength(5);
      const fewer = within(region).getByRole("button", { name: "Show fewer" });
      expect(fewer).toHaveAttribute("aria-expanded", "true");
      expect(region).toHaveAttribute("data-state", "expanded");
      await userEvent.click(fewer);
      expect(within(region).getAllByRole("listitem")).toHaveLength(MAX_SHOWN_WARNINGS);
    });

    it("has no toggle when every entry fits", () => {
      renderWithProviders(
        <MessageWarnings message={body({ warnings: mismatches("www.shop.example.com", 7) })} />,
        createEnv(),
      );
      expect(screen.getAllByRole("listitem")).toHaveLength(1);
      expect(screen.queryByRole("button")).toBeNull();
      expect(screen.getByText("(7 links)")).toBeInTheDocument();
    });
  });
});
