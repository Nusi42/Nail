//! A tiny element tree over [`quick_xml`] events for Autodiscover POX
//! documents (the same approach as `nail-discovery`'s private one).
//! Namespaces are ignored (local names only); no DTDs or custom entities
//! are expanded.

use quick_xml::Reader;
use quick_xml::events::Event;

/// An XML element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Element {
    /// Local name.
    pub name: String,
    /// Attributes (local name, value).
    pub attrs: Vec<(String, String)>,
    /// Child elements in order.
    pub children: Vec<Element>,
    /// Concatenated text content directly inside this element.
    pub text: String,
}

/// Largest document accepted.
pub(crate) const MAX_DOCUMENT: usize = 1024 * 1024;

impl Element {
    /// The first child named `name` (ASCII case-insensitive).
    pub(crate) fn child(&self, name: &str) -> Option<&Element> {
        self.children
            .iter()
            .find(|c| c.name.eq_ignore_ascii_case(name))
    }

    /// Every child named `name`.
    pub(crate) fn children_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a Element> + 'a {
        self.children
            .iter()
            .filter(move |c| c.name.eq_ignore_ascii_case(name))
    }

    /// The trimmed, non-empty text of the first child named `name`.
    pub(crate) fn text_of(&self, name: &str) -> Option<String> {
        self.child(name)
            .map(|c| c.text.trim().to_owned())
            .filter(|t| !t.is_empty())
    }

    /// An attribute value.
    pub(crate) fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

fn local(name: &str) -> String {
    match name.rsplit_once(':') {
        Some((_, local)) => local.to_owned(),
        None => name.to_owned(),
    }
}

fn start(e: &quick_xml::events::BytesStart<'_>) -> Element {
    let attrs = e
        .attributes()
        .flatten()
        .map(|a| {
            let value = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .map(std::borrow::Cow::into_owned)
                .unwrap_or_default();
            (local(a.key.local_name().as_ref()), value)
        })
        .collect();
    Element {
        name: local(e.local_name().as_ref()),
        attrs,
        ..Element::default()
    }
}

/// Parses a document and returns its root element.
pub(crate) fn parse(xml: &[u8]) -> Result<Element, String> {
    if xml.len() > MAX_DOCUMENT {
        return Err("document too large".to_owned());
    }
    let mut reader = Reader::from_reader(xml);
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;
    let mut buf = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buf)
            .map_err(|e| format!("XML error at byte {}: {e}", reader.buffer_position()))?;
        match event {
            Event::Start(e) => {
                if stack.len() > 64 {
                    return Err("XML nested too deeply".to_owned());
                }
                stack.push(start(&e));
            }
            Event::Empty(e) => {
                let element = start(&e);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None if root.is_none() => root = Some(element),
                    None => {}
                }
            }
            Event::End(_) => {
                let element = stack.pop().ok_or("unbalanced XML")?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(element),
                    None if root.is_none() => root = Some(element),
                    None => {}
                }
            }
            Event::Text(t) => {
                if let Some(current) = stack.last_mut() {
                    current.text.push_str(&t.xml10_content());
                }
            }
            Event::CData(t) => {
                if let Some(current) = stack.last_mut() {
                    current.text.push_str(&t);
                }
            }
            Event::GeneralRef(r) => {
                if let Some(current) = stack.last_mut() {
                    if let Ok(Some(c)) = r.resolve_char_ref() {
                        current.text.push(c);
                    } else if let Some(s) = quick_xml::escape::resolve_predefined_entity(&r) {
                        current.text.push_str(s);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !stack.is_empty() {
        return Err("unexpected end of XML".to_owned());
    }
    root.ok_or_else(|| "empty XML document".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_a_tree() {
        let doc = br#"<?xml version="1.0"?>
            <a:Root xmlns:a="urn:x" version="1.1">
              <Item type="imap"><Host>imap.example.org</Host><Empty/></Item>
              <item type="pop3"><host> pop &amp; co &#x41; <![CDATA[<raw>]]></host></item>
            </a:Root>"#;
        let root = parse(doc).unwrap();
        assert_eq!(root.name, "Root");
        assert_eq!(root.attr("version"), Some("1.1"));
        let items: Vec<_> = root.children_named("item").collect();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].attr("TYPE"), Some("imap"));
        assert_eq!(
            items[0].text_of("host").as_deref(),
            Some("imap.example.org")
        );
        assert!(items[0].child("empty").is_some());
        assert_eq!(
            items[1].text_of("HOST").as_deref(),
            Some("pop & co A <raw>")
        );
        assert_eq!(items[0].text_of("missing"), None);
    }

    #[test]
    fn rejects_broken_documents() {
        assert!(parse(b"<a><b></a>").is_err());
        assert!(parse(b"<a>").is_err());
        assert!(parse(b"").is_err());
        assert!(parse(b"not xml at all").is_err());
        assert!(parse(&vec![b' '; MAX_DOCUMENT + 1]).is_err());
    }
}
