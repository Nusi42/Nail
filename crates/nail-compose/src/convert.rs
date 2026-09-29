//! HTML → plain text and HTML → Markdown.
//!
//! One walker over the parsed tree (dom_query / html5ever) writes either
//! target. Block structure (paragraphs, lists, quotes, tables, code) is kept
//! in both; Markdown adds inline markup. Markdown cannot express everything
//! HTML can (colours, fonts, underline, cell spans, image sizes …): the
//! conversion is best effort and reports what it dropped, so the UI can warn
//! before switching an HTML draft to Markdown (design, "Switching and
//! quoting").

use std::collections::BTreeSet;

use dom_query::{Document, NodeRef};

/// Result of [`html_to_markdown`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conversion {
    /// The Markdown.
    pub markdown: String,
    /// Formatting that could not be expressed (stable keys such as `style`,
    /// `underline`, `table-layout`); empty = lossless.
    pub dropped: Vec<&'static str>,
}

impl Conversion {
    /// Whether formatting was lost.
    pub fn lossy(&self) -> bool {
        !self.dropped.is_empty()
    }
}

/// Renders HTML as readable plain text (the `text/plain` alternative of an
/// HTML mail, signatures, quotes).
pub fn html_to_text(html: &str) -> String {
    let doc = Document::from(html);
    let mut w = Writer::new(Target::Text);
    w.children(&doc.root());
    w.finish()
}

/// Converts HTML to Markdown (best effort, see [`Conversion::dropped`]).
pub fn html_to_markdown(html: &str) -> Conversion {
    let doc = Document::from(html);
    let mut w = Writer::new(Target::Markdown);
    w.children(&doc.root());
    let dropped = w.dropped.iter().copied().collect();
    Conversion {
        markdown: w.finish(),
        dropped,
    }
}

