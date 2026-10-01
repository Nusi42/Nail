//! A small namespace-aware XML tree on [`quick_xml`] for the request
//! bodies the mock DAV server receives, and escaping for its answers. A
//! copy of `nail_dav::xml`: the mock must not depend on the client it
//! tests.
//!
//! WebDAV documents are small and shallow; a tree keeps the protocol code
//! readable. Limits guard against hostile servers: at most
//! [`MAX_DEPTH`] levels and [`MAX_ELEMENTS`] elements per document.

use std::borrow::Cow;

use quick_xml::events::{BytesRef, Event};
use quick_xml::name::ResolveResult;
use quick_xml::reader::NsReader;

/// Deepest element nesting accepted.
pub const MAX_DEPTH: usize = 64;
/// Most elements accepted in one document.
pub const MAX_ELEMENTS: usize = 2_000_000;

/// An element: namespace, local name, child elements and its own text
/// (character data and CDATA of the element itself, entities resolved).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    /// Namespace URI (`""` when unbound).
    pub ns: String,
    /// Local name.
    pub name: String,
    /// Child elements in document order.
    pub children: Vec<Element>,
    /// The element's own text.
    pub text: String,
    /// Attributes (local name, unescaped value); namespace declarations
    /// are left out.
    pub attrs: Vec<(String, String)>,
}

/// A document that is not well-formed XML (or breaks the limits).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("malformed XML: {0}")]
pub struct XmlError(pub String);

impl Element {
    /// The value of attribute `name` (local name, any prefix).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether the element is `{ns}name`.
    pub fn is(&self, ns: &str, name: &str) -> bool {
        self.ns == ns && self.name == name
    }

    /// The first child `{ns}name`.
    pub fn child(&self, ns: &str, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.is(ns, name))
    }

    /// Every child `{ns}name`.
    pub fn children_named<'a>(
        &'a self,
        ns: &'a str,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children.iter().filter(move |c| c.is(ns, name))
    }

    /// The trimmed text of the first child `{ns}name`.
    pub fn child_text(&self, ns: &str, name: &str) -> Option<&str> {
        self.child(ns, name).map(|c| c.text.trim())
    }

    /// Follows a path of `(ns, name)` steps through first matches.
    pub fn path(&self, steps: &[(&str, &str)]) -> Option<&Element> {
        let mut at = self;
        for (ns, name) in steps {
            at = at.child(ns, name)?;
        }
        Some(at)
    }
}

fn resolve_reference(reference: &BytesRef<'_>) -> Result<String, XmlError> {
    if let Some(c) = reference
        .resolve_char_ref()
        .map_err(|e| XmlError(e.to_string()))?
    {
        return Ok(c.to_string());
    }
    let name = reference.xml10_content();
    let text = match name.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        other => return Err(XmlError(format!("unknown entity &{other};"))),
    };
    Ok(text.to_owned())
}

fn namespace(resolved: &ResolveResult<'_>) -> String {
    match resolved {
        ResolveResult::Bound(ns) => ns.into_inner().to_owned(),
        _ => String::new(),
    }
}

