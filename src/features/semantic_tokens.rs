//! Port of jdt.ls `SemanticTokensHandler` / `SemanticTokensVisitor`,
//! `TokenType` and `TokenModifier`.
//!
//! The visitor runs in Rust over the resolved JDT DOM that the bridge returns
//! as data (`astBindings`: nodes in visit order plus the bindings of names).
//! Offsets in that data are Java (UTF-16) offsets.

use tower_lsp::lsp_types::{SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokensLegend};

use super::scanner::{scan_range, LineIndex, TokKind};

// ─── TokenType / TokenModifier ───────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum TokenType {
    Namespace,
    Class,
    Interface,
    Enum,
    EnumMember,
    Type,
    TypeParameter,
    Method,
    Property,
    Variable,
    Parameter,
    Modifier,
    Keyword,
    Annotation,
    AnnotationMember,
    Record,
    RecordComponent,
}

pub const TOKEN_TYPES: &[&str] = &[
    "namespace",
    "class",
    "interface",
    "enum",
    "enumMember",
    "type",
    "typeParameter",
    "method",
    "property",
    "variable",
    "parameter",
    "modifier",
    "keyword",
    "annotation",
    "annotationMember",
    "record",
    "recordComponent",
];

pub const TOKEN_MODIFIERS: &[&str] = &[
    "abstract",
    "static",
    "readonly",
    "deprecated",
    "declaration",
    "documentation",
    "public",
    "private",
    "protected",
    "native",
    "generic",
    "typeArgument",
    "importDeclaration",
    "constructor",
];

pub mod modifier {
    pub const ABSTRACT: u32 = 1 << 0;
    pub const STATIC: u32 = 1 << 1;
    pub const FINAL: u32 = 1 << 2;
    pub const DEPRECATED: u32 = 1 << 3;
    pub const DECLARATION: u32 = 1 << 4;
    pub const DOCUMENTATION: u32 = 1 << 5;
    pub const PUBLIC: u32 = 1 << 6;
    pub const PRIVATE: u32 = 1 << 7;
    pub const PROTECTED: u32 = 1 << 8;
    pub const NATIVE: u32 = 1 << 9;
    pub const GENERIC: u32 = 1 << 10;
    pub const TYPE_ARGUMENT: u32 = 1 << 11;
    pub const IMPORT_DECLARATION: u32 = 1 << 12;
    pub const CONSTRUCTOR: u32 = 1 << 13;
}

/// `SemanticTokensHandler.legend()`.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TOKEN_TYPES.iter().map(|t| SemanticTokenType::new(t)).collect(),
        token_modifiers: TOKEN_MODIFIERS.iter().map(|m| SemanticTokenModifier::new(m)).collect(),
    }
}

// ─── Bridge data ─────────────────────────────────────────────────────────────

// JDT `IBinding` kinds.
const PACKAGE: i64 = 1;
const TYPE: i64 = 2;
const VARIABLE: i64 = 3;
const METHOD: i64 = 4;
const MODULE: i64 = 7;

// Binding flags (see `AstBindingsService`).
mod bf {
    pub const ENUM_CONSTANT: i64 = 1;
    pub const RECORD_COMPONENT: i64 = 2;
    pub const FIELD: i64 = 4;
    pub const PARAMETER: i64 = 8;
    pub const CONSTRUCTOR: i64 = 16;
    pub const ANNOTATION_MEMBER: i64 = 32;
    pub const GENERIC_METHOD: i64 = 64;
    pub const PARAMETERIZED_METHOD: i64 = 128;
    pub const TYPE_VARIABLE: i64 = 256;
    pub const ANNOTATION: i64 = 512;
    pub const RECORD: i64 = 1024;
    pub const INTERFACE: i64 = 2048;
    pub const ENUM: i64 = 4096;
    pub const CLASS: i64 = 8192;
    pub const GENERIC_TYPE: i64 = 16384;
    pub const PARAMETERIZED_TYPE: i64 = 32768;
}

// JDT `Modifier` flags.
mod jm {
    pub const PUBLIC: i64 = 0x1;
    pub const PRIVATE: i64 = 0x2;
    pub const PROTECTED: i64 = 0x4;
    pub const STATIC: i64 = 0x8;
    pub const FINAL: i64 = 0x10;
    pub const NATIVE: i64 = 0x100;
    pub const ABSTRACT: i64 = 0x400;
}

