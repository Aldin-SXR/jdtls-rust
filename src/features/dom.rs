//! A JDT DOM (`org.eclipse.jdt.core.dom`) shaped view of a compilation unit,
//! derived from the tree-sitter CST.
//!
//! Only node kinds and source ranges are modelled.  tree-sitter nodes that
//! have no DOM counterpart (`argument_list`, `class_body`, `switch_block`,
//! ...) are dissolved into their parent, nodes that JDT splits further are
//! expanded (`type_identifier` in a type position becomes `SimpleType` →
//! `SimpleName`, `modifiers` becomes one `Modifier` per keyword), and ranges
//! follow JDT (declarations start at their Javadoc, `SwitchCase` includes its
//! `:`).  Javadoc comments are parsed into `TagElement`/`TextElement`/name
//! nodes the way `DocCommentParser` does.

use tree_sitter::{Node, Tree};

#[derive(Clone, Debug)]
pub struct Dom {
    #[allow(dead_code)] // JDT node class name, kept for debugging and future handlers
    pub kind: &'static str,
    pub start: usize,
    pub end: usize,
    pub children: Vec<Dom>,
}

impl Dom {
    fn new(kind: &'static str, start: usize, end: usize, children: Vec<Dom>) -> Self {
        Dom { kind, start, end, children }
    }

    fn of(kind: &'static str, n: Node, children: Vec<Dom>) -> Self {
        Dom::new(kind, n.start_byte(), n.end_byte(), children)
    }
}

pub fn is_javadoc(text: &str) -> bool {
    text.starts_with("/**") && text != "/**/"
}

/// Builds the DOM of a compilation unit.
pub fn build(tree: &Tree, src: &str) -> Dom {
    let c = Conv { src };
    let root = tree.root_node();
    let mut children = Vec::new();
    for ch in named_children(root) {
        children.extend(c.conv(ch));
    }
    Dom::new("CompilationUnit", 0, src.len(), children)
}

/// Non-Javadoc comments (`CompilationUnit.getCommentList()` without Javadoc),
/// as `(kind, start, end)`.
pub fn comments(tree: &Tree, src: &str) -> Vec<Dom> {
    let mut out = Vec::new();
    fn walk(n: Node, src: &str, out: &mut Vec<Dom>) {
        match n.kind() {
            "line_comment" => out.push(Dom::of("LineComment", n, Vec::new())),
            "block_comment" => {
                if !is_javadoc(&src[n.byte_range()]) {
                    out.push(Dom::of("BlockComment", n, Vec::new()));
                }
            }
            _ => {
                let mut c = n.walk();
                for ch in n.children(&mut c) {
                    walk(ch, src, out);
                }
            }
        }
    }
    walk(tree.root_node(), src, &mut out);
    out
}

fn named_children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.named_children(&mut c).collect()
}

fn all_children(n: Node) -> Vec<Node> {
    let mut c = n.walk();
    n.children(&mut c).collect()
}

fn is_comment(n: Node) -> bool {
    matches!(n.kind(), "line_comment" | "block_comment")
}

struct Conv<'a> {
    src: &'a str,
}

const MODIFIER_KEYWORDS: &[&str] = &[
    "public", "protected", "private", "static", "final", "abstract", "native", "synchronized", "transient", "volatile",
    "strictfp", "default", "sealed", "non-sealed",
];