/// Parses a document into its root element.
pub fn parse(document: &str) -> Result<Element, XmlError> {
    let mut reader = NsReader::from_str(document);
    reader.config_mut().expand_empty_elements = true;
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut count = 0usize;
    loop {
        let (resolved, event) = reader
            .read_resolved_event()
            .map_err(|e| XmlError(e.to_string()))?;
        match event {
            Event::Start(start) => {
                count += 1;
                if stack.len() >= MAX_DEPTH || count > MAX_ELEMENTS {
                    return Err(XmlError("document too deep or too large".to_owned()));
                }
                if root.is_some() {
                    return Err(XmlError("content after the root element".to_owned()));
                }
                let name = AsRef::<str>::as_ref(&start.local_name()).to_owned();
                let mut attrs = Vec::new();
                for attr in start.attributes() {
                    let attr = attr.map_err(|e| XmlError(e.to_string()))?;
                    let key = AsRef::<str>::as_ref(&attr.key);
                    if key == "xmlns" || key.starts_with("xmlns:") {
                        continue;
                    }
                    let local = AsRef::<str>::as_ref(&attr.key.local_name()).to_owned();
                    let value = attr
                        .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                        .map_err(|e| XmlError(e.to_string()))?
                        .into_owned();
                    attrs.push((local, value));
                }
                stack.push(Element {
                    ns: namespace(&resolved),
                    name,
                    children: Vec::new(),
                    text: String::new(),
                    attrs,
                });
            }
            Event::End(_) => {
                let done = stack
                    .pop()
                    .ok_or_else(|| XmlError("unbalanced end tag".to_owned()))?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(done),
                    None => root = Some(done),
                }
            }
            Event::Text(text) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text.xml10_content());
                } else if !text.xml10_content().trim().is_empty() {
                    return Err(XmlError("text outside the root element".to_owned()));
                }
            }
            Event::CData(data) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&data.xml10_content());
                }
            }
            Event::GeneralRef(reference) => {
                let text = resolve_reference(&reference)?;
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&text);
                }
            }
            Event::DocType(_) => {
                return Err(XmlError("DOCTYPE is not allowed".to_owned()));
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !stack.is_empty() {
        return Err(XmlError("unexpected end of document".to_owned()));
    }
    root.ok_or_else(|| XmlError("empty document".to_owned()))
}

/// Escapes text for element content or attribute values.
pub(crate) fn escape(text: &str) -> Cow<'_, str> {
    quick_xml::escape::escape(text)
}

#[cfg(test)]
mod tests {
    use super::super::{NS_CALDAV, NS_DAV};
    use super::*;

    #[test]
    fn parses_namespaces_text_entities_and_cdata() {
        let doc = r#"<?xml version="1.0"?>
<multistatus xmlns="DAV:" xmlns:C="urn:ietf:params:xml:ns:caldav">
  <response>
    <href>/a%20b/x.ics</href>
    <propstat><prop>
      <C:calendar-data C:content-type="text/calendar" version="2.0">BEGIN:VCALENDAR&#13;
SUMMARY:Tom &amp; Jerry &lt;3</C:calendar-data>
      <displayname><![CDATA[a <b>]]></displayname>
      <empty/>
    </prop></propstat>
  </response>
</multistatus>"#;
        let root = parse(doc).unwrap();
        assert!(root.is(NS_DAV, "multistatus"));
        let response = root.child(NS_DAV, "response").unwrap();
        assert_eq!(response.child_text(NS_DAV, "href"), Some("/a%20b/x.ics"));
        let prop = response
            .path(&[(NS_DAV, "propstat"), (NS_DAV, "prop")])
            .unwrap();
        assert_eq!(
            prop.child(NS_CALDAV, "calendar-data").unwrap().text,
            "BEGIN:VCALENDAR\r\nSUMMARY:Tom & Jerry <3"
        );
        assert_eq!(prop.child_text(NS_DAV, "displayname"), Some("a <b>"));
        assert!(prop.child(NS_DAV, "empty").unwrap().children.is_empty());
        let data = prop.child(NS_CALDAV, "calendar-data").unwrap();
        assert_eq!(data.attr("content-type"), Some("text/calendar"));
        assert_eq!(data.attr("version"), Some("2.0"));
        assert_eq!(data.attr("missing"), None);
        assert!(root.attrs.is_empty());
        assert_eq!(root.children_named(NS_DAV, "response").count(), 1);
    }

    #[test]
    fn rejects_malformed_documents() {
        for bad in [
            "",
            "<a>",
            "<a></b>",
            "<a/><b/>",
            "<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>",
            "<a>&unknown;</a>",
            "text<a/>",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        let deep = "<a>".repeat(MAX_DEPTH + 1) + &"</a>".repeat(MAX_DEPTH + 1);
        assert!(parse(&deep).is_err());
    }

    #[test]
    fn unbound_prefixless_elements_have_no_namespace() {
        let root = parse("<a><b>x</b></a>").unwrap();
        assert_eq!(root.ns, "");
        assert_eq!(root.child_text("", "b"), Some("x"));
        assert_eq!(escape("a<b&\"c\""), "a&lt;b&amp;&quot;c&quot;");
    }
}
