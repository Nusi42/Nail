import {
  PIPES,
  TemplateError,
  compileTemplate,
  fewerNames,
  formatRelative,
  lookup,
  renderTemplate,
  type TemplateContext,
} from "./template";
import { must } from "../test/must";

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
      "if-many",
      "names",
      "relative",
      "short-date",
      "then",
      "truncate",
    ]);
  });

  describe("conversation rows (P2)", () => {
    const people = (count: number) =>
      Array.from({ length: count }, (_, i) => ({
        name: `P${String(i + 1)}`,
        email: `p${String(i + 1)}@x`,
        me: i === 1,
        unread: i === 0,
      }));
    const row = (source: string, data: object, context: TemplateContext = ctx) =>
      renderTemplate(compileTemplate(source), data, context);

    it("names the participants, the user as `me` and unread senders marked", () => {
      expect(
        row("{participants | names(3)}", { participants: people(3) }, { ...ctx, me: "me" })[0],
      ).toEqual({
        kind: "names",
        field: "participants",
        items: [
          { text: "P1", full: "P1", me: false, unread: true },
          { text: "me", full: "P2", me: true, unread: false },
          { text: "P3", full: "P3", me: false, unread: false },
        ],
        skipped: 0,
      });
    });

    it("keeps the first names and the newest when there are more", () => {
      const segment = row("{participants | names(3)}", { participants: people(6) })[0];
      expect(segment).toMatchObject({ kind: "names", skipped: 3 });
      expect(segment?.kind === "names" && segment.items.map((i) => i.text)).toEqual([
        "P1",
        "P2",
        "P6",
      ]);
      expect(row("{participants | names(1)}", { participants: people(4) })[0]).toMatchObject({
        items: [{ text: "P4" }],
        skipped: 3,
      });
    });

    it("writes the address when there is no `me` text, and takes one address", () => {
      expect(row("{participants | names(2)}", { participants: people(2) })[0]).toMatchObject({
        items: [{ text: "P1" }, { text: "P2", me: true }],
      });
      expect(row("{from | names(2)}", { from: { name: null, email: "a@x" } })[0]).toMatchObject({
        field: "from",
        items: [{ text: "a@x", full: "a@x", me: false, unread: false }],
      });
      expect(row("{nothing | names(2)}", {})[0]).toMatchObject({ items: [], skipped: 0 });
    });

    it("writes first names when it names several people, full names when they would clash", () => {
      const named = [
        { name: "Linus Pauling", email: "linus@x" },
        { name: "Marie Curie", email: "marie@x" },
        { name: null, email: "grace.hopper@navy.example" },
        { name: "Ada Lovelace", email: "ada@x" },
      ];
      const segment = row("{participants | names(3)}", { participants: named })[0];
      expect(segment?.kind === "names" && segment.items.map((i) => [i.text, i.full])).toEqual([
        ["Linus", "Linus Pauling"],
        ["Marie", "Marie Curie"],
        ["Ada", "Ada Lovelace"],
      ]);
      const twoAdas = [
        { name: "Ada Lovelace", email: "ada@x" },
        { name: "Ada King", email: "king@x" },
        { name: null, email: "grace.hopper@navy.example" },
      ];
      const clash = row("{participants | names(3)}", { participants: twoAdas })[0];
      expect(clash?.kind === "names" && clash.items.map((i) => i.text)).toEqual([
        "Ada Lovelace",
        "Ada King",
        "grace.hopper",
      ]);
      // One person keeps the whole name.
      expect(
        row("{participants | names(3)}", { participants: named.slice(0, 1) })[0],
      ).toMatchObject({
        items: [{ text: "Linus Pauling" }],
      });
    });

    it("leaves out the names before the newest, never the newest or me", () => {
      const item = (text: string, me = false) => ({ text, full: text, me, unread: false });
      const start = { items: [item("A"), item("me", true), item("C")], skipped: 0 };
      const once = must(fewerNames(start));
      expect(once.items.map((i) => i.text)).toEqual(["me", "C"]);
      expect(once.skipped).toBe(1);
      expect(fewerNames(once)).toBeNull();
      const two = must(fewerNames({ items: [item("A"), item("B"), item("C")], skipped: 2 }));
      expect(two).toEqual({ items: [item("A"), item("C")], skipped: 3 });
      expect(fewerNames({ items: [item("C")], skipped: 0 })).toBeNull();
    });

    it("shows a count only when there is more than one", () => {
      expect(row("{count | if-many}", { count: 4 })[0]).toMatchObject({
        field: "count",
        text: "4",
      });
      expect(row("{count | if-many}", { count: 1 })[0]).toMatchObject({ text: "" });
      expect(row("{count | if-many}", {})[0]).toMatchObject({ text: "" });
    });

    it("checks the pipes' arguments", () => {
      expect(() => compileTemplate("{participants | names}")).toThrow(/positive whole number/);
      expect(() => compileTemplate("{count | if-many(2)}")).toThrow(/takes no arguments/);
    });

    it("writes `then`'s text after a value that is there, inside the field (WP202)", () => {
      const source = '{latest_from | names(1) | then(":")}  {preview | then(" …")}';
      const [names, , preview] = row(
        source,
        { latest_from: { name: "Grace", email: "g@x", me: false, unread: true }, preview: "Hi" },
        ctx,
      );
      expect(names).toMatchObject({ kind: "names", suffix: ":", items: [{ text: "Grace" }] });
      expect(preview).toMatchObject({ kind: "field", text: "Hi", suffix: " …" });
      // Nothing to follow: no separator.
      const [none, , empty] = row(source, { latest_from: null, preview: "" }, ctx);
      expect(none).toEqual({ kind: "names", field: "latest-from", items: [], skipped: 0 });
      expect(empty).toEqual({ kind: "field", field: "preview", text: "", strong: false });
    });

    it("takes exactly one quoted text for `then`", () => {
      expect(() => compileTemplate("{subject | then}")).toThrow(/one quoted text/);
      expect(() => compileTemplate("{subject | then(3)}")).toThrow(/one quoted text/);
      expect(() => compileTemplate('{subject | then(":", ";")}')).toThrow(/one quoted text/);
    });
  });
});
