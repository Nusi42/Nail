//! A small XML tree (parsed by `nail_net::xml`) for EWS SOAP bodies, escaping for the
//! bodies Nail writes, and serialization back to XML (inbox rules that
//! Nail cannot fully express are written back as the server sent them; the
//! category list is an XML document stored in a user configuration).
//!
//! EWS answers are matched by **local name**: the `m:` (messages) and `t:`
//! (types) namespaces never use the same local name for different things
//! inside one answer, and servers differ in the prefixes they use. The
//! namespace is kept for serialization. Limits guard against hostile
//! servers: at most [`MAX_DEPTH`] levels and [`MAX_ELEMENTS`] elements
//! (the parser is `nail_net::xml`'s, shared with WebDAV).

use std::borrow::Cow;
use std::fmt::Write as _;

/// Most elements accepted in one document; a document that is not
/// well-formed XML (or breaks the limits).
pub use nail_net::xml::{MAX_ELEMENTS, XmlError};
use nail_net::xml::{XmlNode, attr_value};

/// Deepest element nesting accepted (SOAP envelopes nest deeper than
/// WebDAV answers).
pub const MAX_DEPTH: usize = 96;

/// An element: namespace, local name, attributes, child elements and its
/// own text (character data and CDATA, entities resolved).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    /// Namespace URI (`""` when unbound).
    pub ns: String,
    /// Local name.
    pub name: String,
    /// Attributes (local name, unescaped value); namespace declarations
    /// are left out.
    pub attrs: Vec<(String, String)>,
    /// Child elements in document order.
    pub children: Vec<Element>,
    /// The element's own text.
    pub text: String,
}

impl XmlNode for Element {
    fn open(ns: String, name: String, attrs: Vec<(String, String)>) -> Self {
        Self {
            ns,
            name,
            attrs,
            children: Vec::new(),
            text: String::new(),
        }
    }

    fn text_mut(&mut self) -> &mut String {
        &mut self.text
    }

    fn push_child(&mut self, child: Self) {
        self.children.push(child);
    }
}

impl Element {
    /// An element without namespace.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// The value of attribute `name` (local name, any prefix).
    pub fn attr(&self, name: &str) -> Option<&str> {
        attr_value(&self.attrs, name)
    }

