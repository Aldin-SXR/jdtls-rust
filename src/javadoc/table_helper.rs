//! Port of jdt.ls `TableHelper`: normalises HTML tables before Markdown conversion.

use super::html::{Document, NodeId};

/// `TableHelper.normalizeTableHeaders`
pub fn normalize_table_headers(doc: &mut Document, table: NodeId) {
    add_missing_table_headers(doc, table);

    if let Some(thead) = doc.select_tag(table, "thead").first().copied() {
        let thead_rows = doc.select_tag(thead, "tr");
        if thead_rows.len() > 1 {
            let tbody = match doc.select_tag(table, "tbody").first().copied() {
                Some(t) => t,
                None => {
                    let t = doc.create_element("tbody");
                    doc.append_child(table, t);
                    t
                }
            };
            for i in (1..thead_rows.len()).rev() {
                let row = thead_rows[i];
                doc.detach(row);
                doc.prepend_child(tbody, row);
            }
        }
        for th in doc.select_descendant(table, "tbody", "th") {
            convert_th_to_td(doc, th);
        }
    }

    for row in doc.select_tag(table, "tr") {
        normalize_mixed_table_row(doc, row);
    }
}

fn add_missing_table_headers(doc: &mut Document, table: NodeId) {
    let mut num_cols = 0;
    if !doc.select_tag(table, "thead").is_empty() {
        return;
    }
    let thead = doc.create_element("thead");
    doc.insert_child(table, 0, thead);
    if let Some(tbody) = doc.select_tag(table, "tbody").first().copied() {
        let tbody_rows = doc.select_tag(tbody, "tr");
        if !tbody_rows.is_empty() {
            let potential_header = tbody_rows[0];
            let cols_1st_row = doc.children_size(potential_header);
            let th_size = doc.select_tag(potential_header, "th").len();
            if th_size == cols_1st_row {
                doc.append_child(thead, potential_header);
                return;
            }
            for row in &tbody_rows {
                let col_size = doc.select_tag(*row, "td").len() + doc.select_tag(*row, "th").len();
                if col_size > num_cols {
                    num_cols = col_size;
                }
            }
        }
    }
    if num_cols > 0 && doc.children_size(thead) == 0 {
        let new_header = doc.create_element("tr");
        for _ in 0..num_cols {
            let th = doc.create_element("th");
            doc.append_child(new_header, th);
        }
        doc.append_child(thead, new_header);
    }
}

fn normalize_mixed_table_row(doc: &mut Document, row: NodeId) {
    let ths = doc.select_tag(row, "th");
    let tds = doc.select_tag(row, "td");
    if !ths.is_empty() && !tds.is_empty() {
        for th in ths {
            convert_th_to_td(doc, th);
        }
    }
}

fn convert_th_to_td(doc: &mut Document, th: NodeId) {
    let td = doc.create_element("td");
    let attrs = doc.attrs(th).to_vec();
    doc.node_mut(td).attrs = attrs;
    let strong = doc.create_element("strong");
    for child in doc.child_nodes(th) {
        doc.append_child(strong, child);
    }
    doc.append_child(td, strong);
    doc.replace_with(th, td);
}