/// Whether HTML carries any formatting beyond paragraphs and line breaks
/// (switching it to plain text loses something).
pub fn has_formatting(html: &str) -> bool {
    let doc = Document::from(html);
    doc.root().descendants_it().any(|node| {
        if !node.is_element() {
            return false;
        }
        let name = node.node_name().map(|n| n.to_string()).unwrap_or_default();
        let plain = matches!(
            name.as_str(),
            "html" | "head" | "body" | "p" | "br" | "div" | "span" | "meta" | "title"
        );
        !plain
            || node
                .attrs()
                .iter()
                .any(|a| matches!(&*a.name.local, "style" | "align" | "color"))
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Text,
    Markdown,
}

#[derive(Debug, Clone)]
struct Prefix {
    first: String,
    rest: String,
    used: bool,
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    /// 1 = line break, 2 = blank line, more = more blank lines (`<br>`s).
    lines: usize,
    /// Prefix depth the blank lines belong to (the shallowest requested).
    depth: usize,
    /// Markdown: end the line with a hard break (`\`).
    hard: bool,
}

struct Writer {
    target: Target,
    out: String,
    line: String,
    line_open: bool,
    /// Where the current line's content starts (after its prefix).
    content_start: usize,
    started: bool,
    prefixes: Vec<Prefix>,
    pending: Option<Pending>,
    space: bool,
    pre: usize,
    in_table: bool,
    dropped: BTreeSet<&'static str>,
}

const SKIPPED: &[&str] = &["head", "script", "style", "title", "template", "noscript"];

fn name_of(node: &NodeRef<'_>) -> String {
    node.node_name()
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default()
}

fn attr(node: &NodeRef<'_>, name: &str) -> Option<String> {
    node.attr(name).map(|v| v.to_string())
}

impl Writer {
    fn new(target: Target) -> Self {
        Self {
            target,
            out: String::new(),
            line: String::new(),
            line_open: false,
            content_start: 0,
            started: false,
            prefixes: Vec::new(),
            pending: None,
            space: false,
            pre: 0,
            in_table: false,
            dropped: BTreeSet::new(),
        }
    }

    fn md(&self) -> bool {
        self.target == Target::Markdown
    }

    fn drop(&mut self, what: &'static str) {
        if self.md() {
            self.dropped.insert(what);
        }
    }

    // ---- line machinery ----

    /// Requests at least `lines` line ends before the next content.
    fn request(&mut self, lines: usize) {
        if !self.started {
            return;
        }
        let depth = self.prefixes.len();
        self.pending = Some(match self.pending {
            Some(p) => Pending {
                lines: p.lines.max(lines),
                depth: p.depth.min(depth),
                hard: p.hard && lines <= 1,
            },
            None => Pending {
                lines,
                depth,
                hard: false,
            },
        });
        self.space = false;
    }

    /// A `<br>`: one more line end each time.
    fn line_break(&mut self) {
        if !self.started {
            return;
        }
        let depth = self.prefixes.len();
        self.pending = Some(match self.pending {
            Some(p) => Pending {
                lines: p.lines + 1,
                depth: p.depth.min(depth),
                hard: false,
            },
            None => Pending {
                lines: 1,
                depth,
                hard: self.md(),
            },
        });
        self.space = false;
    }

    fn prefix_for_new_line(&mut self) -> String {
        let mut out = String::new();
        for p in &mut self.prefixes {
            if p.used {
                out.push_str(&p.rest);
            } else {
                out.push_str(&p.first);
                p.used = true;
            }
        }
        out
    }

    fn blank_prefix(&self, depth: usize) -> String {
        self.prefixes
            .iter()
            .take(depth)
            .map(|p| p.rest.as_str())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    fn finish_line(&mut self, hard: bool) {
        if !self.line_open {
            return;
        }
        let keep = self.line == "-- " || self.pre > 0 && self.line.ends_with(' ');
        let mut line = if keep {
            self.line.clone()
        } else {
            self.line.trim_end().to_owned()
        };
        if hard && !line.trim().is_empty() {
            line.push('\\');
        }
        self.out.push_str(&line);
        self.out.push('\n');
        self.line.clear();
        self.line_open = false;
    }

    /// Opens a line (applying pending breaks) before writing content.
    fn open(&mut self) {
        if let Some(p) = self.pending.take() {
            self.finish_line(p.hard && p.lines == 1);
            let blank = self.blank_prefix(p.depth);
            for _ in 1..p.lines {
                self.out.push_str(&blank);
                self.out.push('\n');
            }
        }
        if !self.line_open {
            let prefix = self.prefix_for_new_line();
            self.line.push_str(&prefix);
            self.content_start = self.line.len();
            self.line_open = true;
            self.started = true;
            self.space = false;
        }
    }

    fn at_line_start(&self) -> bool {
        !self.line_open || self.pending.is_some()
    }

    /// Writes markup (not escaped, no whitespace handling).
    fn raw(&mut self, text: &str) {
        if self.space && !self.at_line_start() {
            self.line.push(' ');
        }
        self.space = false;
        self.open();
        self.line.push_str(text);
    }

    /// Writes document text: whitespace collapsed (outside `<pre>`),
    /// Markdown specials escaped.
    fn text(&mut self, text: &str) {
        if self.pre > 0 {
            let mut first = true;
            for segment in text.split('\n') {
                if !first {
                    self.request(1);
                    // Empty lines inside <pre> are real lines.
                    if segment.is_empty() {
                        self.open();
                    }
                }
                first = false;
                if !segment.is_empty() {
                    self.open();
                    self.line.push_str(segment);
                }
            }
            return;
        }
        for c in text.chars() {
            if c.is_whitespace() {
                if self.line_open && self.pending.is_none() {
                    self.space = true;
                }
                continue;
            }
            let line_start = self.at_line_start();
            if self.space && !line_start {
                self.line.push(' ');
            }
            self.space = false;
            self.open();
            if self.md() {
                let at_start = line_start || self.line.len() == self.content_start;
                if matches!(c, '\\' | '*' | '_' | '`' | '[' | ']' | '<' | '~')
                    || (self.in_table && c == '|')
                    || (at_start && matches!(c, '#' | '>' | '-' | '+' | '='))
                {
                    self.line.push('\\');
                }
            }
            self.line.push(c);
        }
    }

    fn finish(mut self) -> String {
        self.pending = None;
        self.finish_line(false);
        let mut out = String::new();
        let mut blank_run = 0;
        for line in self.out.lines() {
            let trimmed = line.trim_end_matches([' ', '\t']);
            let line = if line == "-- " { line } else { trimmed };
            if line.is_empty() || line.chars().all(|c| c == '>') {
                blank_run += 1;
                if blank_run > 1 {
                    continue;
                }
            } else {
                blank_run = 0;
            }
            out.push_str(line);
            out.push('\n');
        }
        out.trim_matches('\n').to_owned()
    }

    // ---- tree walking ----

    fn children(&mut self, node: &NodeRef<'_>) {
        for child in node.children_it(false) {
            self.node(&child);
        }
    }

    fn node(&mut self, node: &NodeRef<'_>) {
        if node.is_text() {
            let text = node.text();
            self.text(&text);
            return;
        }
        if !node.is_element() {
            if node.is_document() || node.is_fragment() {
                self.children(node);
            }
            return;
        }
        let name = name_of(node);
        if SKIPPED.contains(&name.as_str()) {
            return;
        }
        if self.pre == 0 {
            self.check_attributes(node, &name);
        }
        match name.as_str() {
            "p" | "div" if node.text().as_ref() == "-- " => {
                // The signature separator keeps its trailing space.
                self.request(2);
                self.raw("-- ");
                self.request(2);
            }
            "p" | "section" | "article" | "header" | "footer" | "figure" | "center" | "address" => {
                if name == "center" {
                    self.drop("alignment");
                }
                self.request(2);
                self.children(node);
                self.request(2);
            }
            "div" | "figcaption" | "dt" | "dd" => {
                self.request(1);
                self.children(node);
                self.request(1);
            }
            "br" => self.line_break(),
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                self.request(2);
                if self.md() {
                    let level: usize = name[1..].parse().unwrap_or(1);
                    self.raw(&format!("{} ", "#".repeat(level)));
                }
                self.children(node);
                self.request(2);
            }
            "hr" => {
                self.request(2);
                self.raw(if self.md() { "---" } else { "----" });
                self.request(2);
            }
            "blockquote" => {
                self.request(2);
                self.prefixes.push(Prefix {
                    first: "> ".into(),
                    rest: "> ".into(),
                    used: false,
                });
                self.children(node);
                self.pending_to_depth();
                self.prefixes.pop();
                self.request(2);
            }
            "ul" | "ol" => self.list(node, name == "ol"),
            "li" => {
                // A list item outside a list.
                self.list_item(node, "- ".into());
            }
            "pre" => self.pre(node),
            "code" if self.pre == 0 => self.inline_code(node),
            "strong" | "b" => self.wrap(node, "**"),
            "em" | "i" | "cite" | "dfn" => self.wrap(node, "*"),
            "del" | "s" | "strike" => self.wrap(node, "~~"),
            "a" => self.link(node),
            "img" => self.image(node),
            "input" => {
                if attr(node, "type").as_deref() == Some("checkbox") {
                    self.raw(if node.has_attr("checked") {
                        "[x]"
                    } else {
                        "[ ]"
                    });
                    self.space = true;
                }
            }
            "table" => self.table(node),
            "u" | "ins" => {
                self.drop("underline");
                self.children(node);
            }
            "sup" | "sub" => {
                self.drop("super-subscript");
                self.children(node);
            }
            "mark" => {
                self.drop("highlight");
                self.children(node);
            }
            "font" => {
                self.drop("font");
                self.children(node);
            }
            _ => self.children(node),
        }
    }

    /// Blank lines requested inside a block belong to that block, not to
    /// the one it ends in.
    fn pending_to_depth(&mut self) {
        let depth = self.prefixes.len().saturating_sub(1);
        if let Some(p) = &mut self.pending {
            p.depth = p.depth.min(depth);
        }
    }

    fn check_attributes(&mut self, node: &NodeRef<'_>, name: &str) {
        if !self.md() {
            return;
        }
        for a in node.attrs() {
            match &*a.name.local {
                "style" if !a.value.trim().is_empty() => self.drop("style"),
                "align" | "valign" => self.drop("alignment"),
                "color" | "bgcolor" => self.drop("color"),
                "width" | "height" if name == "img" => self.drop("image-size"),
                "width" | "height" | "border" | "cellpadding" | "cellspacing" => {
                    self.drop("table-layout")
                }
                "colspan" | "rowspan" if a.value.trim() != "1" => self.drop("table-layout"),
                _ => {}
            }
        }
    }

    fn wrap(&mut self, node: &NodeRef<'_>, marker: &str) {
        if !self.md() || node.text().trim().is_empty() {
            self.children(node);
            return;
        }
        self.raw(marker);
        self.children(node);
        self.space = false;
        self.line.push_str(marker);
    }

    fn inline_code(&mut self, node: &NodeRef<'_>) {
        let code = node.text().replace('\n', " ");
        if !self.md() {
            self.raw(&code);
            return;
        }
        let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
        let fence = "`".repeat(longest + 1);
        let pad = if code.starts_with('`') || code.ends_with('`') {
            " "
        } else {
            ""
        };
        self.raw(&format!("{fence}{pad}{code}{pad}{fence}"));
    }

    fn link(&mut self, node: &NodeRef<'_>) {
        let href = attr(node, "href").unwrap_or_default();
        let text = node.text().trim().to_owned();
        let usable = !href.is_empty() && !href.starts_with('#');
        if !usable {
            self.children(node);
            return;
        }
        let same = text == href || format!("mailto:{text}") == href;
        if self.md() {
            if same {
                self.raw(&format!("<{href}>"));
            } else {
                self.raw("[");
                self.children(node);
                self.space = false;
                self.line
                    .push_str(&format!("]({})", href.replace(' ', "%20")));
            }
        } else {
            self.children(node);
            if !same && !text.is_empty() && !href.starts_with("mailto:") {
                self.space = true;
                self.raw(&format!("<{href}>"));
            } else if text.is_empty() {
                self.raw(&href);
            }
        }
    }

    fn image(&mut self, node: &NodeRef<'_>) {
        let alt = attr(node, "alt").unwrap_or_default();
        let src = attr(node, "src").unwrap_or_default();
        if self.md() {
            let alt = alt.replace(['[', ']'], "");
            self.raw(&format!("![{alt}]({})", src.replace(' ', "%20")));
        } else if !alt.trim().is_empty() {
            self.raw(&format!("[{}]", alt.trim()));
        }
    }

    fn list(&mut self, node: &NodeRef<'_>, ordered: bool) {
        let nested = self
            .prefixes
            .iter()
            .any(|p| p.first.ends_with(". ") || p.first == "- ");
        self.request(if nested { 1 } else { 2 });
        let mut number: i64 = attr(node, "start")
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(1);
        for child in node.children_it(false) {
            if child.is_element() && name_of(&child) == "li" {
                let marker = if ordered {
                    let m = format!("{number}. ");
                    number += 1;
                    m
                } else {
                    "- ".to_owned()
                };
                self.list_item(&child, marker);
            } else {
                self.node(&child);
            }
        }
        self.request(if nested { 1 } else { 2 });
    }

    fn list_item(&mut self, node: &NodeRef<'_>, marker: String) {
        self.request(1);
        let rest = " ".repeat(marker.len());
        self.prefixes.push(Prefix {
            first: marker,
            rest,
            used: false,
        });
        // Paragraphs inside items stay tight.
        self.children(node);
        if let Some(p) = &mut self.pending {
            p.lines = p.lines.min(1);
        }
        let unused = self.prefixes.last().is_some_and(|p| !p.used);
        if unused {
            // An empty item still shows its marker.
            self.open();
        }
        self.pending_to_depth();
        self.prefixes.pop();
        self.request(1);
    }

    fn pre(&mut self, node: &NodeRef<'_>) {
        self.request(2);
        let language = node
            .children_it(false)
            .find(|c| c.is_element() && name_of(c) == "code")
            .and_then(|c| attr(&c, "class"))
            .and_then(|class| {
                class
                    .split_whitespace()
                    .find_map(|c| c.strip_prefix("language-").map(str::to_owned))
            })
            .unwrap_or_default();
        let code = node.text();
        let code = code.strip_suffix('\n').unwrap_or(&code).to_owned();
        if self.md() {
            let longest = code.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let fence = "`".repeat(longest.max(2) + 1);
            self.raw(&format!("{fence}{language}"));
            self.pre += 1;
            self.request(1);
            self.text(&code);
            self.request(1);
            self.pre -= 1;
            self.raw(&fence);
        } else {
            self.pre += 1;
            self.text(&code);
            self.pre -= 1;
        }
        self.request(2);
    }

    fn table(&mut self, node: &NodeRef<'_>) {
        let mut rows: Vec<Vec<(bool, String)>> = Vec::new();
        let mut simple = true;
        collect_rows(node, &mut rows, &mut simple);
        let nested_blocks = node.descendants_it().any(|d| {
            d.is_element()
                && matches!(
                    name_of(&d).as_str(),
                    "table" | "ul" | "ol" | "pre" | "blockquote" | "h1" | "h2" | "h3" | "p" | "div"
                )
                && d.id != node.id
        });
        let rendered: Vec<Vec<String>> = rows
            .iter()
            .map(|cells| cells.iter().map(|(_, html)| self.cell(html)).collect())
            .collect();
        if rendered.is_empty() {
            return;
        }
        self.request(2);
        let md_table = self.md() && simple && !nested_blocks;
        if self.md() && !md_table {
            self.drop("table-layout");
        }
        let width = rendered.iter().map(Vec::len).max().unwrap_or(0);
        for (i, row) in rendered.iter().enumerate() {
            let mut cells = row.clone();
            if md_table {
                cells.resize(width, String::new());
                self.raw(&format!("| {} |", cells.join(" | ")));
                if i == 0 {
                    self.request(1);
                    self.raw(&format!("|{}", " --- |".repeat(width)));
                }
            } else {
                self.raw(&cells.join(" | "));
            }
            self.request(1);
        }
        self.request(2);
    }

    /// One table cell's inline content, on one line.
    fn cell(&mut self, html: &str) -> String {
        let doc = Document::from(format!("<body>{html}</body>").as_str());
        let mut w = Writer::new(self.target);
        w.in_table = true;
        w.children(&doc.root());
        self.dropped.extend(w.dropped.iter().copied());
        w.finish()
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn collect_rows(node: &NodeRef<'_>, rows: &mut Vec<Vec<(bool, String)>>, simple: &mut bool) {
    for child in node.children_it(false) {
        if !child.is_element() {
            continue;
        }
        match name_of(&child).as_str() {
            "thead" | "tbody" | "tfoot" => collect_rows(&child, rows, simple),
            "tr" => {
                let mut cells = Vec::new();
                for cell in child.children_it(false) {
                    if !cell.is_element() {
                        continue;
                    }
                    let name = name_of(&cell);
                    if name == "td" || name == "th" {
                        for span in ["colspan", "rowspan"] {
                            if attr(&cell, span).is_some_and(|v| v.trim() != "1") {
                                *simple = false;
                            }
                        }
                        cells.push((name == "th", cell.inner_html().to_string()));
                    }
                }
                rows.push(cells);
            }
            "caption" => *simple = false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn text_keeps_block_structure() {
        let html = "<html><head><title>T</title><style>p{}</style></head><body>\
            <h1>Title</h1><p>Hello <b>Bob</b>,<br>second   line</p>\
            <ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>\
            <ol start=\"3\"><li>three</li></ol>\
            <blockquote><p>quoted</p><blockquote>deeper</blockquote></blockquote>\
            <p><a href=\"https://example.org\">site</a> <a href=\"mailto:a@b.c\">a@b.c</a>\
            <img src=\"cid:x\" alt=\"Logo\"></p>\
            <pre>  code\n    indented</pre><hr>\
            <table><tr><th>A</th><th>B</th></tr><tr><td>1</td><td>2</td></tr></table>\
            <script>ignored()</script></body></html>";
        assert_eq!(
            html_to_text(html),
            "Title\n\nHello Bob,\nsecond line\n\n- one\n- two\n  - deep\n\n3. three\n\n\
             > quoted\n>\n> > deeper\n\nsite <https://example.org> a@b.c[Logo]\n\n  code\n    indented\n\n\
             ----\n\nA | B\n1 | 2"
        );
    }

    #[test]
    fn markdown_round_trip_of_common_formatting() {
        let html = "<h2>Plan</h2><p>Some <strong>bold</strong>, <em>italic</em>, \
            <del>gone</del> and <code>x = 1</code>.</p>\
            <ul><li><input type=\"checkbox\" checked disabled> done</li><li>open</li></ul>\
            <blockquote><p>quote</p></blockquote>\
            <pre><code class=\"language-rust\">fn main() {}\n</code></pre>\
            <p><a href=\"https://nail.example/docs\">docs</a> <a href=\"https://x.org\">https://x.org</a>\
            <img src=\"cid:logo\" alt=\"Logo\"></p>\
            <table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>a|b</td></tr></tbody></table>";
        let conversion = html_to_markdown(html);
        assert_eq!(conversion.dropped, Vec::<&str>::new());
        assert_eq!(
            conversion.markdown,
            "## Plan\n\nSome **bold**, *italic*, ~~gone~~ and `x = 1`.\n\n- [x] done\n- open\n\n\
             > quote\n\n```rust\nfn main() {}\n```\n\n\
             [docs](https://nail.example/docs) <https://x.org>![Logo](cid:logo)\n\n\
             | A | B |\n| --- | --- |\n| 1 | a\\|b |"
        );
        assert!(!conversion.lossy());
    }

    #[test]
    fn markdown_reports_what_it_drops() {
        let conversion = html_to_markdown(
            "<p style=\"color:red\"><u>u</u> <font face=\"x\">f</font> H<sub>2</sub>O</p>\
             <table><tr><td colspan=\"2\">wide</td></tr></table><img src=\"a.png\" width=\"20\">",
        );
        assert_eq!(
            conversion.dropped,
            [
                "font",
                "image-size",
                "style",
                "super-subscript",
                "table-layout",
                "underline"
            ]
        );
        assert!(
            conversion.markdown.starts_with("u f H2O"),
            "{}",
            conversion.markdown
        );
        assert!(
            conversion.markdown.contains("wide"),
            "{}",
            conversion.markdown
        );
    }

    #[test]
    fn markdown_escapes_specials_and_keeps_breaks() {
        let conversion = html_to_markdown("<p># not a heading *x* [y]<br>next</p><p>- no list</p>");
        assert_eq!(
            conversion.markdown,
            "\\# not a heading \\*x\\* \\[y\\]\\\nnext\n\n\\- no list"
        );
    }

    #[test]
    fn signature_separator_survives() {
        let html = "<p>Hi</p><div class=\"signature\"><p>-- </p><p>Ada<br>Analyst</p></div>";
        assert_eq!(html_to_text(html), "Hi\n\n-- \n\nAda\nAnalyst");
        assert_eq!(
            html_to_markdown(html).markdown,
            "Hi\n\n-- \n\nAda\\\nAnalyst"
        );
    }

    #[test]
    fn detects_formatting() {
        assert!(!has_formatting("<p>a<br>b</p><div>c</div>"));
        assert!(has_formatting("<p>a <b>b</b></p>"));
        assert!(has_formatting("<p style=\"color:red\">a</p>"));
        assert!(!has_formatting("plain"));
    }

    #[test]
    fn nested_lists_and_quotes_in_markdown() {
        let md = html_to_markdown(
            "<ol><li>a<ul><li>b</li></ul></li><li>c</li></ol><blockquote><ul><li>q</li></ul></blockquote>",
        );
        assert_eq!(md.markdown, "1. a\n   - b\n2. c\n\n> - q");
    }
}
