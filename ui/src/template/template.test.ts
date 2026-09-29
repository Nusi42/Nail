import {
  PIPES,
  TemplateError,
  compileTemplate,
  formatRelative,
  lookup,
  renderTemplate,
  type TemplateContext,
} from "./template";

const NOW = Date.parse("2026-09-29T12:00:00Z");
const ctx: TemplateContext = { locale: "en", now: NOW, unread: false };

const message = {
  subject: "Compiler notes for Monday",
  from: { name: "Grace Hopper", email: "grace@navy.example" },
  to: [
    { name: "Ada", email: "ada@example.org" },
    { name: null, email: "bob@example.org" },
  ],
  date: "2026-09-29T11:55:00Z",
  tags: ["work", "urgent"],
  has_attachments: true,
};

const render = (source: string, context: TemplateContext = ctx) =>
  renderTemplate(compileTemplate(source), message, context);

describe("template language", () => {
  it("renders field paths and literal text", () => {
    expect(render("{from.name}  {subject}")).toEqual([
      { kind: "field", field: "from", text: "Grace Hopper", strong: false },
      { kind: "text", text: "  " },
      { kind: "field", field: "subject", text: "Compiler notes for Monday", strong: false },
    ]);
  });

  it("falls back to the email for addresses without a name", () => {
    expect(lookup({ from: { name: null, email: "x@y.z" } }, ["from", "name"])).toBe("x@y.z");
    expect(render("{to}")[0]).toMatchObject({ text: "Ada, bob@example.org" });
  });

  it("maps field names to part suffixes", () => {
    expect(render("{has_attachments}")[0]).toMatchObject({
      field: "has-attachments",
      text: "true",
    });
  });

  it("formats dates relatively and shortly", () => {
    expect(render("{date | relative}")[0]).toMatchObject({ text: "5 minutes ago" });
    expect(formatRelative(new Date(NOW - 26 * 3600_000), NOW, "en")).toBe("yesterday");
    expect(formatRelative(new Date(NOW - 10_000), NOW, "en")).toBe("now");
    expect(formatRelative(new Date(NOW - 26 * 3600_000), NOW, "de")).toBe("gestern");
    const short = render("{date | short-date}")[0];
    expect(short).toMatchObject({ kind: "field" });
    expect((short as { text: string }).text).toMatch(/\d{2}:\d{2}/);
  });

  it("truncates with an ellipsis", () => {
    expect(render("{subject | truncate(8)}")[0]).toMatchObject({ text: "Compiler…" });
    expect(render("{subject | truncate(100)}")[0]).toMatchObject({ text: message.subject });
  });

  it("marks fields strong only for unread items", () => {
    expect(render("{from.name | bold-if-unread}")[0]).toMatchObject({ strong: false });
    expect(render("{from.name | bold-if-unread}", { ...ctx, unread: true })[0]).toMatchObject({
      strong: true,
    });
  });

  it("renders chips from lists and words", () => {
    expect(render("{tags | chips}")[0]).toEqual({
      kind: "chips",
      field: "tags",
      items: ["work", "urgent"],
    });
  });

  it("chains pipes", () => {
    expect(
      render("{subject | truncate(3) | bold-if-unread}", { ...ctx, unread: true })[0],
    ).toMatchObject({
      text: "Com…",
      strong: true,
    });
  });

  it("supports escaped braces", () => {
    expect(render("{{literal}}")).toEqual([{ kind: "text", text: "{literal}" }]);
  });

  it("returns empty text for unknown fields and never reads the prototype", () => {
    expect(render("{nope.deeper}")[0]).toMatchObject({ text: "" });
    expect(render("{constructor}")[0]).toMatchObject({ text: "" });
    expect(render("{__proto__}")[0]).toMatchObject({ text: "" });
    expect(render("{subject.length}")[0]).toMatchObject({ text: "" });
  });

  it.each([
    ["{subject | shout}", /unknown pipe "shout"/],
    ["{subject | constructor}", /unknown pipe "constructor"/],
    ["{subject | toString}", /unknown pipe/],
    ["{subject | truncate}", /one positive whole number/],
    ["{subject | truncate(0)}", /one positive whole number/],
    ['{subject | truncate("x")}', /one positive whole number/],
    ["{subject | relative(3)}", /takes no arguments/],
    ["{subject", /unclosed/],
    ["subject}", /unmatched/],
    ["{}", /expected a field name/],
    ["{a.}", /expected a field name/],
    ["{a b}", /unexpected "b"/],
    ["{a | }", /expected a pipe name/],
    ["{alert(1)}", /unexpected "\("/],
  ])("rejects %s", (source, message) => {
    expect(() => compileTemplate(source)).toThrow(TemplateError);
    expect(() => compileTemplate(source)).toThrow(message);
  });

  it("reports the error position", () => {
    try {
      compileTemplate("{subject | nope}");
      expect.unreachable();
    } catch (error) {
      expect((error as TemplateError).position).toBe(11);
    }
  });

  it("has exactly the documented pipes", () => {
    expect(Object.keys(PIPES).sort()).toEqual([
      "bold-if-unread",
      "chips",
      "relative",
      "short-date",
      "truncate",
    ]);
  });
});
