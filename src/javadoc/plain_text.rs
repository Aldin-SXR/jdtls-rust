//! Port of jdt.ls `HtmlToPlainText` (a fork of jsoup's example).

use super::html::{Document, NodeId};

/// `HtmlToPlainText.getPlainText(element)`
pub fn get_plain_text(doc: &Document, root: NodeId) -> String {
    let mut accum = String::new();
    let mut list_nesting = 0usize;
    doc.traverse(root, &mut |doc, node, enter| {
        let name = doc.node_name(node).to_string();
        if enter {
            if doc.is_text(node) {
                append(&mut accum, &doc.text_node_text(node));
            } else if name == "ul" {
                list_nesting += 1;
            } else if name == "li" {
                append(&mut accum, "\n ");
                for _ in 1..list_nesting.max(1) {
                    append(&mut accum, "  ");
                }
                if list_nesting == 1 {
                    append(&mut accum, "* ");
                } else {
                    append(&mut accum, "- ");
                }
            } else if name == "dt" {
                append(&mut accum, "  ");
            } else if ["p", "h1", "h2", "h3", "h4", "h5", "tr"].contains(&name.as_str()) {
                append(&mut accum, "\n");
            }
        } else if ["br", "dd", "dt", "p", "h1", "h2", "h3", "h4", "h5"].contains(&name.as_str()) {
            append(&mut accum, "\n");
        } else if name == "th" || name == "td" {
            append(&mut accum, " ");
        } else if name == "a" {
            let url = doc.abs_url(node, "href");
            append(&mut accum, &format!(" <{}>", url));
        } else if name == "ul" {
            list_nesting = list_nesting.saturating_sub(1);
        }
    });
    accum
}

fn append(accum: &mut String, text: &str) {
    if text == " " && (accum.is_empty() || accum.ends_with(' ') || accum.ends_with('\n')) {
        return;
    }
    accum.push_str(text);
}
