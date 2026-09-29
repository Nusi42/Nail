//! Allowlist sanitizing of outgoing (and pasted / quoted) HTML.
//!
//! Outgoing mail keeps what the composer can produce and what renders in
//! mail clients: text formatting, headings, lists, tables, links, images
//! (`https:`, `cid:` and `data:` images), inline `style` attributes and
//! classes. Scripts, forms, frames, event handlers, `<style>` blocks and
//! `javascript:` URLs are removed.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

fn builder() -> &'static ammonia::Builder<'static> {
    static BUILDER: OnceLock<ammonia::Builder<'static>> = OnceLock::new();
    BUILDER.get_or_init(|| {
        let mut b = ammonia::Builder::default();
        b.add_tags([
            "span",
            "div",
            "font",
            "center",
            "section",
            "input",
            "u",
            "s",
            "mark",
            "figure",
            "figcaption",
            "colgroup",
            "col",
            "caption",
        ]);
        b.add_generic_attributes(["style", "class", "id", "dir", "align", "title", "lang"]);
        let mut tag_attributes: HashMap<&'static str, HashSet<&'static str>> = HashMap::new();
        tag_attributes.insert("a", HashSet::from(["href", "name", "target"]));
        tag_attributes.insert("img", HashSet::from(["src", "alt", "width", "height"]));
        tag_attributes.insert("input", HashSet::from(["type", "checked", "disabled"]));
        tag_attributes.insert("font", HashSet::from(["color", "face", "size"]));
        tag_attributes.insert("blockquote", HashSet::from(["type", "cite"]));
        tag_attributes.insert("ol", HashSet::from(["start", "type"]));
        tag_attributes.insert("li", HashSet::from(["value"]));
        tag_attributes.insert("col", HashSet::from(["span", "width"]));
        tag_attributes.insert("colgroup", HashSet::from(["span", "width"]));
        for cell in ["td", "th"] {
            tag_attributes.insert(
                cell,
                HashSet::from(["colspan", "rowspan", "valign", "width", "bgcolor"]),
            );
        }
        tag_attributes.insert(
            "table",
            HashSet::from(["border", "cellpadding", "cellspacing", "width", "bgcolor"]),
        );
        b.tag_attributes(tag_attributes);
        b.url_schemes(HashSet::from([
            "http", "https", "mailto", "tel", "cid", "data",
        ]));
        b.url_relative(ammonia::UrlRelative::PassThrough);
        b.link_rel(None);
        b.attribute_filter(|element, attribute, value| match (element, attribute) {
            // `data:` only for images; links must not carry documents.
            (_, "href") if value.trim_start().to_ascii_lowercase().starts_with("data:") => None,
            ("img", "src")
                if value.trim_start().to_ascii_lowercase().starts_with("data:")
                    && !value
                        .trim_start()
                        .to_ascii_lowercase()
                        .starts_with("data:image/") =>
            {
                None
            }
            ("input", "type") if value != "checkbox" => None,
            (_, "style") if value.to_ascii_lowercase().contains("expression(") => None,
            _ => Some(value.into()),
        });
        b
    })
}

/// Sanitizes HTML for sending (and for quoting or pasting into a draft).
pub fn sanitize_outgoing(html: &str) -> String {
    builder().clean(html).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_mail_formatting() {
        let html = "<p style=\"color:red\" class=\"x\">a <b>b</b> <u>c</u></p>\
            <table border=\"1\"><tr><td colspan=\"2\">t</td></tr></table>\
            <img src=\"cid:logo@nail\" alt=\"Logo\" width=\"20\">\
            <img src=\"data:image/png;base64,AAAA\" alt=\"\">\
            <ul><li><input type=\"checkbox\" checked=\"\" disabled=\"\"> done</li></ul>\
            <a href=\"https://example.org\">l</a>";
        let clean = sanitize_outgoing(html);
        for kept in [
            "style=\"color:red\"",
            "class=\"x\"",
            "<u>c</u>",
            "colspan=\"2\"",
            "src=\"cid:logo@nail\"",
            "data:image/png",
            "type=\"checkbox\"",
            "href=\"https://example.org\"",
        ] {
            assert!(clean.contains(kept), "{kept} missing in {clean}");
        }
        assert!(!clean.contains("rel="), "{clean}");
    }

    #[test]
    fn removes_active_content() {
        let html = "<script>x()</script><style>p{}</style><p onclick=\"y()\">p</p>\
            <a href=\"javascript:alert(1)\">j</a><a href=\"data:text/html,x\">d</a>\
            <img src=\"data:text/html,x\"><iframe src=\"https://e.org\"></iframe>\
            <form><input type=\"text\" value=\"v\"></form>\
            <p style=\"width: expression(alert(1))\">e</p>";
        let clean = sanitize_outgoing(html);
        for gone in [
            "script",
            "<style",
            "onclick",
            "javascript",
            "data:text",
            "iframe",
            "form",
            "type=\"text\"",
            "expression",
        ] {
            assert!(!clean.contains(gone), "{gone} left in {clean}");
        }
        assert!(clean.contains("<p>p</p>"), "{clean}");
    }
}