#[derive(Clone, Copy, Debug)]
struct Binding {
    kind: i64,
    modifiers: i64,
    deprecated: bool,
    flags: i64,
    declaring_class: Option<usize>,
}

#[derive(Debug)]
struct Node {
    kind: String,
    start: usize,
    length: usize,
    parent: Option<usize>,
    loc: Option<String>,
    binding: Option<usize>,
    type_binding: Option<usize>,
    ctor_binding: Option<usize>,
    restricted_start: Option<usize>,
    nested: bool,
    comment_root: bool,
    tag_name: Option<String>,
    children: Vec<usize>,
}

pub struct Ast {
    nodes: Vec<Node>,
    bindings: Vec<Binding>,
}

fn opt_index(v: i64) -> Option<usize> {
    (v >= 0).then_some(v as usize)
}

impl Ast {
    /// Decodes the `astBindings` bridge response.
    pub fn from_bridge(strings: &[String], nodes: &[Vec<i64>], bindings: &[Vec<i64>]) -> Ast {
        let s = |i: i64| opt_index(i).and_then(|i| strings.get(i)).cloned();
        let mut out: Vec<Node> = nodes
            .iter()
            .filter(|n| n.len() >= 11)
            .map(|n| Node {
                kind: s(n[0]).unwrap_or_default(),
                start: n[1].max(0) as usize,
                length: n[2].max(0) as usize,
                parent: opt_index(n[3]),
                loc: s(n[4]),
                binding: opt_index(n[5]),
                type_binding: opt_index(n[6]),
                ctor_binding: opt_index(n[7]),
                restricted_start: opt_index(n[8]),
                nested: n[9] & 1 != 0,
                comment_root: n[9] & 2 != 0,
                tag_name: s(n[10]),
                children: Vec::new(),
            })
            .collect();
        for i in 0..out.len() {
            if let Some(p) = out[i].parent {
                if p < out.len() {
                    out[p].children.push(i);
                }
            }
        }
        let bindings = bindings
            .iter()
            .filter(|b| b.len() >= 5)
            .map(|b| Binding { kind: b[0], modifiers: b[1], deprecated: b[2] != 0, flags: b[3], declaring_class: opt_index(b[4]) })
            .collect();
        Ast { nodes: out, bindings }
    }

}

// ─── Binding classification ──────────────────────────────────────────────────

impl Ast {
    fn binding(&self, i: Option<usize>) -> Option<&Binding> {
        i.and_then(|i| self.bindings.get(i))
    }

    /// `TokenType.getApplicableType`.
    fn applicable_type(&self, b: Option<&Binding>) -> Option<TokenType> {
        let b = b?;
        match b.kind {
            VARIABLE => Some(if b.flags & bf::ENUM_CONSTANT != 0 {
                TokenType::EnumMember
            } else if b.flags & bf::RECORD_COMPONENT != 0 {
                TokenType::RecordComponent
            } else if b.flags & bf::FIELD != 0 {
                TokenType::Property
            } else if b.flags & bf::PARAMETER != 0 {
                TokenType::Parameter
            } else {
                TokenType::Variable
            }),
            METHOD => {
                if b.flags & bf::CONSTRUCTOR != 0 {
                    return self.applicable_type(self.binding(b.declaring_class));
                }
                Some(if b.flags & bf::ANNOTATION_MEMBER != 0 { TokenType::AnnotationMember } else { TokenType::Method })
            }
            TYPE => Some(if b.flags & bf::TYPE_VARIABLE != 0 {
                TokenType::TypeParameter
            } else if b.flags & bf::ANNOTATION != 0 {
                TokenType::Annotation
            } else if b.flags & bf::RECORD != 0 {
                TokenType::Record
            } else if b.flags & bf::INTERFACE != 0 {
                TokenType::Interface
            } else if b.flags & bf::ENUM != 0 {
                TokenType::Enum
            } else if b.flags & bf::CLASS != 0 {
                TokenType::Class
            } else {
                TokenType::Type
            }),
            PACKAGE | MODULE => Some(TokenType::Namespace),
            _ => None,
        }
    }

