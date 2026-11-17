//! A small owned XML tree: parsed tolerantly with quick-xml, written by hand so the
//! output is byte-for-byte deterministic.
//!
//! The tree also carries MVR content the model does not know (vendor `UserData`,
//! unknown child nodes), which is written back unchanged.

use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

/// An XML element with its attributes (in source order) and children.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

/// A child of an [`Element`]: another element or a run of text.
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Element(Element),
    Text(String),
}

impl Element {
    pub fn new(name: impl Into<String>) -> Self {
        Element { name: name.into(), ..Default::default() }
    }

    /// The value of an attribute. Names compare case-insensitively, because
    /// exporters disagree (`IPv4` vs `ipv4`, `fileName` vs `filename`).
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// Child elements.
    pub fn elements(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Element(e) => Some(e),
            Node::Text(_) => None,
        })
    }

    /// The first child element with this name.
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.elements().find(|e| e.name == name)
    }

    /// All text directly inside this element, concatenated and trimmed.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for n in &self.children {
            if let Node::Text(t) = n {
                s.push_str(t);
            }
        }
        s.trim().to_string()
    }

    pub fn with_attr(mut self, k: &str, v: impl Into<String>) -> Self {
        self.attrs.push((k.to_string(), v.into()));
        self
    }

    pub fn with_text(mut self, t: impl Into<String>) -> Self {
        self.children.push(Node::Text(t.into()));
        self
    }

    pub fn push(&mut self, e: Element) {
        self.children.push(Node::Element(e));
    }

    /// Drop whitespace-only text nodes recursively (indentation from the source),
    /// so trees compare equal regardless of formatting.
    pub fn strip_whitespace(&mut self) {
        let has_elements = self.elements().next().is_some();
        self.children.retain(|n| match n {
            Node::Text(t) => !(has_elements && t.trim().is_empty()),
            Node::Element(_) => true,
        });
        for n in &mut self.children {
            match n {
                Node::Element(e) => e.strip_whitespace(),
                // Mixed content is written trimmed, so keep it trimmed here too.
                Node::Text(t) if has_elements => *t = t.trim().to_string(),
                Node::Text(_) => {}
            }
        }
    }
}

/// Result of a tolerant parse: the root element (if any) and problems found.
pub struct Parsed {
    pub root: Option<Element>,
    pub problems: Vec<String>,
}

