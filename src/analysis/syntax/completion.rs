//! Syntax-only completion items sourced from the current tree-sitter tree.

use std::collections::{BTreeMap, HashSet};
use tower_lsp::lsp_types::{CompletionItem, CompletionItemKind};
use tree_sitter::{Node, Tree};

type ScopeKey = (usize, usize);

/// Returns true when the cursor is positioned on the *name* identifier of a
/// variable/parameter declaration (the slot where the user types a new name).
/// Completions should be suppressed in this position.
pub fn is_in_declaration_name(tree: &Tree, _source: &str, offset: usize) -> bool {
    let root = tree.root_node();
    let check = if offset > 0 { offset - 1 } else { 0 };
    let Some(node) = root.named_descendant_for_byte_range(check, check) else {
        return false;
    };
    if node.kind() != "identifier" {
        return false;
    }
    let Some(parent) = node.parent() else { return false; };
    matches!(
        parent.kind(),
        "variable_declarator" | "formal_parameter" | "catch_formal_parameter"
    ) && parent.child_by_field_name("name") == Some(node)
}

/// Returns true when the cursor is in the *name* position of a type declaration,
/// covering both the empty-name case (`int |`) and the partial-name case
/// (`int myV|`).  Works purely from the source text so it handles incomplete
/// statements that tree-sitter may emit as ERROR nodes.
///
/// Pattern matched:  `(modifiers)* type [partial_name]`
/// where the line contains no `=`, `;`, `(`, or `)` before the cursor.
pub fn is_awaiting_declaration_name(source: &str, offset: usize) -> bool {
    let line_start = source[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let line = &source[line_start..offset];

    // Once `=`, `;`, or `(` appears the cursor is past the name slot.
    if line.bytes().any(|b| matches!(b, b'=' | b';' | b'(' | b')')) {
        return false;
    }

    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.is_empty() { return false; }

    // Peel off leading declaration modifiers.
    const MODS: &[&str] = &[
        "final", "private", "public", "protected", "static",
        "volatile", "transient", "synchronized",
    ];
    let type_and_name: Vec<&str> = {
        let mut past_mods = false;
        tokens.into_iter().filter(|t| {
            if !past_mods && MODS.contains(t) { return false; }
            past_mods = true;
            true
        }).collect()
    };

    if type_and_name.is_empty() { return false; }

    // First remaining token must be a Java type name.
    let type_tok = type_and_name[0];
    // Strip array suffixes and generic params for the base-name check.
    let base = type_tok.trim_end_matches("[]");
    let base = base.split('<').next().unwrap_or(base).trim();
    const PRIMS: &[&str] = &[
        "int", "long", "double", "float", "boolean", "char", "byte", "short",
    ];
    let is_type = PRIMS.contains(&base)
        || (base.starts_with(|c: char| c.is_uppercase())
            && base.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$'));
    if !is_type { return false; }

    match type_and_name.len() {
        // `int |` — cursor is right after the type+space, name not started yet.
        1 => line.chars().last().is_some_and(char::is_whitespace),
        // `int myV|` — one more token: the partial variable name.
        2 => {
            let name = type_and_name[1];
            // Variable names start with lowercase or `_`; class names start uppercase.
            // If the "name" token starts uppercase it's more likely a second type, skip.
            name.starts_with(|c: char| c.is_lowercase() || c == '_')
                && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
        }
        _ => false,
    }
}

/// Returns true when the cursor is right after `= ` (or `=\t`) on the current
/// line, meaning the user has opened an expression slot but hasn't typed
/// anything yet.  Auto-completing before the first letter is typed is noisy.
pub fn is_after_assignment_operator(source: &str, offset: usize) -> bool {
    if offset == 0 { return false; }
    let bytes = source.as_bytes();
    // Scan backwards past spaces/tabs.
    let mut i = offset;
    while i > 0 && matches!(bytes[i - 1], b' ' | b'\t') { i -= 1; }
    // The character before the whitespace must be `=` but NOT `==`, `!=`, `<=`, `>=`.
    if i == 0 { return false; }
    if bytes[i - 1] != b'=' { return false; }
    if i >= 2 && matches!(bytes[i - 2], b'=' | b'!' | b'<' | b'>') { return false; }
    true
}

/// Returns true when the cursor is inside a method or constructor body.
/// Used to suppress class-level snippets (main, method, field, etc.) in that context.
pub fn is_inside_method_body(tree: &Tree, offset: usize) -> bool {
    let root = tree.root_node();
    let Some(mut node) = node_at_offset(root, offset) else {
        return false;
    };
    loop {
        match node.kind() {
            "method_declaration" | "constructor_declaration" => return true,
            "class_declaration" | "interface_declaration" | "enum_declaration"
            | "record_declaration" | "compilation_unit" => return false,
            _ => {}
        }
        let Some(parent) = node.parent() else { return false; };
        node = parent;
    }
}

/// Returns true when the cursor is inside the *formal parameter list* of a method
/// or constructor declaration — i.e. between the `(` and `)` of the signature,
/// not inside the body block.
pub fn is_in_parameter_declaration(tree: &Tree, offset: usize) -> bool {
    let root = tree.root_node();
    let Some(mut node) = node_at_offset(root, offset) else {
        return false;
    };
    loop {
        match node.kind() {
            "formal_parameters" => return true,
            // Entering a block means we've left the parameter list.
            "block" | "constructor_body" => return false,
            "class_body" | "interface_body" | "enum_body" | "compilation_unit" => return false,
            _ => {}
        }
        let Some(parent) = node.parent() else { return false; };
        node = parent;
    }
}

/// Returns true when the cursor is in the *parameter-name slot* — i.e. the current
/// parameter segment (text after the last `,` or `(` before the cursor) already
/// contains a type token before the current prefix, so the cursor is where the name
/// should go.
///
/// Examples:
/// - `String |` → true  (cursor in name slot, prefix is "")
/// - `String fo|` → true  (partial name being typed)
/// - `Str|` → false  (cursor still in type token)
/// - `final Str|` → false  (only modifier before prefix)
/// - `|` → false  (beginning of parameter, type slot)
pub fn is_in_param_name_slot(source: &str, offset: usize) -> bool {
    let bytes = source.as_bytes();
    let end = offset.min(bytes.len());

    // Compute current prefix length (skip back over identifier chars).
    let mut prefix_start = end;
    while prefix_start > 0 && {
        let b = bytes[prefix_start - 1];
        b.is_ascii_alphanumeric() || b == b'_'
    } {
        prefix_start -= 1;
    }
    let prefix_end = prefix_start; // everything before this is "before the prefix"

    // Walk backward (before the prefix) to find the start of the current parameter segment.
    let mut i = if prefix_end > 0 { prefix_end - 1 } else { return false; };
    let mut angle_depth: i32 = 0;
    let mut seg_start = 0usize;
    loop {
        let c = bytes[i];
        match c {
            b'>' => { angle_depth += 1; }
            b'<' => { angle_depth = (angle_depth - 1).max(0); }
            b',' | b'(' if angle_depth == 0 => { seg_start = i + 1; break; }
            _ => {}
        }
        if i == 0 { break; }
        i -= 1;
    }

    // Extract segment text before the prefix.
    let before_prefix = source[seg_start..prefix_end].trim();
    if before_prefix.is_empty() {
        return false;
    }

    // Tokenize: count non-modifier, non-annotation identifier tokens.
    let mut type_token_count = 0u32;
    let mut prev_was_at = false;
    for tok in before_prefix.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '[' | ']')) {
        if tok.is_empty() { continue; }
        if tok == "@" { prev_was_at = true; continue; }
        if prev_was_at { prev_was_at = false; continue; } // skip annotation name
        prev_was_at = false;
        if tok.starts_with('@') { continue; } // fused @Annotation
        if tok == "final" { continue; }
        if tok.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            type_token_count += 1;
        }
    }
    type_token_count >= 1
}