    /// `TokenModifier.checkJavaModifiers`.
    fn java_modifiers(&self, b: Option<&Binding>) -> u32 {
        let Some(b) = b else { return 0 };
        let m = b.modifiers;
        let mut out = 0;
        if m & jm::PUBLIC != 0 {
            out |= modifier::PUBLIC;
        }
        if m & jm::PRIVATE != 0 {
            out |= modifier::PRIVATE;
        }
        if m & jm::PROTECTED != 0 {
            out |= modifier::PROTECTED;
        }
        if m & jm::ABSTRACT != 0 {
            out |= modifier::ABSTRACT;
        }
        if m & jm::STATIC != 0 {
            out |= modifier::STATIC;
        }
        if m & jm::FINAL != 0 {
            out |= modifier::FINAL;
        }
        if m & jm::NATIVE != 0 {
            out |= modifier::NATIVE;
        }
        if b.deprecated {
            out |= modifier::DEPRECATED;
        }
        out
    }

    /// `TokenModifier.checkConstructor`.
    fn constructor(&self, b: Option<&Binding>) -> u32 {
        match b {
            Some(b) if b.kind == METHOD && b.flags & bf::CONSTRUCTOR != 0 => modifier::CONSTRUCTOR,
            _ => 0,
        }
    }

    /// `TokenModifier.checkGeneric`.
    fn generic(&self, b: Option<&Binding>) -> u32 {
        let Some(b) = b else { return 0 };
        match b.kind {
            TYPE if b.flags & (bf::GENERIC_TYPE | bf::PARAMETERIZED_TYPE) != 0 => modifier::GENERIC,
            METHOD => {
                if b.flags & (bf::GENERIC_METHOD | bf::PARAMETERIZED_METHOD) != 0 {
                    modifier::GENERIC
                } else {
                    self.generic(self.binding(b.declaring_class))
                }
            }
            _ => 0,
        }
    }

    /// `TokenModifier.checkDeclaration`.
    fn declaration(&self, name: usize) -> u32 {
        let n = &self.nodes[name];
        let (Some(p), Some(loc)) = (n.parent, n.loc.as_deref()) else { return 0 };
        let declares = matches!(
            self.nodes[p].kind.as_str(),
            "TypeDeclaration"
                | "MethodDeclaration"
                | "SingleVariableDeclaration"
                | "VariableDeclarationFragment"
                | "EnumDeclaration"
                | "EnumConstantDeclaration"
                | "TypeParameter"
                | "AnnotationTypeDeclaration"
                | "AnnotationTypeMemberDeclaration"
                | "RecordDeclaration"
        ) && loc == "name";
        if declares {
            modifier::DECLARATION
        } else {
            0
        }
    }

    fn prop(&self, i: usize, id: &str) -> Vec<usize> {
        self.nodes[i].children.iter().copied().filter(|c| self.nodes[*c].loc.as_deref() == Some(id)).collect()
    }

    fn prop1(&self, i: usize, id: &str) -> Option<usize> {
        self.prop(i, id).into_iter().next()
    }

    fn end(&self, i: usize) -> usize {
        self.nodes[i].start + self.nodes[i].length
    }
}

// ─── Visitor ─────────────────────────────────────────────────────────────────

struct Utf16Text<'a> {
    src: &'a str,
    li: LineIndex,
    /// Byte offset of every UTF-16 offset (`None` for ASCII sources).
    to_byte: Option<Vec<usize>>,
}

impl<'a> Utf16Text<'a> {
    fn new(src: &'a str) -> Self {
        let to_byte = if src.is_ascii() {
            None
        } else {
            let mut v = Vec::with_capacity(src.len() + 1);
            for (b, c) in src.char_indices() {
                for _ in 0..c.len_utf16() {
                    v.push(b);
                }
            }
            v.push(src.len());
            Some(v)
        };
        Utf16Text { src, li: LineIndex::new(src), to_byte }
    }

    fn byte(&self, utf16: usize) -> usize {
        match &self.to_byte {
            None => utf16.min(self.src.len()),
            Some(v) => v.get(utf16).copied().unwrap_or(self.src.len()),
        }
    }

    fn utf16(&self, byte: usize) -> usize {
        match &self.to_byte {
            None => byte,
            Some(_) => self.src[..byte.min(self.src.len())].chars().map(char::len_utf16).sum(),
        }
    }
}

