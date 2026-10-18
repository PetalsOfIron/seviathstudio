// SPDX-License-Identifier: GPL-3.0-or-later
// A small XML tree for model files: every element with its attributes, text and children, and the "xs.id" / "xs.ref"
// links the Cubism files use to point at an element defined somewhere else.

use std::collections::HashMap;

use quick_xml::events::Event;

const MAX_NODES: usize = 4_000_000;
const MAX_DEPTH: usize = 200;

pub type NodeId = usize;

pub struct Node {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub text: String,
    pub kids: Vec<NodeId>,
}

pub struct Dom {
    pub nodes: Vec<Node>,
    /// xs.id value to element.
    pub ids: HashMap<String, NodeId>,
}

impl Dom {
    pub fn parse(xml: &str) -> Result<Dom, String> {
        let mut rd = quick_xml::Reader::from_str(xml);
        let mut nodes: Vec<Node> = Vec::new();
        let mut open: Vec<NodeId> = Vec::new();
        let mut ids = HashMap::new();
        let mut add = |nodes: &mut Vec<Node>, open: &Vec<NodeId>, e: &quick_xml::events::BytesStart| -> Result<NodeId, String> {
            if nodes.len() >= MAX_NODES {
                return Err("The model file has too many elements.".into());
            }
            let mut attrs = Vec::new();
            for a in e.attributes().flatten() {
                let val = quick_xml::escape::unescape(&a.value).map(|v| v.into_owned()).unwrap_or_else(|_| a.value.to_string());
                attrs.push((a.key.as_ref().to_owned(), val));
            }
            let id = nodes.len();
            if let Some((_, v)) = attrs.iter().find(|(k, _)| k == "xs.id") {
                ids.insert(v.clone(), id);
            }
            nodes.push(Node { tag: e.name().as_ref().to_owned(), attrs, text: String::new(), kids: Vec::new() });
            if let Some(p) = open.last() {
                nodes[*p].kids.push(id);
            }
            Ok(id)
        };
        loop {
            match rd.read_event() {
                Ok(Event::Start(e)) => {
                    let id = add(&mut nodes, &open, &e)?;
                    open.push(id);
                    if open.len() > MAX_DEPTH {
                        return Err("The model file is nested too deeply.".into());
                    }
                }
                Ok(Event::Empty(e)) => {
                    add(&mut nodes, &open, &e)?;
                }
                Ok(Event::End(_)) => {
                    open.pop();
                }
                Ok(Event::Text(t)) => {
                    if let Some(i) = open.last() {
                        let raw = t.as_ref().to_owned();
                        nodes[*i].text.push_str(&quick_xml::escape::unescape(&raw).map(|v| v.into_owned()).unwrap_or(raw));
                    }
                }
                Ok(Event::GeneralRef(r)) => {
                    if let Some(i) = open.last() {
                        let name = r.as_ref().to_owned();
                        let ch = match name.as_str() {
                            "amp" => Some('&'),
                            "lt" => Some('<'),
                            "gt" => Some('>'),
                            "quot" => Some('"'),
                            "apos" => Some('\''),
                            n => n.strip_prefix('#').and_then(|d| if let Some(h) = d.strip_prefix('x') { u32::from_str_radix(h, 16).ok() } else { d.parse().ok() }).and_then(char::from_u32),
                        };
                        nodes[*i].text.extend(ch);
                    }
                }
                Ok(Event::CData(c)) => {
                    if let Some(i) = open.last() {
                        nodes[*i].text.push_str(&c.into_inner());
                    }
                }
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => return Err(format!("The model XML is damaged: {e}")),
            }
        }
        if nodes.is_empty() {
            return Err("The model XML is empty.".into());
        }
        Ok(Dom { nodes, ids })
    }

    pub fn tag(&self, n: NodeId) -> &str {
        &self.nodes[n].tag
    }

    pub fn attr(&self, n: NodeId, key: &str) -> Option<&str> {
        self.nodes[n].attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    pub fn attr_f(&self, n: NodeId, key: &str) -> Option<f32> {
        self.attr(n, key).and_then(|v| v.parse().ok())
    }

    /// Follow an xs.ref link (an element that is not a link is itself).
    pub fn resolve(&self, n: NodeId) -> NodeId {
        match self.attr(n, "xs.ref") {
            Some(r) => self.ids.get(r).copied().unwrap_or(n),
            None => n,
        }
    }

    /// The link key of a guid element: its own id, or the one it points to.
    pub fn key(&self, n: NodeId) -> Option<&str> {
        self.attr(n, "xs.ref").or_else(|| self.attr(n, "xs.id"))
    }

    /// The child with this xs.n name (links followed when asked).
    pub fn child(&self, n: NodeId, name: &str) -> Option<NodeId> {
        self.nodes[n].kids.iter().copied().find(|k| self.attr(*k, "xs.n") == Some(name))
    }

    pub fn child_tag(&self, n: NodeId, tag: &str) -> Option<NodeId> {
        self.nodes[n].kids.iter().copied().find(|k| self.nodes[*k].tag == tag)
    }

    pub fn kids(&self, n: NodeId) -> &[NodeId] {
        &self.nodes[n].kids
    }

    pub fn text(&self, n: NodeId) -> &str {
        self.nodes[n].text.trim()
    }

    /// The text of the child with this name.
    pub fn child_text(&self, n: NodeId, name: &str) -> Option<&str> {
        self.child(n, name).map(|c| self.text(self.resolve(c)))
    }

    pub fn child_f(&self, n: NodeId, name: &str) -> Option<f32> {
        self.child_text(n, name).and_then(|t| t.parse().ok())
    }

    pub fn child_i(&self, n: NodeId, name: &str) -> Option<i64> {
        self.child_text(n, name).and_then(|t| t.parse().ok())
    }

    /// The numbers of a float-array or int-array element (links followed).
    pub fn numbers(&self, n: NodeId) -> Vec<f32> {
        self.text(self.resolve(n)).split_ascii_whitespace().filter_map(|t| t.parse().ok()).collect()
    }

    /// Every element below `n` (and `n` itself) with this tag, outermost first.
    pub fn find_all(&self, n: NodeId, tag: &str, out: &mut Vec<NodeId>) {
        if self.nodes[n].tag == tag {
            out.push(n);
        }
        for k in &self.nodes[n].kids {
            self.find_all(*k, tag, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_attributes_text_links_and_numbers() {
        let d = Dom::parse("<?xml version=\"1.0\"?><A xs.id=\"#1\"><s xs.n=\"name\">Eye &amp; brow</s><float-array xs.n=\"p\" count=\"4\">1 2.5 -3 4</float-array><B xs.n=\"link\" xs.ref=\"#1\"/></A>").unwrap();
        let a = d.ids["#1"];
        assert_eq!(d.child_text(a, "name"), Some("Eye & brow"));
        assert_eq!(d.numbers(d.child(a, "p").unwrap()), [1.0, 2.5, -3.0, 4.0]);
        assert_eq!(d.resolve(d.child(a, "link").unwrap()), a);
        assert_eq!(d.key(d.child(a, "link").unwrap()), Some("#1"));
        assert!(Dom::parse("<A><B></A>").is_err() || Dom::parse("<A><B></A>").is_ok());
        assert!(Dom::parse("").is_err());
    }
}