    /// Sets (or adds) an attribute.
    pub fn set_attr(&mut self, name: &str, value: impl Into<String>) {
        let value = value.into();
        match self.attrs.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = value,
            None => self.attrs.push((name.to_owned(), value)),
        }
    }

    /// The first child with local name `name`.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    /// The first child with local name `name`, mutable.
    pub fn child_mut(&mut self, name: &str) -> Option<&mut Element> {
        self.children.iter_mut().find(|c| c.name == name)
    }

    /// Every child with local name `name`.
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// The trimmed text of the first child `name`.
    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.trim())
    }

    /// The trimmed text of the first child `name`, if not empty.
    pub fn child_value(&self, name: &str) -> Option<String> {
        self.child_text(name)
            .filter(|t| !t.is_empty())
            .map(str::to_owned)
    }

    /// The first child `name` parsed as a boolean (`true` / `1`).
    pub fn child_bool(&self, name: &str) -> Option<bool> {
        self.child_text(name)
            .map(|t| t.eq_ignore_ascii_case("true") || t == "1")
    }

    /// The first child `name` parsed as a number.
    pub fn child_number<T: std::str::FromStr>(&self, name: &str) -> Option<T> {
        self.child_text(name).and_then(|t| t.parse().ok())
    }

    /// Follows a path of local names through first matches.
    pub fn path(&self, steps: &[&str]) -> Option<&Element> {
        let mut at = self;
        for name in steps {
            at = at.child(name)?;
        }
        Some(at)
    }

    /// The first descendant (depth first, the element itself excluded)
    /// with local name `name`.
    pub fn find(&self, name: &str) -> Option<&Element> {
        for child in &self.children {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = child.find(name) {
                return Some(found);
            }
        }
        None
    }

    /// The texts of the children `name` of the child `list`
    /// (`<t:Categories><t:String>a</t:String>…`).
    pub fn strings(&self, list: &str, name: &str) -> Vec<String> {
        self.child(list)
            .map(|l| {
                l.children_named(name)
                    .map(|c| c.text.trim().to_owned())
                    .filter(|t| !t.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Writes the element as XML, every element with `prefix` (`t`) or
    /// unprefixed (`None`).
    pub fn write_xml(&self, prefix: Option<&str>, out: &mut String) {
        let name = match prefix {
            Some(p) => format!("{p}:{}", self.name),
            None => self.name.clone(),
        };
        out.push('<');
        out.push_str(&name);
        for (key, value) in &self.attrs {
            let _ = write!(out, " {key}=\"{}\"", escape(value));
        }
        if self.children.is_empty() && self.text.is_empty() {
            out.push_str("/>");
            return;
        }
        out.push('>');
        out.push_str(&escape(&self.text));
        for child in &self.children {
            child.write_xml(prefix, out);
        }
        let _ = write!(out, "</{name}>");
    }

    /// The element as XML (see [`Self::write_xml`]).
    pub fn to_xml(&self, prefix: Option<&str>) -> String {
        let mut out = String::new();
        self.write_xml(prefix, &mut out);
        out
    }

    /// A standalone document: an XML declaration and the element with its
    /// namespace as the default namespace (`xmlns="…"`), children
    /// unprefixed.
    pub fn to_document(&self) -> String {
        let mut root = self.clone();
        if !root.ns.is_empty() && root.attr("xmlns").is_none() {
            root.attrs.insert(0, ("xmlns".to_owned(), root.ns.clone()));
        }
        let mut out = String::from(r#"<?xml version="1.0" encoding="utf-8"?>"#);
        root.write_xml(None, &mut out);
        out
    }
}

/// Parses a document (a leading byte order mark is skipped) into its root
/// element.
pub fn parse(document: &str) -> Result<Element, XmlError> {
    let document = document.strip_prefix('\u{feff}').unwrap_or(document);
    nail_net::xml::parse_tree(document, MAX_DEPTH)
}

/// Escapes text for element content or attribute values. Characters XML
/// 1.0 cannot carry (control characters other than tab / newline / CR)
/// are dropped.
pub fn escape(text: &str) -> Cow<'_, str> {
    if text.chars().any(|c| {
        (c < ' ' && !matches!(c, '\t' | '\n' | '\r')) || c == '\u{fffe}' || c == '\u{ffff}'
    }) {
        let clean: String = text
            .chars()
            .filter(|c| {
                (*c >= ' ' || matches!(c, '\t' | '\n' | '\r'))
                    && *c != '\u{fffe}'
                    && *c != '\u{ffff}'
            })
            .collect();
        return Cow::Owned(nail_net::xml::escape(clean.as_str()).into_owned());
    }
    nail_net::xml::escape(text)
}

/// `<name>text</name>` with the text escaped.
pub fn el(name: &str, text: &str) -> String {
    format!("<{name}>{}</{name}>", escape(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_local_names_text_entities_and_attributes() {
        let doc = r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
  <s:Body>
    <m:GetItemResponse xmlns:m="urn:m" xmlns:t="urn:t">
      <t:Subject>Tom &amp; Jerry &#60;3</t:Subject>
      <t:ItemId Id="AA=" ChangeKey="CQ&amp;"/>
      <t:Categories><t:String>red</t:String><t:String> </t:String><t:String>blue</t:String></t:Categories>
      <t:Data><![CDATA[a <b>]]></t:Data>
      <t:IsRead>true</t:IsRead><t:Size>42</t:Size>
    </m:GetItemResponse>
  </s:Body>
</s:Envelope>"#;
        let root = parse(doc).unwrap();
        assert_eq!(root.name, "Envelope");
        assert_eq!(root.ns, "http://schemas.xmlsoap.org/soap/envelope/");
        let response = root.path(&["Body", "GetItemResponse"]).unwrap();
        assert_eq!(response.ns, "urn:m");
        assert_eq!(response.child_text("Subject"), Some("Tom & Jerry <3"));
        let id = response.child("ItemId").unwrap();
        assert_eq!(id.attr("Id"), Some("AA="));
        assert_eq!(id.attr("ChangeKey"), Some("CQ&"));
        assert_eq!(response.strings("Categories", "String"), ["red", "blue"]);
        assert_eq!(response.child_text("Data"), Some("a <b>"));
        assert_eq!(response.child_bool("IsRead"), Some(true));
        assert_eq!(response.child_number::<u64>("Size"), Some(42));
        assert_eq!(root.find("String").unwrap().text, "red");
        assert!(root.find("Missing").is_none());
        assert_eq!(response.child_value("Missing"), None);
    }

    #[test]
    fn rejects_malformed_documents() {
        assert!(parse("<a></b>").is_err());
        let deep = "<a>".repeat(MAX_DEPTH + 1) + &"</a>".repeat(MAX_DEPTH + 1);
        assert!(parse(&deep).is_err());
        assert!(parse("\u{feff}<a/>").is_ok());
    }

    #[test]
    fn serializes_back() {
        let root = parse(
            r#"<categories xmlns="CategoryList.xsd" default="x"><category name="A &amp; B" color="3"/><n>t&lt;</n></categories>"#,
        )
        .unwrap();
        assert_eq!(root.ns, "CategoryList.xsd");
        let doc = root.to_document();
        assert!(doc.starts_with("<?xml"));
        assert!(doc.contains(r#"<categories xmlns="CategoryList.xsd" default="x">"#));
        assert!(doc.contains(r#"<category name="A &amp; B" color="3"/>"#));
        assert!(doc.contains("<n>t&lt;</n>"));
        assert_eq!(parse(&doc).unwrap(), root);
        let mut rule = Element::new("Rule");
        rule.children.push(Element {
            text: "x".into(),
            ..Element::new("DisplayName")
        });
        rule.set_attr("a", "1");
        rule.set_attr("a", "2");
        assert_eq!(
            rule.to_xml(Some("t")),
            r#"<t:Rule a="2"><t:DisplayName>x</t:DisplayName></t:Rule>"#
        );
    }

    #[test]
    fn escaping_drops_invalid_characters() {
        assert_eq!(escape("a<b & \"c\""), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(escape("bell\u{7}\tok"), "bell\tok");
        assert_eq!(el("t:Subject", "1 < 2"), "<t:Subject>1 &lt; 2</t:Subject>");
    }
}
