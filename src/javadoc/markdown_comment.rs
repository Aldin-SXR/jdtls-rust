//! Markdown documentation comments (`///`): the `content.startsWith("///")`
//! branch of jdt.ls `JavadocContentAccess2.getMarkdownContent`
//! (`collectTagElements` / `collectLinkedTag`).

use crate::javadoc::doc_ast::{DocNode, DocSource, Location, Utf16};

pub struct MarkdownComment<'a> {
    doc: &'a DocSource,
    src: Utf16,
    link: &'a dyn Fn(&Location) -> String,
}

/// Java `String.split(regex)` for a literal separator (trailing empty
/// strings removed).
fn java_split_count(s: &str, sep: &str) -> usize {
    let mut parts: Vec<&str> = s.split(sep).collect();
    while parts.len() > 1 && parts.last() == Some(&"") {
        parts.pop();
    }
    if parts.len() == 1 && parts[0].is_empty() && !s.is_empty() {
        // the whole string was separators
        return 0;
    }
    parts.len()
}

/// Iteration order of a `java.util.HashMap<String, V>` with default
/// capacity, for the keys in insertion order.
pub fn java_hashmap_order(keys: &[String]) -> Vec<String> {
    fn hash(s: &str) -> i32 {
        let mut h: i32 = 0;
        for u in s.encode_utf16() {
            h = h.wrapping_mul(31).wrapping_add(u as i32);
        }
        h
    }
    let mut cap = 16usize;
    while (keys.len() as f64) > cap as f64 * 0.75 {
        cap *= 2;
    }
    let mut buckets: Vec<Vec<String>> = vec![Vec::new(); cap];
    for k in keys {
        let h = hash(k);
        let spread = (h ^ ((h as u32) >> 16) as i32) as u32;
        buckets[(spread as usize) & (cap - 1)].push(k.clone());
    }
    buckets.into_iter().flatten().collect()
}

impl<'a> MarkdownComment<'a> {
    pub fn new(doc: &'a DocSource, link: &'a dyn Fn(&Location) -> String) -> Self {
        Self { doc, src: Utf16::new(&doc.raw), link }
    }

    fn node(&self, id: usize) -> &'a DocNode {
        &self.doc.nodes[id]
    }

    pub fn render(&self) -> String {
        let mut grouped: Vec<(String, Vec<usize>)> = Vec::new();
        let mut buf = String::new();
        for &tag in &self.doc.tags {
            match self.node(tag).tag_name() {
                Some(name) => {
                    if let Some(g) = grouped.iter_mut().find(|(n, _)| n == name) {
                        g.1.push(tag);
                    } else {
                        grouped.push((name.to_owned(), vec![tag]));
                    }
                }
                None => {
                    buf.push('\n');
                    self.collect_tag_elements(tag, &mut buf);
                }
            }
        }
        let keys: Vec<String> = grouped.iter().map(|(k, _)| k.clone()).collect();
        for key in java_hashmap_order(&keys) {
            let heading = match key.as_str() {
                "@apiNote" => "API Note:",
                "@author" => "Author:",
                "@implSpec" => "Impl Spec:",
                "@implNote" => "Impl Note:",
                "@param" => "Parameters:",
                "@provides" => "Provides:",
                "@return" => "Returns:",
                "@throws" => "Throws:",
                "@exception" => "Throws:",
                "@since" => "Since:",
                "@see" => "See:",
                "@version" => "See:",
                "@uses" => "Uses:",
                _ => "",
            };
            buf.push('\n');
            buf.push_str(&format!("* **{heading}**"));
            let tags = &grouped.iter().find(|(k, _)| *k == key).unwrap().1;
            for &tag in tags {
                buf.push('\n');
                buf.push_str("  * ");
                self.collect_tag_elements(tag, &mut buf);
            }
        }
        if buf.is_empty() {
            self.doc.raw.clone()
        } else {
            buf[1..].to_owned()
        }
    }

    fn collect_tag_elements(&self, tag: usize, buf: &mut String) {
        let t = self.node(tag);
        let mut queue: std::collections::VecDeque<usize> = t.fragments.iter().copied().collect();
        while let Some(e) = queue.pop_front() {
            let en = self.node(e);
            if en.is_tag() {
                if matches!(en.tag_name(), Some("@link") | Some("@linkplain")) {
                    self.collect_linked_tag(e, buf);
                } else {
                    self.collect_tag_elements(e, buf);
                }
            } else if en.is_text() {
                buf.push_str(en.text());
            } else if t.tag_name() == Some("@see") {
                self.collect_linked_tag(tag, buf);
            }
            if let Some(&next) = queue.front() {
                let curr_end = en.end();
                let next_start = self.node(next).s;
                if curr_end != next_start {
                    if java_split_count(&self.src.substring(curr_end, next_start), "///") > 2 {
                        buf.push_str("  \n");
                    } else {
                        buf.push('\n');
                    }
                } else {
                    buf.push(' ');
                }
            }
        }
    }

    fn collect_linked_tag(&self, tag: usize, buf: &mut String) {
        let children = &self.node(tag).fragments;
        if children.is_empty() {
            return;
        }
        let (title, target) = if children.len() == 2 {
            (self.node(children[0]).text().to_owned(), children[1])
        } else {
            let target = children[0];
            let res = link_element(self.node(target));
            let c0 = self.node(target);
            let starts_with_hash = matches!(c0.t.as_str(), "memberRef" | "methodRef") && c0.qualifier.is_none();
            if res.0.is_empty() && starts_with_hash && res.1.is_some() {
                (res.1.clone().unwrap(), target)
            } else {
                (res.0, target)
            }
        };
        buf.push_str(&format!("[{title}]"));
        let uri = self.node(target).link.as_ref().map(|l| (self.link)(l)).unwrap_or_default();
        buf.push_str(&format!("({uri})"));
    }
}

/// `collectLinkElement`: (refTypeName, refMemberName)
fn link_element(n: &DocNode) -> (String, Option<String>) {
    match n.t.as_str() {
        "name" => (n.fqn.clone().unwrap_or_default(), None),
        "memberRef" | "methodRef" => (n.qualifier.clone().unwrap_or_default(), n.name.clone()),
        "text" => (n.text().to_owned(), None),
        _ => ("null".to_owned(), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_semantics() {
        assert_eq!(java_split_count("\n/// \n///", "///"), 2);
        assert_eq!(java_split_count("\n///", "///"), 1);
        assert_eq!(java_split_count("\n///\n/// ", "///"), 3);
    }
}