impl<'a> Conv<'a> {
    fn text(&self, n: Node) -> &'a str {
        &self.src[n.byte_range()]
    }

    fn convs(&self, n: Node) -> Vec<Dom> {
        let mut out = Vec::new();
        for ch in named_children(n) {
            out.extend(self.conv(ch));
        }
        out
    }

    /// Javadoc comment attached to the declaration `n`.
    fn javadoc_for<'t>(&self, n: Node<'t>) -> Option<Node<'t>> {
        let mut p = n.prev_sibling();
        let mut found = None;
        while let Some(s) = p {
            if !is_comment(s) {
                break;
            }
            if found.is_none() && s.kind() == "block_comment" && is_javadoc(self.text(s)) {
                found = Some(s);
            }
            p = s.prev_sibling();
        }
        found
    }

    /// A body declaration: Javadoc child first, range extended to it.
    fn decl(&self, kind: &'static str, n: Node, mut children: Vec<Dom>) -> Dom {
        let mut start = n.start_byte();
        if let Some(doc) = self.javadoc_for(n) {
            start = doc.start_byte();
            children.insert(0, self.javadoc(doc.start_byte(), doc.end_byte()));
        }
        Dom::new(kind, start, n.end_byte(), children)
    }

    fn name_chain(&self, n: Node) -> Dom {
        match n.kind() {
            "scoped_identifier" | "scoped_type_identifier" | "field_access" => {
                let parts = named_children(n);
                let mut children = Vec::new();
                for p in parts {
                    if matches!(p.kind(), "identifier" | "type_identifier") && p.end_byte() == n.end_byte() {
                        children.push(Dom::of("SimpleName", p, Vec::new()));
                    } else if matches!(p.kind(), "scoped_identifier" | "scoped_type_identifier" | "field_access" | "identifier" | "type_identifier") {
                        children.push(self.name_chain(p));
                    } else {
                        children.extend(self.conv(p));
                    }
                }
                Dom::of("QualifiedName", n, children)
            }
            _ => Dom::of("SimpleName", n, Vec::new()),
        }
    }

    fn is_name_chain(&self, n: Node) -> bool {
        match n.kind() {
            "identifier" => true,
            "field_access" => {
                let obj = n.child_by_field_name("object");
                let field = n.child_by_field_name("field");
                obj.is_some_and(|o| self.is_name_chain(o)) && field.is_some_and(|f| f.kind() == "identifier")
            }
            _ => false,
        }
    }

    fn simple_type(&self, n: Node) -> Dom {
        match n.kind() {
            "type_identifier" => Dom::of("SimpleType", n, vec![Dom::of("SimpleName", n, Vec::new())]),
            "scoped_type_identifier" => {
                let first = n.named_child(0);
                if first.is_some_and(|f| f.kind() == "generic_type") {
                    let mut children = Vec::new();
                    for ch in named_children(n) {
                        if ch.kind() == "type_identifier" && ch.end_byte() == n.end_byte() {
                            children.push(Dom::of("SimpleName", ch, Vec::new()));
                        } else {
                            children.extend(self.conv(ch));
                        }
                    }
                    Dom::of("QualifiedType", n, children)
                } else {
                    Dom::of("SimpleType", n, vec![self.name_chain(n)])
                }
            }
            _ => self.conv(n).into_iter().next().unwrap_or_else(|| Dom::of("SimpleType", n, Vec::new())),
        }
    }

    fn dimensions(&self, n: Node) -> Vec<Dom> {
        let mut out = Vec::new();
        let mut open = None;
        for ch in all_children(n) {
            match ch.kind() {
                "[" => open = Some(ch.start_byte()),
                "]" => {
                    if let Some(s) = open.take() {
                        out.push(Dom::new("Dimension", s, ch.end_byte(), Vec::new()));
                    }
                }
                _ => {}
            }
        }
        out
    }

    fn modifiers(&self, n: Node) -> Vec<Dom> {
        let mut out = Vec::new();
        for ch in all_children(n) {
            if ch.is_named() {
                out.extend(self.conv(ch));
            } else if MODIFIER_KEYWORDS.contains(&self.text(ch)) {
                out.push(Dom::of("Modifier", ch, Vec::new()));
            }
        }
        out
    }

    /// Converts the children of a declaration, mapping declaration names.
    fn decl_children(&self, n: Node) -> Vec<Dom> {
        let mut out = Vec::new();
        let mut c = n.walk();
        for (i, ch) in n.children(&mut c).enumerate() {
            if !ch.is_named() {
                continue;
            }
            let field = n.field_name_for_child(i as u32);
            if field == Some("name") && matches!(ch.kind(), "identifier" | "type_identifier") {
                out.push(Dom::of("SimpleName", ch, Vec::new()));
            } else if field == Some("dimensions") && ch.kind() == "dimensions" {
                out.extend(self.dimensions(ch));
            } else {
                out.extend(self.conv(ch));
            }
        }
        out
    }

    fn statement_context(&self, n: Node) -> bool {
        n.parent().is_some_and(|p| {
            matches!(
                p.kind(),
                "block" | "constructor_body" | "switch_block_statement_group" | "labeled_statement" | "if_statement"
                    | "while_statement" | "do_statement" | "for_statement" | "enhanced_for_statement" | "program"
            )
        })
    }

    fn conv(&self, n: Node) -> Vec<Dom> {
        let k = n.kind();
        let one = |d: Dom| vec![d];
        match k {
            "line_comment" | "block_comment" => Vec::new(),
            "ERROR" => self.convs(n),
            "package_declaration" => one(self.decl("PackageDeclaration", n, self.decl_children(n))),
            "import_declaration" => {
                let children = named_children(n).into_iter().filter(|c| c.kind() != "asterisk").map(|c| self.name_chain(c)).collect();
                one(Dom::of("ImportDeclaration", n, children))
            }
            "class_declaration" | "interface_declaration" | "enum_declaration" | "record_declaration" | "annotation_type_declaration" => {
                let kind = match k {
                    "class_declaration" | "interface_declaration" => "TypeDeclaration",
                    "enum_declaration" => "EnumDeclaration",
                    "record_declaration" => "RecordDeclaration",
                    _ => "AnnotationTypeDeclaration",
                };
                let d = self.decl(kind, n, self.decl_children(n));
                if n.parent().is_some_and(|p| matches!(p.kind(), "block" | "constructor_body" | "switch_block_statement_group")) {
                    one(Dom::new("TypeDeclarationStatement", d.start, d.end, vec![d]))
                } else {
                    one(d)
                }
            }
            "method_declaration" | "constructor_declaration" | "compact_constructor_declaration" => {
                one(self.decl("MethodDeclaration", n, self.decl_children(n)))
            }
            "annotation_type_element_declaration" => one(self.decl("AnnotationTypeMemberDeclaration", n, self.decl_children(n))),
            "field_declaration" | "constant_declaration" => one(self.decl("FieldDeclaration", n, self.decl_children(n))),
            "enum_constant" => one(self.decl("EnumConstantDeclaration", n, self.decl_children(n))),
            "static_initializer" => {
                let mut children = Vec::new();
                for ch in all_children(n) {
                    if !ch.is_named() && self.text(ch) == "static" {
                        children.push(Dom::of("Modifier", ch, Vec::new()));
                    } else if ch.is_named() {
                        children.extend(self.conv(ch));
                    }
                }
                one(self.decl("Initializer", n, children))
            }
            "block" if n.parent().is_some_and(|p| p.kind() == "class_body" || p.kind() == "enum_body_declarations") => {
                let b = Dom::of("Block", n, self.convs(n));
                one(self.decl("Initializer", n, vec![b]))
            }
            "class_body" => {
                if n.parent().is_some_and(|p| matches!(p.kind(), "object_creation_expression" | "enum_constant")) {
                    one(Dom::of("AnonymousClassDeclaration", n, self.convs(n)))
                } else {
                    self.convs(n)
                }
            }
            "modifiers" => self.modifiers(n),
            "interface_body" | "enum_body" | "enum_body_declarations" | "annotation_type_body" | "superclass" | "super_interfaces"
            | "extends_interfaces" | "type_list" | "permits" | "throws" | "type_parameters" | "type_arguments" | "formal_parameters"
            | "argument_list" | "resource_specification" | "switch_block" | "switch_block_statement_group" | "inferred_parameters"
            | "type_bound" | "annotation_argument_list" | "finally_clause" | "catch_type" | "record_pattern_body" => {
                let mut out = Vec::new();
                for ch in all_children(n) {
                    if ch.kind() == "switch_label" {
                        // `case X:` — JDT's SwitchCase includes the colon.
                        let mut end = ch.end_byte();
                        if let Some(next) = ch.next_sibling() {
                            if next.kind() == ":" || next.kind() == "->" {
                                end = next.end_byte();
                            }
                        }
                        out.push(Dom::new("SwitchCase", ch.start_byte(), end, self.convs(ch)));
                    } else if ch.is_named() {
                        if n.kind() == "inferred_parameters" && ch.kind() == "identifier" {
                            out.push(Dom::of("VariableDeclarationFragment", ch, vec![Dom::of("SimpleName", ch, Vec::new())]));
                        } else {
                            out.extend(self.conv(ch));
                        }
                    }
                }
                out
            }
            "switch_rule" => {
                let mut out = Vec::new();
                let parts = all_children(n);
                if let Some(label) = parts.iter().find(|c| c.kind() == "switch_label") {
                    let arrow_end = parts.iter().find(|c| c.kind() == "->").map_or(label.end_byte(), |a| a.end_byte());
                    out.push(Dom::new("SwitchCase", label.start_byte(), arrow_end, self.convs(*label)));
                }
                for ch in parts.iter().filter(|c| c.is_named() && c.kind() != "switch_label") {
                    out.extend(self.conv(*ch));
                }
                out
            }
            "switch_expression" => {
                let kind = if self.statement_context(n) { "SwitchStatement" } else { "SwitchExpression" };
                one(Dom::of(kind, n, self.convs(n)))
            }
            "parenthesized_expression" => {
                let is_condition = n.parent().is_some_and(|p| {
                    matches!(p.kind(), "if_statement" | "while_statement" | "do_statement" | "switch_expression" | "synchronized_statement")
                });
                if is_condition {
                    self.convs(n)
                } else {
                    one(Dom::of("ParenthesizedExpression", n, self.convs(n)))
                }
            }
            "local_variable_declaration" => {
                let kind = match n.parent().map(|p| p.kind()) {
                    Some("for_statement") => "VariableDeclarationExpression",
                    _ => "VariableDeclarationStatement",
                };
                one(Dom::of(kind, n, self.decl_children(n)))
            }
            "variable_declarator" => one(Dom::of("VariableDeclarationFragment", n, self.decl_children(n))),
            "formal_parameter" | "spread_parameter" | "catch_formal_parameter" | "receiver_parameter" => {
                one(Dom::of("SingleVariableDeclaration", n, self.decl_children(n)))
            }
            "resource" => {
                if n.child_by_field_name("name").is_some() {
                    let children = self.decl_children(n);
                    let name = n.child_by_field_name("name").unwrap();
                    let (types, rest): (Vec<Dom>, Vec<Dom>) = children.into_iter().partition(|d| d.end <= name.start_byte());
                    let frag = Dom::new("VariableDeclarationFragment", name.start_byte(), n.end_byte(), rest);
                    let mut c = types;
                    c.push(frag);
                    one(Dom::of("VariableDeclarationExpression", n, c))
                } else {
                    self.convs(n)
                }
            }
            "enhanced_for_statement" => {
                let mut children = Vec::new();
                let name = n.child_by_field_name("name");
                let value = n.child_by_field_name("value");
                let mut svd_start = None;
                let mut svd = Vec::new();
                let mut c = n.walk();
                let parts: Vec<(Option<&str>, Node)> = n.children(&mut c).enumerate().map(|(i, ch)| (n.field_name_for_child(i as u32), ch)).collect();
                for (field, ch) in parts {
                    if !ch.is_named() {
                        continue;
                    }
                    let before_value = value.is_some_and(|v| ch.end_byte() <= v.start_byte()) && name.is_some_and(|nm| ch.start_byte() <= nm.start_byte() || field == Some("dimensions"));
                    if before_value {
                        svd_start.get_or_insert(ch.start_byte());
                        if field == Some("name") {
                            svd.push(Dom::of("SimpleName", ch, Vec::new()));
                        } else if field == Some("dimensions") {
                            svd.extend(self.dimensions(ch));
                        } else {
                            svd.extend(self.conv(ch));
                        }
                    } else {
                        if let Some(s) = svd_start.take() {
                            let end = svd.last().map_or(s, |d| d.end);
                            children.push(Dom::new("SingleVariableDeclaration", s, end, std::mem::take(&mut svd)));
                        }
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of("EnhancedForStatement", n, children))
            }
            "lambda_expression" => {
                let mut children = Vec::new();
                for ch in named_children(n) {
                    if ch.kind() == "identifier" {
                        children.push(Dom::of("VariableDeclarationFragment", ch, vec![Dom::of("SimpleName", ch, Vec::new())]));
                    } else {
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of("LambdaExpression", n, children))
            }
            "method_invocation" => {
                let obj = n.child_by_field_name("object");
                let is_super = obj.is_some_and(|o| o.kind() == "super" || (o.kind() == "field_access" && o.child_by_field_name("field").is_some_and(|f| f.kind() == "super")));
                let mut children = Vec::new();
                let mut c = n.walk();
                let parts: Vec<(Option<&str>, Node)> = n.children(&mut c).enumerate().map(|(i, ch)| (n.field_name_for_child(i as u32), ch)).collect();
                for (field, ch) in parts {
                    if !ch.is_named() {
                        continue;
                    }
                    if field == Some("name") {
                        children.push(Dom::of("SimpleName", ch, Vec::new()));
                    } else if field == Some("object") {
                        if ch.kind() == "super" {
                            continue;
                        }
                        if ch.kind() == "field_access" && is_super {
                            if let Some(q) = ch.child_by_field_name("object") {
                                children.push(self.name_chain(q));
                            }
                            continue;
                        }
                        children.extend(self.expr(ch));
                    } else {
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of(if is_super { "SuperMethodInvocation" } else { "MethodInvocation" }, n, children))
            }
            "field_access" => {
                if self.is_name_chain(n) {
                    return one(self.name_chain(n));
                }
                let obj = n.child_by_field_name("object");
                let field = n.child_by_field_name("field");
                if field.is_some_and(|f| f.kind() == "this") {
                    let q = obj.map(|o| vec![self.name_chain(o)]).unwrap_or_default();
                    return one(Dom::of("ThisExpression", n, q));
                }
                let mut children = Vec::new();
                let sup = obj.is_some_and(|o| o.kind() == "super");
                if let Some(o) = obj {
                    if !sup {
                        children.extend(self.expr(o));
                    }
                }
                if let Some(f) = field {
                    children.push(Dom::of("SimpleName", f, Vec::new()));
                }
                one(Dom::of(if sup { "SuperFieldAccess" } else { "FieldAccess" }, n, children))
            }
            "object_creation_expression" => one(Dom::of("ClassInstanceCreation", n, self.convs(n))),
            "explicit_constructor_invocation" => {
                let sup = n.child_by_field_name("constructor").is_some_and(|c| c.kind() == "super");
                let mut children = Vec::new();
                for ch in named_children(n) {
                    if matches!(ch.kind(), "this" | "super") {
                        continue;
                    }
                    children.extend(self.conv(ch));
                }
                one(Dom::of(if sup { "SuperConstructorInvocation" } else { "ConstructorInvocation" }, n, children))
            }
            "binary_expression" => one(self.infix(n)),
            "identifier" => one(Dom::of("SimpleName", n, Vec::new())),
            "scoped_identifier" => one(self.name_chain(n)),
            "type_identifier" => one(if n.parent().is_some_and(|p| p.kind() == "type_parameter") {
                Dom::of("SimpleName", n, Vec::new())
            } else {
                self.simple_type(n)
            }),
            "scoped_type_identifier" => one(self.simple_type(n)),
            "generic_type" => {
                let mut children = Vec::new();
                for ch in named_children(n) {
                    if matches!(ch.kind(), "type_identifier" | "scoped_type_identifier") {
                        children.push(self.simple_type(ch));
                    } else {
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of("ParameterizedType", n, children))
            }
            "array_type" => {
                let mut children = Vec::new();
                for ch in named_children(n) {
                    if ch.kind() == "dimensions" {
                        children.extend(self.dimensions(ch));
                    } else {
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of("ArrayType", n, children))
            }
            "dimensions" => self.dimensions(n),
            "integral_type" | "floating_point_type" | "boolean_type" | "void_type" => one(Dom::of("PrimitiveType", n, Vec::new())),
            "wildcard" => one(Dom::of("WildcardType", n, self.convs(n))),
            "type_parameter" => one(Dom::of("TypeParameter", n, self.convs(n))),
            "annotated_type" => self.convs(n),
            "marker_annotation" => one(Dom::of("MarkerAnnotation", n, self.decl_children(n))),
            "annotation" => {
                let args = n.child_by_field_name("arguments");
                let normal = args.is_some_and(|a| named_children(a).iter().any(|c| c.kind() == "element_value_pair"))
                    || args.is_some_and(|a| named_children(a).is_empty());
                one(Dom::of(if normal { "NormalAnnotation" } else { "SingleMemberAnnotation" }, n, self.decl_children(n)))
            }
            "element_value_pair" => {
                let mut children = Vec::new();
                let mut c = n.walk();
                for (i, ch) in n.children(&mut c).enumerate() {
                    if !ch.is_named() {
                        continue;
                    }
                    if n.field_name_for_child(i as u32) == Some("key") {
                        children.push(Dom::of("SimpleName", ch, Vec::new()));
                    } else {
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of("MemberValuePair", n, children))
            }
            "element_value_array_initializer" | "array_initializer" => one(Dom::of("ArrayInitializer", n, self.convs(n))),
            "string_literal" => one(Dom::of(if self.text(n).starts_with("\"\"\"") { "TextBlock" } else { "StringLiteral" }, n, Vec::new())),
            "character_literal" => one(Dom::of("CharacterLiteral", n, Vec::new())),
            "decimal_integer_literal" | "hex_integer_literal" | "octal_integer_literal" | "binary_integer_literal"
            | "decimal_floating_point_literal" | "hex_floating_point_literal" => one(Dom::of("NumberLiteral", n, Vec::new())),
            "true" | "false" => one(Dom::of("BooleanLiteral", n, Vec::new())),
            "null_literal" => one(Dom::of("NullLiteral", n, Vec::new())),
            "this" => one(Dom::of("ThisExpression", n, Vec::new())),
            "class_literal" => one(Dom::of("TypeLiteral", n, self.type_children(n))),
            "unary_expression" => one(Dom::of("PrefixExpression", n, self.expr_children(n))),
            "update_expression" => {
                let prefix = n.child(0).is_some_and(|c| !c.is_named());
                one(Dom::of(if prefix { "PrefixExpression" } else { "PostfixExpression" }, n, self.expr_children(n)))
            }
            "assignment_expression" => one(Dom::of("Assignment", n, self.expr_children(n))),
            "ternary_expression" => one(Dom::of("ConditionalExpression", n, self.expr_children(n))),
            "cast_expression" => one(Dom::of("CastExpression", n, self.convs(n))),
            "instanceof_expression" => {
                let pattern = n.child_by_field_name("name").is_some() || named_children(n).iter().any(|c| c.kind() == "record_pattern");
                if pattern {
                    let mut children = Vec::new();
                    let name = n.child_by_field_name("name");
                    let parts = named_children(n);
                    let left = parts.first().copied();
                    if let Some(l) = left {
                        children.extend(self.expr(l));
                    }
                    let rest: Vec<Node> = parts.into_iter().skip(1).collect();
                    if let Some(nm) = name {
                        let start = rest.first().map_or(nm.start_byte(), |r| r.start_byte());
                        let mut svd = Vec::new();
                        for r in &rest {
                            if *r == nm {
                                svd.push(Dom::of("SimpleName", nm, Vec::new()));
                            } else {
                                svd.extend(self.conv(*r));
                            }
                        }
                        let svd = Dom::new("SingleVariableDeclaration", start, nm.end_byte(), svd);
                        children.push(Dom::new("TypePattern", start, nm.end_byte(), vec![svd]));
                    } else {
                        for r in rest {
                            children.extend(self.conv(r));
                        }
                    }
                    one(Dom::of("PatternInstanceofExpression", n, children))
                } else {
                    one(Dom::of("InstanceofExpression", n, self.convs(n)))
                }
            }
            "array_access" => one(Dom::of("ArrayAccess", n, self.expr_children(n))),
            "array_creation_expression" => {
                // `new int[3][]`: JDT has an ArrayType covering `int[3][]`.
                let mut children = Vec::new();
                let parts = named_children(n);
                let ty = parts.first().copied();
                let dims: Vec<Node> = parts.iter().copied().filter(|c| matches!(c.kind(), "dimensions_expr" | "dimensions")).collect();
                if let Some(t) = ty {
                    let end = dims.last().map_or(t.end_byte(), |d| d.end_byte());
                    let mut at = self.conv(t);
                    for d in &dims {
                        if d.kind() == "dimensions" {
                            at.extend(self.dimensions(*d));
                        } else {
                            at.push(Dom::of("Dimension", *d, Vec::new()));
                        }
                    }
                    children.push(Dom::new("ArrayType", t.start_byte(), end, at));
                    for d in &dims {
                        if d.kind() == "dimensions_expr" {
                            children.extend(self.convs(*d));
                        }
                    }
                }
                for p in parts.iter().filter(|c| c.kind() == "array_initializer") {
                    children.extend(self.conv(*p));
                }
                one(Dom::of("ArrayCreation", n, children))
            }
            "method_reference" => {
                let first = n.named_child(0);
                let kind = match first.map(|f| f.kind()) {
                    Some("super") => "SuperMethodReference",
                    _ if named_children(n).len() == 1 && all_children(n).iter().any(|c| c.kind() == "new") => "CreationReference",
                    Some("type_identifier" | "generic_type" | "scoped_type_identifier" | "array_type" | "integral_type") => "TypeMethodReference",
                    _ => "ExpressionMethodReference",
                };
                let mut children = Vec::new();
                for (i, ch) in named_children(n).into_iter().enumerate() {
                    if i > 0 && ch.kind() == "identifier" {
                        children.push(Dom::of("SimpleName", ch, Vec::new()));
                    } else if ch.kind() != "super" {
                        children.extend(self.conv(ch));
                    }
                }
                one(Dom::of(kind, n, children))
            }
            "expression_statement" => one(Dom::of("ExpressionStatement", n, self.convs(n))),
            "block" | "constructor_body" => one(Dom::of("Block", n, self.convs(n))),
            "if_statement" => one(Dom::of("IfStatement", n, self.convs(n))),
            "while_statement" => one(Dom::of("WhileStatement", n, self.convs(n))),
            "do_statement" => one(Dom::of("DoStatement", n, self.convs(n))),
            "for_statement" => one(Dom::of("ForStatement", n, self.convs(n))),
            "return_statement" => one(Dom::of("ReturnStatement", n, self.convs(n))),
            "throw_statement" => one(Dom::of("ThrowStatement", n, self.convs(n))),
            "break_statement" => one(Dom::of("BreakStatement", n, self.convs(n))),
            "continue_statement" => one(Dom::of("ContinueStatement", n, self.convs(n))),
            "yield_statement" => one(Dom::of("YieldStatement", n, self.convs(n))),
            "assert_statement" => one(Dom::of("AssertStatement", n, self.convs(n))),
            "synchronized_statement" => one(Dom::of("SynchronizedStatement", n, self.convs(n))),
            "labeled_statement" => one(Dom::of("LabeledStatement", n, self.convs(n))),
            "try_statement" | "try_with_resources_statement" => one(Dom::of("TryStatement", n, self.convs(n))),
            "catch_clause" => one(Dom::of("CatchClause", n, self.convs(n))),
            ";" => Vec::new(),
            "module_declaration" => one(self.decl("ModuleDeclaration", n, self.convs(n))),
            "module_body" => self.convs(n),
            "requires_module_directive" => one(Dom::of("RequiresDirective", n, self.convs(n))),
            "exports_module_directive" => one(Dom::of("ExportsDirective", n, self.convs(n))),
            "opens_module_directive" => one(Dom::of("OpensDirective", n, self.convs(n))),
            "uses_module_directive" => one(Dom::of("UsesDirective", n, self.convs(n))),
            "provides_module_directive" => one(Dom::of("ProvidesDirective", n, self.convs(n))),
            "record_pattern" => one(Dom::of("RecordPattern", n, self.convs(n))),
            "type_pattern" => one(Dom::of("TypePattern", n, vec![Dom::of("SingleVariableDeclaration", n, self.decl_children(n))])),
            "guard" => self.convs(n),
            "string_fragment" | "escape_sequence" | "multiline_string_fragment" => Vec::new(),
            _ if n.is_named() => one(Dom::of("ASTNode", n, self.convs(n))),
            _ => Vec::new(),
        }
    }

    /// An expression position: name chains become `QualifiedName`s.
    fn expr(&self, n: Node) -> Vec<Dom> {
        self.conv(n)
    }

    fn expr_children(&self, n: Node) -> Vec<Dom> {
        named_children(n).into_iter().flat_map(|c| self.expr(c)).collect()
    }

    fn type_children(&self, n: Node) -> Vec<Dom> {
        self.convs(n)
    }

    /// `InfixExpression`, flattening left-nested chains of the same operator
    /// into extended operands like JDT's `ASTConverter`.
    fn infix(&self, n: Node) -> Dom {
        let op = n.child_by_field_name("operator").map(|o| self.text(o)).unwrap_or("");
        let mut operands = Vec::new();
        let mut cur = n;
        let mut rights = Vec::new();
        loop {
            let left = cur.child_by_field_name("left");
            let right = cur.child_by_field_name("right");
            if let Some(r) = right {
                rights.push(r);
            }
            match left {
                Some(l) if l.kind() == "binary_expression" && l.child_by_field_name("operator").map(|o| self.text(o)) == Some(op) => cur = l,
                Some(l) => {
                    operands.push(l);
                    break;
                }
                None => break,
            }
        }
        rights.reverse();
        operands.extend(rights);
        let children = operands.into_iter().flat_map(|o| self.expr(o)).collect();
        Dom::of("InfixExpression", n, children)
    }

    // ── Javadoc ──────────────────────────────────────────────────────────────

    fn javadoc(&self, start: usize, end: usize) -> Dom {
        Dom::new("Javadoc", start, end, javadoc_tags(self.src, start, end))
    }
}

/// `DocCommentParser`-like structure of the Javadoc comment `src[start..end]`.
pub fn javadoc_tags(src: &str, start: usize, end: usize) -> Vec<Dom> {
    let content_start = (start + 3).min(end);
    let content_end = end.saturating_sub(2).max(content_start);
    let b = src.as_bytes();
    let mut tags: Vec<Dom> = Vec::new();
    // Current block tag (description first).
    let mut current = Dom::new("TagElement", usize::MAX, 0, Vec::new());
    let mut line_start = content_start;
    let mut first_line = true;
    while line_start < content_end {
        let mut line_end = line_start;
        while line_end < content_end && b[line_end] != b'\n' && b[line_end] != b'\r' {
            line_end += 1;
        }
        // Margin: whitespace, then stars (not on the first line).
        let mut i = line_start;
        while i < line_end && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        if !first_line {
            while i < line_end && b[i] == b'*' {
                i += 1;
            }
        }
        while i < line_end && (b[i] == b' ' || b[i] == b'\t') {
            i += 1;
        }
        let mut text_end = line_end;
        while text_end > i && (b[text_end - 1] == b' ' || b[text_end - 1] == b'\t') {
            text_end -= 1;
        }
        if i < text_end {
            if b[i] == b'@' && i + 1 < text_end && b[i + 1].is_ascii_alphabetic() {
                if current.start != usize::MAX {
                    tags.push(current);
                }
                let mut j = i + 1;
                while j < text_end && (b[j].is_ascii_alphanumeric() || b[j] == b'.' || b[j] == b'-') {
                    j += 1;
                }
                let name = &src[i + 1..j];
                current = Dom::new("TagElement", i, j, Vec::new());
                let rest_start = j;
                let mut k = j;
                while k < text_end && (b[k] == b' ' || b[k] == b'\t') {
                    k += 1;
                }
                let mut after = rest_start;
                if matches!(name, "param" | "see" | "throws" | "exception" | "uses" | "provides" | "serialField") && k < text_end {
                    if let Some(r) = reference(src, k, text_end, name == "param") {
                        after = r.iter().map(|d| d.end).max().unwrap_or(k);
                        current.children.extend(r);
                    }
                }
                inline_fragments(src, after, text_end, &mut current.children);
            } else {
                if current.start == usize::MAX {
                    current.start = i;
                }
                inline_fragments(src, i, text_end, &mut current.children);
            }
            if let Some(last) = current.children.last() {
                current.end = current.end.max(last.end);
            }
        }
        first_line = false;
        line_start = line_end;
        if line_start < content_end && b[line_start] == b'\r' {
            line_start += 1;
        }
        if line_start < content_end && b[line_start] == b'\n' {
            line_start += 1;
        }
    }
    if current.start != usize::MAX {
        tags.push(current);
    }
    tags
}

/// Text and inline `{@tag ...}` fragments of `src[a..b]`.
fn inline_fragments(src: &str, a: usize, b: usize, out: &mut Vec<Dom>) {
    let bytes = src.as_bytes();
    let mut text_start = a;
    let mut i = a;
    while i < b {
        if bytes[i] == b'{' && i + 1 < b && bytes[i + 1] == b'@' {
            if i > text_start && !src[text_start..i].trim().is_empty() {
                out.push(Dom::new("TextElement", text_start, i, Vec::new()));
            }
            let mut depth = 0;
            let mut j = i;
            while j < b {
                if bytes[j] == b'{' {
                    depth += 1;
                } else if bytes[j] == b'}' {
                    depth -= 1;
                    if depth == 0 {
                        j += 1;
                        break;
                    }
                }
                j += 1;
            }
            let close = j;
            let inner_end = if close <= b && close > i && bytes[close - 1] == b'}' { close - 1 } else { close };
            let mut k = i + 2;
            while k < inner_end && (bytes[k].is_ascii_alphanumeric() || bytes[k] == b'.') {
                k += 1;
            }
            let name = &src[i + 2..k];
            let mut tag = Dom::new("TagElement", i, close, Vec::new());
            let mut after = k;
            if matches!(name, "link" | "linkplain" | "see" | "value") {
                let mut s = k;
                while s < inner_end && (bytes[s] == b' ' || bytes[s] == b'\t') {
                    s += 1;
                }
                if s < inner_end {
                    if let Some(r) = reference(src, s, inner_end, false) {
                        after = r.iter().map(|d| d.end).max().unwrap_or(s);
                        tag.children.extend(r);
                    }
                }
            }
            if after < inner_end && !src[after..inner_end].trim().is_empty() {
                tag.children.push(Dom::new("TextElement", after, inner_end, Vec::new()));
            }
            out.push(tag);
            i = close;
            text_start = close;
            continue;
        }
        i += 1;
    }
    if b > text_start && !src[text_start..b].trim().is_empty() {
        out.push(Dom::new("TextElement", text_start, b, Vec::new()));
    }
}

fn ident_end(src: &str, mut i: usize, b: usize) -> usize {
    let bytes = src.as_bytes();
    while i < b && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'$' || bytes[i] >= 0x80) {
        i += 1;
    }
    i
}

fn name_at(src: &str, a: usize, b: usize) -> Option<(Dom, usize)> {
    let bytes = src.as_bytes();
    let mut i = a;
    let mut cur: Option<Dom> = None;
    loop {
        let e = ident_end(src, i, b);
        if e == i {
            break;
        }
        let simple = Dom::new("SimpleName", i, e, Vec::new());
        cur = Some(match cur {
            None => simple,
            Some(q) => Dom::new("QualifiedName", q.start, e, vec![q, simple]),
        });
        i = e;
        if i + 1 < b && bytes[i] == b'.' && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_') {
            i += 1;
        } else {
            break;
        }
    }
    cur.map(|c| (c, i))
}

/// A Javadoc reference (`java.lang.String`, `Foo#bar`, `#m(Integer, Double)`)
/// or a `@param` name.
fn reference(src: &str, a: usize, b: usize, param: bool) -> Option<Vec<Dom>> {
    let bytes = src.as_bytes();
    if param {
        if bytes[a] == b'<' {
            let (n, e) = name_at(src, a + 1, b)?;
            let mut out = vec![Dom::new("TextElement", a, a + 1, Vec::new()), n];
            if e < b && bytes[e] == b'>' {
                out.push(Dom::new("TextElement", e, e + 1, Vec::new()));
            }
            return Some(out);
        }
        return name_at(src, a, b).map(|(n, _)| vec![n]);
    }
    let (qualifier, mut i) = match name_at(src, a, b) {
        Some((n, e)) => (Some(n), e),
        None => (None, a),
    };
    if i < b && bytes[i] == b'#' {
        let member_start = i + 1;
        let member_end = ident_end(src, member_start, b);
        let mut children: Vec<Dom> = qualifier.into_iter().collect();
        children.push(Dom::new("SimpleName", member_start, member_end, Vec::new()));
        i = member_end;
        if i < b && bytes[i] == b'(' {
            let mut j = i + 1;
            let mut close = None;
            let mut param_start = j;
            while j < b {
                if bytes[j] == b')' || bytes[j] == b',' {
                    let seg = &src[param_start..j];
                    let lead = seg.len() - seg.trim_start().len();
                    let s = param_start + lead;
                    let e = param_start + seg.trim_end().len();
                    if e > s {
                        let (ty, _) = name_at(src, s, e).unwrap_or((Dom::new("SimpleName", s, e, Vec::new()), e));
                        let t = Dom::new("SimpleType", ty.start, ty.end, vec![ty]);
                        children.push(Dom::new("MethodRefParameter", s, e, vec![t]));
                    }
                    if bytes[j] == b')' {
                        close = Some(j + 1);
                        break;
                    }
                    param_start = j + 1;
                }
                j += 1;
            }
            let end = close.unwrap_or(b);
            return Some(vec![Dom::new("MethodRef", a, end, children)]);
        }
        return Some(vec![Dom::new("MemberRef", a, member_end, children)]);
    }
    qualifier.map(|q| vec![q])
}

/// `NodeFinder.perform(root, start, length)`.
pub fn node_finder<'d>(root: &'d Dom, start: usize, length: usize) -> Option<Vec<&'d Dom>> {
    let end = start + length;
    let mut covering: Option<Vec<&Dom>> = None;
    let mut covered: Option<Vec<&Dom>> = None;
    fn visit<'d>(n: &'d Dom, path: &mut Vec<&'d Dom>, start: usize, end: usize, covering: &mut Option<Vec<&'d Dom>>, covered: &mut Option<Vec<&'d Dom>>) {
        if n.end < start || end < n.start {
            return;
        }
        path.push(n);
        let mut descend = true;
        if n.start <= start && end <= n.end {
            *covering = Some(path.clone());
        }
        if start <= n.start && n.end <= end {
            let is_covering = covering.as_ref().is_some_and(|c| std::ptr::eq(*c.last().unwrap(), n));
            if is_covering {
                *covered = Some(path.clone());
            } else {
                if covered.is_none() {
                    *covered = Some(path.clone());
                }
                descend = false;
            }
        }
        if descend {
            for c in &n.children {
                visit(c, path, start, end, covering, covered);
            }
        }
        path.pop();
    }
    let mut path = Vec::new();
    visit(root, &mut path, start, end, &mut covering, &mut covered);
    if let Some(c) = &covered {
        let n = c.last().unwrap();
        if n.start == start && n.end - n.start == length {
            return covered;
        }
    }
    covering
}