/// Returns true when the cursor is inside a class/interface/enum body (at any depth),
/// including inside method bodies.  Returns false at the top level of the file.
pub fn is_inside_class_body(tree: &Tree, offset: usize) -> bool {
    let root = tree.root_node();
    let Some(mut node) = node_at_offset(root, offset) else {
        return false;
    };
    loop {
        match node.kind() {
            "class_body" | "interface_body" | "enum_body" | "record_declaration" => return true,
            "compilation_unit" => return false,
            _ => {}
        }
        let Some(parent) = node.parent() else { return false; };
        node = parent;
    }
}

/// Completion items for `this.<prefix>` — fields and methods of the current
/// class extracted from the tree-sitter AST.  Only handles `this.` (not
/// arbitrary expressions).
pub fn this_member_completions(tree: &Tree, source: &str, offset: usize) -> Vec<CompletionItem> {
    let bytes = source.as_bytes();
    let mut i = offset.min(bytes.len());
    while i > 0 && is_java_identifier_part(bytes[i - 1] as char) {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'.' {
        return vec![];
    }
    let prefix = &source[i..offset.min(source.len())];
    let dot_pos = i - 1;
    let mut j = dot_pos;
    while j > 0 && is_java_identifier_part(bytes[j - 1] as char) {
        j -= 1;
    }
    let target = &source[j..dot_pos];
    if target != "this" {
        return vec![];
    }
    let mut items = vec![];
    collect_class_members(tree.root_node(), source, prefix, &mut items);
    items
}

fn collect_class_members(node: Node, source: &str, prefix: &str, out: &mut Vec<CompletionItem>) {
    match node.kind() {
        "field_declaration" => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                if child.kind() == "variable_declarator" {
                    if let Some(name_node) = child.child_by_field_name("name") {
                        if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                            if name.starts_with(prefix) {
                                out.push(CompletionItem {
                                    label: name.to_owned(),
                                    kind: Some(CompletionItemKind::FIELD),
                                    sort_text: Some(format!("1-{name}")),
                                    ..Default::default()
                                });
                            }
                        }
                    }
                }
            }
        }
        "method_declaration" => {
            if let Some(name_node) = node.child_by_field_name("name") {
                if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                    if name.starts_with(prefix) {
                        out.push(CompletionItem {
                            label: name.to_owned(),
                            kind: Some(CompletionItemKind::METHOD),
                            sort_text: Some(format!("2-{name}")),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                collect_class_members(child, source, prefix, out);
            }
        }
    }
}

/// Completion items for simple names of imported types in this file.
/// Used to offer e.g. `List` after `import java.util.List;` in expression context.
pub fn import_type_completions(tree: &Tree, source: &str, prefix: &str) -> Vec<CompletionItem> {
    let root = tree.root_node();
    let mut items = vec![];
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if child.kind() != "import_declaration" {
            continue;
        }
        let Ok(text) = child.utf8_text(source.as_bytes()) else { continue };
        let text = text.trim().trim_end_matches(';');
        let rest = text.strip_prefix("import").map(str::trim).unwrap_or("");
        let rest = rest.strip_prefix("static").map(str::trim).unwrap_or(rest);
        let simple = rest.rsplit('.').next().unwrap_or(rest);
        if simple.is_empty() || simple == "*" {
            continue;
        }
        if prefix.is_empty() || simple.starts_with(prefix) {
            items.push(CompletionItem {
                label: simple.to_owned(),
                kind: Some(CompletionItemKind::CLASS),
                detail: Some(rest.to_owned()),
                sort_text: Some(format!("2-{simple}")),
                ..Default::default()
            });
        }
    }
    items
}