struct Visitor<'a> {
    ast: &'a Ast,
    text: &'a Utf16Text<'a>,
    tokens: Vec<(usize, usize, TokenType, u32)>,
    static_modifiers: u32,
}

impl Visitor<'_> {
    fn add(&mut self, offset: usize, length: usize, t: TokenType, modifiers: u32) {
        self.tokens.push((offset, length, t, modifiers | self.static_modifiers));
    }

    fn add_node(&mut self, i: usize, t: TokenType, modifiers: u32) {
        let n = &self.ast.nodes[i];
        self.add(n.start, n.length, t, modifiers);
    }

    fn accept(&mut self, i: usize) {
        if self.visit(i) {
            let children = self.ast.nodes[i].children.clone();
            for c in children {
                self.accept(c);
            }
        }
        if self.ast.nodes[i].kind == "Javadoc" {
            self.static_modifiers &= !modifier::DOCUMENTATION;
        }
    }

    fn accept_opt(&mut self, i: Option<usize>) {
        if let Some(i) = i {
            self.accept(i);
        }
    }

    fn accept_list(&mut self, list: Vec<usize>) {
        for i in list {
            self.accept(i);
        }
    }

    fn visit(&mut self, i: usize) -> bool {
        let ast = self.ast;
        let n = &ast.nodes[i];
        match n.kind.as_str() {
            "TypeLiteral" => {
                let ty = ast.prop1(i, "type");
                self.accept_opt(ty);
                if let Some(t) = ty {
                    if n.length == ast.nodes[t].length + 6 {
                        let offset = n.start + n.length - 5;
                        self.add(offset, 5, TokenType::Keyword, 0);
                    }
                }
                false
            }
            "Javadoc" => {
                self.static_modifiers |= modifier::DOCUMENTATION;
                true
            }
            "TagElement" => {
                if let Some(tag) = &n.tag_name {
                    let offset = if n.nested { n.start + 1 } else { n.start };
                    let len = tag.encode_utf16().count();
                    self.add(offset, len, TokenType::Keyword, 0);
                }
                self.accept_list(ast.prop(i, "fragments"));
                false
            }
            "PackageDeclaration" => {
                self.accept_opt(ast.prop1(i, "javadoc"));
                self.accept_list(ast.prop(i, "annotations"));
                self.namespace_names(ast.prop1(i, "name"));
                false
            }
            "ImportDeclaration" => {
                self.static_modifiers |= modifier::IMPORT_DECLARATION;
                let binding = ast.binding(n.binding);
                let name = ast.prop1(i, "name");
                if binding.map_or(true, |b| b.kind == PACKAGE) {
                    self.namespace_names(name);
                } else if let Some(name) = name {
                    self.non_package_name_of_import(name);
                }
                self.static_modifiers &= !modifier::IMPORT_DECLARATION;
                false
            }
            "Modifier" => {
                self.add_node(i, TokenType::Modifier, 0);
                false
            }
            "SimpleName" => {
                let b = ast.binding(n.binding);
                if let Some(t) = ast.applicable_type(b) {
                    let m = ast.java_modifiers(b) | ast.constructor(b) | ast.generic(b) | ast.declaration(i);
                    self.add_node(i, t, m);
                }
                false
            }
            "ModuleDeclaration" => {
                let comment = ast.nodes.iter().position(|c| c.comment_root && c.start == n.start);
                self.accept_opt(comment);
                self.accept_list(ast.prop(i, "annotations"));
                self.namespace_names(ast.prop1(i, "name"));
                self.accept_list(ast.prop(i, "moduleDirectives"));
                false
            }
            "RequiresDirective" => {
                self.namespace_names(ast.prop1(i, "name"));
                false
            }
            "ExportsDirective" | "OpensDirective" => {
                self.accept_opt(ast.prop1(i, "name"));
                for m in ast.prop(i, "modules") {
                    self.namespace_names(Some(m));
                }
                false
            }
            "ParameterizedType" => {
                self.accept_opt(ast.prop1(i, "type"));
                for t in ast.prop(i, "typeArguments") {
                    self.type_argument(t);
                }
                false
            }
            "MethodInvocation" => {
                self.accept_opt(ast.prop1(i, "expression"));
                for t in ast.prop(i, "typeArguments") {
                    self.type_argument(t);
                }
                self.accept_opt(ast.prop1(i, "name"));
                self.accept_list(ast.prop(i, "arguments"));
                false
            }
            "ClassInstanceCreation" => {
                self.accept_opt(ast.prop1(i, "expression"));
                for t in ast.prop(i, "typeArguments") {
                    self.type_argument(t);
                }
                let tb = n.type_binding;
                let cb = n.ctor_binding;
                if let Some(ty) = ast.prop1(i, "type") {
                    self.visit_simple_name_of_type(ty, &mut |v, name| {
                        if let Some(t) = v.ast.applicable_type(v.ast.binding(tb)) {
                            let c = v.ast.binding(cb);
                            let m = v.ast.java_modifiers(c) | v.ast.generic(c) | modifier::CONSTRUCTOR;
                            v.add_node(name, t, m);
                        }
                    });
                }
                self.accept_list(ast.prop(i, "arguments"));
                self.accept_opt(ast.prop1(i, "anonymousClassDeclaration"));
                false
            }
            "RecordDeclaration" => {
                self.accept_opt(ast.prop1(i, "javadoc"));
                self.accept_list(ast.prop(i, "modifiers"));
                if let Some(rs) = n.restricted_start {
                    self.add(rs, 6, TokenType::Modifier, 0);
                }
                self.accept_opt(ast.prop1(i, "name"));
                self.accept_list(ast.prop(i, "typeParameters"));
                self.accept_list(ast.prop(i, "recordComponents"));
                self.accept_list(ast.prop(i, "superInterfaceTypes"));
                self.accept_list(ast.prop(i, "bodyDeclarations"));
                false
            }
            "TypeDeclaration" => {
                self.accept_opt(ast.prop1(i, "javadoc"));
                let modifiers = ast.prop(i, "modifiers");
                self.accept_list(modifiers.clone());
                self.type_declaration_gap(i, &modifiers);
                if let Some(rs) = n.restricted_start {
                    self.add(rs, 7, TokenType::Modifier, 0);
                }
                self.accept_list(ast.prop(i, "permitsTypes"));
                self.accept_list(ast.prop(i, "bodyDeclarations"));
                false
            }
            _ => true,
        }
    }

    fn type_declaration_gap(&mut self, i: usize, modifiers: &[usize]) {
        let ast = self.ast;
        let javadoc = ast.prop1(i, "javadoc");
        let gap_start = modifiers
            .last()
            .map(|m| ast.end(*m))
            .or_else(|| javadoc.map(|j| ast.end(j)))
            .unwrap_or(ast.nodes[i].start);
        let super_interfaces = ast.prop(i, "superInterfaceTypes");
        let superclass = ast.prop1(i, "superclassType");
        let name = ast.prop1(i, "name");
        let gap_end = super_interfaces
            .first()
            .map(|s| ast.nodes[*s].start)
            .or_else(|| superclass.map(|s| ast.nodes[s].start))
            .or_else(|| name.map(|n| ast.nodes[n].start))
            .unwrap_or(gap_start);
        if gap_end <= gap_start {
            return;
        }
        let (bs, be) = (self.text.byte(gap_start), self.text.byte(gap_end));
        let toks = scan_range(self.text.src, bs, be);
        for t in toks.into_iter().filter(|t| !t.is_comment()) {
            if t.kind != TokKind::Keyword {
                continue;
            }
            let offset = self.text.utf16(t.start);
            let length = self.text.utf16(t.end) - offset;
            match t.text(self.text.src) {
                "class" | "interface" => {
                    self.add(offset, length, TokenType::Modifier, 0);
                    self.accept_opt(name);
                    self.accept_list(ast.prop(i, "typeParameters"));
                }
                "extends" => {
                    self.add(offset, length, TokenType::Modifier, 0);
                    self.accept_opt(superclass);
                }
                "implements" => {
                    self.add(offset, length, TokenType::Modifier, 0);
                    self.accept_list(super_interfaces.clone());
                }
                _ => {}
            }
        }
    }

    fn non_package_name_of_import(&mut self, name: usize) {
        let ast = self.ast;
        if ast.nodes[name].kind == "SimpleName" {
            self.accept(name);
            return;
        }
        let qualifier = ast.prop1(name, "qualifier");
        if self.has_package_qualifier(name) {
            self.namespace_names(qualifier);
        } else if let Some(q) = qualifier {
            self.non_package_name_of_import(q);
        }
        self.accept_opt(ast.prop1(name, "name"));
    }

    fn has_package_qualifier(&self, qualified: usize) -> bool {
        let ast = self.ast;
        let qualifier_binding = ast.prop1(qualified, "qualifier").and_then(|q| ast.binding(ast.nodes[q].binding));
        match qualifier_binding {
            Some(b) => b.kind == PACKAGE,
            None => ast.binding(ast.nodes[qualified].binding).is_some_and(|b| b.kind == PACKAGE || b.kind == TYPE),
        }
    }

    fn simple_names_of_name(&self, name: Option<usize>, out: &mut Vec<usize>) {
        let Some(name) = name else { return };
        match self.ast.nodes[name].kind.as_str() {
            "SimpleName" => out.push(name),
            "QualifiedName" => {
                self.simple_names_of_name(self.ast.prop1(name, "qualifier"), out);
                if let Some(n) = self.ast.prop1(name, "name") {
                    out.push(n);
                }
            }
            _ => {}
        }
    }

    fn namespace_names(&mut self, name: Option<usize>) {
        let mut names = Vec::new();
        self.simple_names_of_name(name, &mut names);
        for n in names {
            self.add_node(n, TokenType::Namespace, 0);
        }
    }

    fn type_argument(&mut self, ty: usize) {
        self.visit_simple_name_of_type(ty, &mut |v, name| {
            let b = v.ast.binding(v.ast.nodes[name].binding);
            if let Some(t) = v.ast.applicable_type(b) {
                let m = v.ast.java_modifiers(b) | v.ast.generic(b) | modifier::TYPE_ARGUMENT;
                v.add_node(name, t, m);
            }
        });
    }

    fn visit_simple_name_of_type(&mut self, ty: usize, f: &mut dyn FnMut(&mut Self, usize)) {
        let ast = self.ast;
        match ast.nodes[ty].kind.as_str() {
            "SimpleType" => {
                self.accept_list(ast.prop(ty, "annotations"));
                if let Some(name) = ast.prop1(ty, "name") {
                    match ast.nodes[name].kind.as_str() {
                        "SimpleName" => f(self, name),
                        "QualifiedName" => {
                            self.accept_opt(ast.prop1(name, "qualifier"));
                            if let Some(n) = ast.prop1(name, "name") {
                                f(self, n);
                            }
                        }
                        _ => {}
                    }
                }
            }
            "QualifiedType" | "NameQualifiedType" => {
                self.accept_opt(ast.prop1(ty, "qualifier"));
                self.accept_list(ast.prop(ty, "annotations"));
                if let Some(n) = ast.prop1(ty, "name") {
                    f(self, n);
                }
            }
            "ParameterizedType" => {
                if let Some(t) = ast.prop1(ty, "type") {
                    self.visit_simple_name_of_type(t, f);
                }
                for t in ast.prop(ty, "typeArguments") {
                    self.type_argument(t);
                }
            }
            _ => self.accept(ty),
        }
    }
}