/// Parse XML text into a tree. Never fails: on a syntax error the elements read so
/// far are kept (open elements are closed) and the error is reported.
pub fn parse(text: &str) -> Parsed {
    let mut reader = Reader::from_str(text);
    let cfg = reader.config_mut();
    cfg.check_end_names = false;
    cfg.trim_text(false);

    let mut problems = Vec::new();
    let mut stack: Vec<Element> = Vec::new();
    let mut root: Option<Element> = None;

    let close = |stack: &mut Vec<Element>, root: &mut Option<Element>, el: Element| {
        if let Some(parent) = stack.last_mut() {
            parent.children.push(Node::Element(el));
        } else if root.is_none() {
            *root = Some(el);
        }
    };

    loop {
        match reader.read_event() {
            Ok(Event::Start(s)) => {
                stack.push(start_element(&s, &mut problems));
            }
            Ok(Event::Empty(s)) => {
                let el = start_element(&s, &mut problems);
                close(&mut stack, &mut root, el);
            }
            Ok(Event::End(_)) => {
                if let Some(el) = stack.pop() {
                    close(&mut stack, &mut root, el);
                } else {
                    problems.push("closing tag without opening tag".into());
                }
            }
            Ok(Event::Text(t)) => {
                if let Some(top) = stack.last_mut() {
                    push_text(top, &t.xml10_content());
                }
            }
            Ok(Event::CData(t)) => {
                if let Some(top) = stack.last_mut() {
                    push_text(top, &t.xml10_content());
                }
            }
            Ok(Event::GeneralRef(r)) => {
                let resolved = match r.resolve_char_ref() {
                    Ok(Some(c)) => Some(c.to_string()),
                    Ok(None) => quick_xml::escape::resolve_predefined_entity(&r).map(String::from),
                    Err(_) => None,
                };
                let s = resolved.unwrap_or_else(|| {
                    problems.push(format!("unknown entity &{};", &*r));
                    format!("&{};", &*r)
                });
                if let Some(top) = stack.last_mut() {
                    push_text(top, &s);
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {} // declaration, comments, PIs, doctype
            Err(e) => {
                problems.push(format!("XML syntax error at byte {}: {e}", reader.error_position()));
                break;
            }
        }
    }
    if !stack.is_empty() {
        problems.push(format!("{} unclosed element(s) at end of XML", stack.len()));
        while let Some(el) = stack.pop() {
            close(&mut stack, &mut root, el);
        }
    }
    Parsed { root, problems }
}

fn push_text(el: &mut Element, s: &str) {
    if let Some(Node::Text(t)) = el.children.last_mut() {
        t.push_str(s);
    } else {
        el.children.push(Node::Text(s.to_string()));
    }
}

fn start_element(s: &quick_xml::events::BytesStart, problems: &mut Vec<String>) -> Element {
    let name = s.name().as_ref().to_string();
    let mut el = Element::new(name);
    for a in s.attributes().with_checks(false) {
        match a {
            Ok(a) => {
                let k = a.key.as_ref().to_string();
                let v = match a.normalized_value(XmlVersion::Implicit1_0) {
                    Ok(v) => v.into_owned(),
                    Err(_) => a.value.to_string(),
                };
                el.attrs.push((k, v));
            }
            Err(e) => problems.push(format!("bad attribute in <{}>: {e}", el.name)),
        }
    }
    el
}

/// Write a tree as an indented XML document with declaration.
pub fn write_document(root: &Element) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\n");
    write_element(&mut out, root, 0);
    out
}

fn write_element(out: &mut String, el: &Element, depth: usize) {
    indent(out, depth);
    out.push('<');
    out.push_str(&el.name);
    for (k, v) in &el.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape_into(out, v, true);
        out.push('"');
    }
    if el.children.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push('>');
    let only_text = el.children.iter().all(|n| matches!(n, Node::Text(_)));
    if only_text {
        for n in &el.children {
            if let Node::Text(t) = n {
                escape_into(out, t, false);
            }
        }
    } else {
        out.push('\n');
        for n in &el.children {
            match n {
                Node::Element(e) => write_element(out, e, depth + 1),
                Node::Text(t) => {
                    if !t.trim().is_empty() {
                        indent(out, depth + 1);
                        escape_into(out, t.trim(), false);
                        out.push('\n');
                    }
                }
            }
        }
        indent(out, depth);
    }
    out.push_str("</");
    out.push_str(&el.name);
    out.push_str(">\n");
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn escape_into(out: &mut String, s: &str, attr: bool) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' if attr => out.push_str("&quot;"),
            '\n' if attr => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' if attr => out.push_str("&#9;"),
            // Characters XML 1.0 cannot carry at all are dropped.
            c if (c as u32) < 0x20 && !matches!(c, '\n' | '\t') => {}
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_text_attrs_and_entities() {
        let src = r#"<?xml version="1.0"?><A x="1 &amp; 2" y='q"'><B>a &lt; b &#x41;</B><C/>tail</A>"#;
        let p = parse(src);
        assert!(p.problems.is_empty(), "{:?}", p.problems);
        let mut root = p.root.unwrap();
        root.strip_whitespace();
        assert_eq!(root.attr("X"), Some("1 & 2"));
        assert_eq!(root.child("B").unwrap().text(), "a < b A");
        let mut again = parse(&write_document(&root)).root.unwrap();
        again.strip_whitespace();
        assert_eq!(again, root);
    }

    #[test]
    fn broken_xml_keeps_what_was_read() {
        let p = parse("<A><B>1</B><C><D>");
        assert!(!p.problems.is_empty());
        let root = p.root.unwrap();
        assert_eq!(root.name, "A");
        assert!(root.child("B").is_some());
        assert!(root.child("C").unwrap().child("D").is_some());
    }

    #[test]
    fn garbage_gives_no_root() {
        let p = parse("not xml at all");
        assert!(p.root.is_none());
    }
}