/// Extract the identifier prefix immediately before `offset` (for completion filtering).
pub fn current_prefix(source: &str, offset: usize) -> String {
    identifier_prefix(source, offset)
}

pub fn local_completions(tree: &Tree, source: &str, offset: usize) -> Vec<CompletionItem> {
    let prefix = identifier_prefix(source, offset);
    let visible_scopes = visible_scope_chain(tree, offset);
    let mut items = BTreeMap::new();

    collect_visible_declarations(tree.root_node(), source, offset, &visible_scopes, &mut items);

    items
        .into_iter()
        .filter(|(label, _)| matches_prefix(label, &prefix))
        .map(|(_, item)| item)
        .collect()
}

fn collect_visible_declarations(
    node: Node,
    source: &str,
    offset: usize,
    visible_scopes: &HashSet<ScopeKey>,
    out: &mut BTreeMap<String, CompletionItem>,
) {
    if node.start_byte() > offset {
        return;
    }

    match node.kind() {
        "formal_parameter" | "spread_parameter" | "catch_formal_parameter" => {
            if let Some(scope) = enclosing_scope(node) {
                if visible_scopes.contains(&scope) {
                    maybe_add_name(
                        declaration_name_node(node),
                        source,
                        offset,
                        CompletionItemKind::VARIABLE,
                        "Parameter",
                        out,
                    );
                }
            }
        }
        "local_variable_declaration" => {
            if let Some(scope) = enclosing_scope(node) {
                if visible_scopes.contains(&scope) {
                    let mut cursor = node.walk();
                    for child in node.children(&mut cursor) {
                        if child.kind() == "variable_declarator" {
                            maybe_add_name(
                                child.child_by_field_name("name"),
                                source,
                                offset,
                                CompletionItemKind::VARIABLE,
                                "Local",
                                out,
                            );
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_visible_declarations(child, source, offset, visible_scopes, out);
    }
}

fn maybe_add_name(
    name_node: Option<Node>,
    source: &str,
    offset: usize,
    kind: CompletionItemKind,
    detail: &str,
    out: &mut BTreeMap<String, CompletionItem>,
) {
    let Some(name_node) = name_node else {
        return;
    };

    if name_node.end_byte() > offset {
        return;
    }

    let label = name_node
        .utf8_text(source.as_bytes())
        .ok()
        .map(str::to_owned);
    let Some(label) = label else {
        return;
    };

    if !is_valid_java_identifier(&label) {
        return;
    }

    out.entry(label.clone()).or_insert_with(|| CompletionItem {
        label: label.clone(),
        kind: Some(kind),
        detail: Some(detail.to_owned()),
        sort_text: Some(format!("0-{label}")),
        ..Default::default()
    });
}

fn declaration_name_node(node: Node) -> Option<Node> {
    node.child_by_field_name("name")
        .or_else(|| last_identifier(node))
}

fn visible_scope_chain(tree: &Tree, offset: usize) -> HashSet<ScopeKey> {
    let root = tree.root_node();
    let Some(mut node) = node_at_offset(root, offset) else {
        return HashSet::new();
    };

    let mut scopes = HashSet::new();
    loop {
        if is_scope(node) {
            scopes.insert(scope_key(node));
        }
        let Some(parent) = node.parent() else {
            break;
        };
        node = parent;
    }
    scopes
}

fn node_at_offset(root: Node, offset: usize) -> Option<Node> {
    if root.end_byte() == 0 {
        return Some(root);
    }

    let clamped = offset.min(root.end_byte().saturating_sub(1));
    root.named_descendant_for_byte_range(clamped, clamped)
        .or_else(|| root.descendant_for_byte_range(clamped, clamped))
        .or(Some(root))
}

fn enclosing_scope(node: Node) -> Option<ScopeKey> {
    let mut current = Some(node);
    while let Some(candidate) = current {
        if is_scope(candidate) {
            return Some(scope_key(candidate));
        }
        current = candidate.parent();
    }
    None
}

fn scope_key(node: Node) -> ScopeKey {
    (node.start_byte(), node.end_byte())
}

fn is_scope(node: Node) -> bool {
    matches!(
        node.kind(),
        "program"
            | "compilation_unit"
            | "class_body"
            | "interface_body"
            | "enum_body"
            | "annotation_type_body"
            | "block"
            | "switch_block"
            | "method_declaration"
            | "constructor_declaration"
            | "lambda_expression"
            | "catch_clause"
            | "for_statement"
            | "enhanced_for_statement"
    )
}

fn identifier_prefix(source: &str, offset: usize) -> String {
    let Some(prefix_end) = source.get(..offset) else {
        return String::new();
    };

    let start = prefix_end
        .char_indices()
        .rev()
        .find_map(|(idx, ch)| (!is_java_identifier_part(ch)).then_some(idx + ch.len_utf8()))
        .unwrap_or(0);

    prefix_end[start..].to_owned()
}

fn matches_prefix(label: &str, prefix: &str) -> bool {
    prefix.is_empty() || label.starts_with(prefix)
}

fn is_java_identifier_part(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphanumeric()
}

fn is_valid_java_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' && first != '$' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

fn last_identifier(node: Node) -> Option<Node> {
    let mut cursor = node.walk();
    let children: Vec<Node> = node.children(&mut cursor).collect();

    for child in children.into_iter().rev() {
        if child.kind() == "identifier" {
            return Some(child);
        }
        if let Some(identifier) = last_identifier(child) {
            return Some(identifier);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tree_sitter::Parser;

    fn parse(source: &str) -> Tree {
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_java::language())
            .expect("tree-sitter-java init");
        parser.parse(source, None).expect("tree available")
    }

    #[test]
    fn completes_local_variables_in_scope() {
        let source = "class Test { void run(String arg) { int value = 1;  } }";
        let tree = parse(source);
        let offset = source.find("  }").unwrap() + 1;

        let labels: Vec<String> = local_completions(&tree, source, offset)
            .into_iter()
            .map(|item| item.label)
            .collect();

        assert!(labels.iter().any(|label| label == "value"));
        assert!(labels.iter().any(|label| label == "arg"));
    }

    #[test]
    fn filters_local_completions_by_prefix() {
        let source = "class Test { void run(String arg) { int value = 1; val } }";
        let tree = parse(source);
        let offset = source.find("val }").unwrap() + 3;

        let labels: Vec<String> = local_completions(&tree, source, offset)
            .into_iter()
            .map(|item| item.label)
            .collect();

        assert!(labels.iter().any(|label| label == "value"));
        assert!(!labels.iter().any(|label| label == "arg"));
    }

    #[test]
    fn awaiting_declaration_name_suppresses() {
        // Primitives — cursor right after type+space (empty name slot)
        assert!(is_awaiting_declaration_name("    int ", 8));
        assert!(is_awaiting_declaration_name("    long ", 9));
        assert!(is_awaiting_declaration_name("    boolean ", 12));
        // Reference types
        assert!(is_awaiting_declaration_name("    String ", 11));
        assert!(is_awaiting_declaration_name("    List<String> ", 17));
        assert!(is_awaiting_declaration_name("    int[] ", 10));
        // With modifiers
        assert!(is_awaiting_declaration_name("    final int ", 14));
        // Partial name typed — still suppressed
        assert!(is_awaiting_declaration_name("    int myV", 11));
        assert!(is_awaiting_declaration_name("    final int a", 15));
        // Does NOT fire when name is complete and followed by `=` or `;`
        assert!(!is_awaiting_declaration_name("    int x =", 11));
        assert!(!is_awaiting_declaration_name("    int x;", 10));
        // Does NOT fire for a bare lowercase word (not a type)
        assert!(!is_awaiting_declaration_name("    foo ", 8));
        // Does NOT fire for something like `int x` where name starts uppercase (likely a type)
        assert!(!is_awaiting_declaration_name("    int X", 9));
    }

    #[test]
    fn after_assignment_operator_suppresses() {
        assert!(is_after_assignment_operator("int x = ", 8));
        assert!(is_after_assignment_operator("int x =\t", 8));
        // Does NOT fire for `==`, `!=`, `<=`, `>=`
        assert!(!is_after_assignment_operator("if (a == ", 9));
        assert!(!is_after_assignment_operator("if (a != ", 9));
        assert!(!is_after_assignment_operator("if (a <= ", 9));
        assert!(!is_after_assignment_operator("if (a >= ", 9));
        // Does NOT fire when a letter follows (user started typing)
        assert!(!is_after_assignment_operator("int x = s", 9));
    }
}