/// Runs the visitor over the unit and encodes the tokens
/// (`SemanticTokensVisitor.encodedTokens`).
pub fn semantic_tokens(src: &str, ast: &Ast) -> Vec<SemanticToken> {
    let text = Utf16Text::new(src);
    let mut v = Visitor { ast, text: &text, tokens: Vec::new(), static_modifiers: 0 };
    if let Some(root) = ast.nodes.iter().position(|n| n.parent.is_none() && !n.comment_root) {
        v.accept(root);
    }
    let mut data = Vec::with_capacity(v.tokens.len());
    let mut current_line: i64 = 0;
    let mut current_column: i64 = 0;
    for (offset, length, t, modifiers) in v.tokens {
        let pos = text.li.position(src, text.byte(offset));
        let line = pos.line as i64;
        let column = pos.character as i64;
        let delta_line = line - current_line;
        if delta_line != 0 {
            current_line = line;
            current_column = 0;
        }
        let delta_column = column - current_column;
        current_column = column;
        if delta_line != 0 || delta_column != 0 {
            data.push(SemanticToken {
                delta_line: delta_line as u32,
                delta_start: delta_column as u32,
                length: length as u32,
                token_type: t as u32,
                token_modifiers_bitset: modifiers,
            });
        }
    }
    data
}
