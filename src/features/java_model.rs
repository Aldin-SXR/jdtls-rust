//! A Rust stand-in for the JDT Java model of a compilation unit
//! (`ICompilationUnit.getChildren()` and friends): package declaration, import
//! container, types and their members with JDT-style source and name ranges.
//!
//! It is built from the JDT-like token stream of [`super::scanner`] with a
//! diet, brace-matching parse that mirrors how JDT's `SourceElementParser`
//! recovers on broken input: member bodies are matched by braces, unexpected
//! tokens are skipped.  Source ranges start at the attached Javadoc (if any)
//! and end at the closing `}` / `;`; name ranges cover the identifier.

use super::scanner::{scan, TokKind, Token};

pub mod flags {
    pub const PUBLIC: u32 = 0x0001;
    pub const PRIVATE: u32 = 0x0002;
    pub const PROTECTED: u32 = 0x0004;
    pub const STATIC: u32 = 0x0008;
    pub const FINAL: u32 = 0x0010;
    pub const SYNCHRONIZED: u32 = 0x0020;
    pub const VOLATILE: u32 = 0x0040;
    pub const TRANSIENT: u32 = 0x0080;
    pub const VARARGS: u32 = 0x0080_0000;
    pub const NATIVE: u32 = 0x0100;
    pub const ABSTRACT: u32 = 0x0400;
    pub const STRICTFP: u32 = 0x0800;
    pub const DEPRECATED: u32 = 0x0010_0000;
    pub const DEFAULT: u32 = 0x0001_0000;
    pub const SEALED: u32 = 0x1000_0000;
    pub const NON_SEALED: u32 = 0x0400_0000;
}

pub type Span = (usize, usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeKind {
    Class,
    Interface,
    Enum,
    Record,
    Annotation,
}

#[derive(Clone, Debug)]
pub struct PackageDecl {
    pub name: String,
    pub source: Span,
    pub name_range: Span,
}

#[derive(Clone, Debug)]
pub struct TypeDecl {
    pub kind: TypeKind,
    pub name: String,
    pub name_range: Span,
    pub source: Span,
    pub flags: u32,
    /// Rendered type parameters (`T extends Comparable<T>`).
    pub type_params: Vec<String>,
    pub anonymous: bool,
    /// Simple name of the instantiated type of an anonymous class.
    pub anon_super: Option<String>,
    /// Anonymous body of an enum constant.
    pub enum_body: bool,
    pub members: Vec<Member>,
}

#[derive(Clone, Debug)]
pub struct MethodDecl {
    pub name: String,
    pub name_range: Span,
    pub source: Span,
    pub flags: u32,
    pub constructor: bool,
    /// Rendered parameter types (`String[]`, `int...`).
    pub params: Vec<String>,
    pub type_params: Vec<String>,
    pub return_type: Option<String>,
    /// Local and anonymous types declared in the body.
    pub children: Vec<TypeDecl>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // model data not consumed by every handler yet
pub struct FieldDecl {
    pub name: String,
    pub name_range: Span,
    pub source: Span,
    pub flags: u32,
    pub enum_constant: bool,
    pub record_component: bool,
    pub type_label: Option<String>,
    /// Signature.toString rendering, retaining package qualification.
    pub type_signature: Option<String>,
    pub children: Vec<TypeDecl>,
}

#[derive(Clone, Debug)]
#[allow(dead_code)] // model data not consumed by every handler yet
pub struct InitializerDecl {
    pub source: Span,
    pub flags: u32,
    pub children: Vec<TypeDecl>,
}

#[derive(Clone, Debug)]
pub enum Member {
    Type(TypeDecl),
    Method(MethodDecl),
    Field(FieldDecl),
    Initializer(InitializerDecl),
}

#[derive(Clone, Debug, Default)]
pub struct CompilationUnit {
    pub package: Option<PackageDecl>,
    pub imports: Vec<Span>,
    pub types: Vec<TypeDecl>,
}

impl CompilationUnit {
    pub fn import_container(&self) -> Option<Span> {
        Some((self.imports.first()?.0, self.imports.last()?.1))
    }
}

// ─── Type references ─────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
enum TypeArg {
    Type(TypeRef),
    Wildcard(Option<(bool, TypeRef)>),
}

#[derive(Clone, Debug)]
struct TypeRef {
    /// Name segments with their type arguments.
    segments: Vec<(String, Vec<TypeArg>)>,
    dims: usize,
}

impl TypeRef {
    /// `JavaElementLabels` rendering of the (unresolved) type signature:
    /// simple name of the erasure, type arguments of the last segment, dims.
    fn label(&self) -> String {
        self.render(false)
    }

    fn signature(&self) -> String {
        self.render(true)
    }

    fn render(&self, qualified: bool) -> String {
        let mut s = String::new();
        let first = if qualified {
            0
        } else {
            self.segments.len().saturating_sub(1)
        };
        for (i, (name, args)) in self.segments.iter().enumerate().skip(first) {
            if i > first {
                s.push('.');
            }
            s.push_str(name);
            if !args.is_empty() {
                s.push('<');
                for (i, a) in args.iter().enumerate() {
                    if i > 0 {
                        s.push_str(if qualified { "," } else { ", " });
                    }
                    match a {
                        TypeArg::Type(t) => s.push_str(&t.render(qualified)),
                        TypeArg::Wildcard(None) => s.push('?'),
                        TypeArg::Wildcard(Some((true, t))) => {
                            s.push_str("? extends ");
                            s.push_str(&t.render(qualified));
                        }
                        TypeArg::Wildcard(Some((false, t))) => {
                            s.push_str("? super ");
                            s.push_str(&t.render(qualified));
                        }
                    }
                }
                s.push('>');
            }
        }
        for _ in 0..self.dims {
            s.push_str("[]");
        }
        s
    }
}

const PRIMITIVES: &[&str] = &["boolean", "byte", "char", "short", "int", "long", "float", "double", "void"];

// ─── Parser ──────────────────────────────────────────────────────────────────

struct Parser<'a> {
    src: &'a str,
    toks: Vec<Token>,
    comments: Vec<Token>,
    pos: usize,
}

struct Modifiers {
    flags: u32,
}

pub fn parse(src: &str) -> CompilationUnit {
    let all = scan(src);
    let (comments, toks): (Vec<Token>, Vec<Token>) = all.into_iter().partition(|t| t.is_comment());
    let mut p = Parser { src, toks, comments, pos: 0 };
    p.compilation_unit()
}

impl<'a> Parser<'a> {
    fn len(&self) -> usize {
        self.toks.len()
    }

    fn text(&self, i: usize) -> &'a str {
        match self.toks.get(i) {
            Some(t) => &self.src[t.start..t.end],
            None => "",
        }
    }

    fn is(&self, i: usize, s: &str) -> bool {
        self.toks.get(i).is_some_and(|t| t.kind != TokKind::StringLit && t.kind != TokKind::CharLit && &self.src[t.start..t.end] == s)
    }

    fn is_ident(&self, i: usize) -> bool {
        self.toks.get(i).is_some_and(|t| t.kind == TokKind::Ident)
    }

    fn start_of(&self, i: usize) -> usize {
        self.toks.get(i).map_or(self.src.len(), |t| t.start)
    }

    fn end_of(&self, i: usize) -> usize {
        self.toks.get(i).map_or(self.src.len(), |t| t.end)
    }

    /// Index just after the bracket matching the one at `i`.
    fn skip_balanced(&self, i: usize) -> usize {
        let open = self.text(i);
        let close = match open {
            "{" => "}",
            "(" => ")",
            "[" => "]",
            "<" => ">",
            _ => return i + 1,
        };
        let mut depth = 0usize;
        let mut j = i;
        while j < self.len() {
            if self.is(j, open) {
                depth += 1;
            } else if self.is(j, close) {
                depth -= 1;
                if depth == 0 {
                    return j + 1;
                }
            } else if open == "<" && (self.is(j, ";") || self.is(j, "{") || self.is(j, "(") || self.is(j, "=")) {
                // Not a type argument list after all.
                return j;
            }
            j += 1;
        }
        self.len()
    }

    /// Javadoc attached to the declaration starting at token `first`.
    fn javadoc_before(&self, first: usize) -> Option<Token> {
        let lower = if first == 0 { 0 } else { self.end_of(first - 1) };
        let upper = self.start_of(first);
        self.comments
            .iter()
            .filter(|c| c.start >= lower && c.end <= upper)
            .last()
            .filter(|c| c.kind == TokKind::Javadoc)
            .copied()
    }

    fn decl_start(&self, first: usize) -> (usize, u32) {
        match self.javadoc_before(first) {
            Some(doc) => {
                let deprecated = javadoc_has_deprecated(doc.text(self.src));
                (doc.start, if deprecated { flags::DEPRECATED } else { 0 })
            }
            None => (self.start_of(first), 0),
        }
    }

    /// Skips an annotation at `pos` (which must be `@`, not `@interface`).
    /// Returns whether it was `@Deprecated`.
    fn skip_annotation(&mut self) -> bool {
        self.pos += 1;
        let mut last = "";
        while self.is_ident(self.pos) || self.toks.get(self.pos).is_some_and(|t| t.kind == TokKind::Keyword) {
            last = self.text(self.pos);
            self.pos += 1;
            if self.is(self.pos, ".") {
                self.pos += 1;
            } else {
                break;
            }
        }
        if self.is(self.pos, "(") {
            self.pos = self.skip_balanced(self.pos);
        }
        last == "Deprecated"
    }

    fn modifiers(&mut self) -> Modifiers {
        let mut f = 0;
        loop {
            let t = self.text(self.pos);
            let bit = match t {
                "public" => flags::PUBLIC,
                "private" => flags::PRIVATE,
                "protected" => flags::PROTECTED,
                "static" => flags::STATIC,
                "final" => flags::FINAL,
                "abstract" => flags::ABSTRACT,
                "native" => flags::NATIVE,
                "synchronized" => flags::SYNCHRONIZED,
                "transient" => flags::TRANSIENT,
                "volatile" => flags::VOLATILE,
                "strictfp" => flags::STRICTFP,
                "default" if !self.is(self.pos + 1, ":") && !self.is(self.pos + 1, "->") => flags::DEFAULT,
                "sealed" if self.is_ident(self.pos + 1) || self.is_type_keyword(self.pos + 1) => flags::SEALED,
                "non" if self.is(self.pos + 1, "-") && self.is(self.pos + 2, "sealed") => {
                    self.pos += 2;
                    flags::NON_SEALED
                }
                "@" if !self.is(self.pos + 1, "interface") => {
                    if self.skip_annotation() {
                        f |= flags::DEPRECATED;
                    }
                    continue;
                }
                _ => break,
            };
            f |= bit;
            self.pos += 1;
        }
        Modifiers { flags: f }
    }

    fn is_type_keyword(&self, i: usize) -> bool {
        self.is(i, "class")
            || self.is(i, "interface")
            || self.is(i, "enum")
            || (self.is(i, "@") && self.is(i + 1, "interface"))
            || (self.is(i, "record") && self.is_ident(i + 1) && (self.is(i + 2, "(") || self.is(i + 2, "<")))
    }

    fn compilation_unit(&mut self) -> CompilationUnit {
        let mut cu = CompilationUnit::default();
        while self.pos < self.len() {
            let first = self.pos;
            if self.is(first, ";") {
                self.pos += 1;
                continue;
            }
            let mods = self.modifiers();
            if self.is(self.pos, "package") {
                let (start, _) = self.decl_start(first);
                self.pos += 1;
                let name_start = self.pos;
                while self.pos < self.len() && !self.is(self.pos, ";") && !self.is(self.pos, "{") && !self.is(self.pos, "}") && !self.is_keyword_stop(self.pos) {
                    self.pos += 1;
                }
                let name_end = self.pos;
                let end = if self.is(self.pos, ";") {
                    self.pos += 1;
                    self.end_of(self.pos - 1)
                } else {
                    self.end_of(name_end.saturating_sub(1))
                };
                if name_end > name_start {
                    let name: String = (name_start..name_end).map(|i| self.text(i)).collect();
                    cu.package = Some(PackageDecl {
                        name,
                        source: (start, end),
                        name_range: (self.start_of(name_start), self.end_of(name_end - 1)),
                    });
                }
                continue;
            }
            if self.is(self.pos, "import") {
                let start = self.start_of(self.pos);
                self.pos += 1;
                while self.pos < self.len() && !self.is(self.pos, ";") && !self.is_keyword_stop(self.pos) {
                    self.pos += 1;
                }
                let end = if self.is(self.pos, ";") {
                    self.pos += 1;
                    self.end_of(self.pos - 1)
                } else {
                    self.end_of(self.pos - 1)
                };
                cu.imports.push((start, end));
                continue;
            }
            if self.is_type_keyword(self.pos) {
                let t = self.type_decl(first, mods.flags);
                cu.types.push(t);
                continue;
            }
            if (self.is(self.pos, "module") || (self.is(self.pos, "open") && self.is(self.pos + 1, "module"))) && first == self.pos {
                while self.pos < self.len() && !self.is(self.pos, "{") {
                    self.pos += 1;
                }
                self.pos = self.skip_balanced(self.pos);
                continue;
            }
            if self.pos == first {
                self.pos += 1;
            }
        }
        cu
    }

    fn is_keyword_stop(&self, i: usize) -> bool {
        self.is(i, "import") || self.is(i, "package") || self.is_type_keyword(i) || self.is(i, "public")
    }

    /// Parses a type declaration whose modifiers start at token `first`; `pos`
    /// is at the type keyword.
    fn type_decl(&mut self, first: usize, mod_flags: u32) -> TypeDecl {
        let (start, doc_flags) = self.decl_start(first);
        let kind = match self.text(self.pos) {
            "class" => TypeKind::Class,
            "interface" => TypeKind::Interface,
            "enum" => TypeKind::Enum,
            "@" => {
                self.pos += 1;
                TypeKind::Annotation
            }
            _ => TypeKind::Record,
        };
        self.pos += 1;
        let (name, name_range) = if self.is_ident(self.pos) {
            let r = (self.start_of(self.pos), self.end_of(self.pos));
            self.pos += 1;
            (self.text(self.pos - 1).to_owned(), r)
        } else {
            let p = self.end_of(self.pos.saturating_sub(1));
            (String::new(), (p, p))
        };
        let type_params = if self.is(self.pos, "<") { self.type_parameters() } else { Vec::new() };
        let mut members = Vec::new();
        if kind == TypeKind::Record && self.is(self.pos, "(") {
            let close = self.skip_balanced(self.pos);
            for p in self.params(self.pos + 1, close.saturating_sub(1)) {
                members.push(Member::Field(FieldDecl {
                    name: p.name,
                    name_range: p.name_range,
                    source: p.source,
                    flags: 0,
                    enum_constant: false,
                    record_component: true,
                    type_label: Some(p.label),
                    type_signature: Some(p.signature),
                    children: Vec::new(),
                }));
            }
            self.pos = close;
        }
        // Header: extends / implements / permits.
        while self.pos < self.len() && !self.is(self.pos, "{") && !self.is(self.pos, ";") && !self.is(self.pos, "}") {
            if self.is(self.pos, "<") {
                self.pos = self.skip_balanced(self.pos).max(self.pos + 1);
            } else {
                self.pos += 1;
            }
        }
        let mut end = self.end_of(self.pos.saturating_sub(1));
        if self.is(self.pos, "{") {
            self.pos += 1;
            end = self.body(kind, &name, &mut members);
        } else if self.is(self.pos, ";") {
            self.pos += 1;
            end = self.end_of(self.pos - 1);
        }
        TypeDecl {
            kind,
            name,
            name_range,
            source: (start, end),
            flags: mod_flags | doc_flags,
            type_params,
            anonymous: false,
            anon_super: None,
            enum_body: false,
            members,
        }
    }

    fn type_parameters(&mut self) -> Vec<String> {
        let close = self.skip_balanced(self.pos);
        let mut out = Vec::new();
        let mut i = self.pos + 1;
        let end = close.saturating_sub(1);
        while i < end {
            let save = self.pos;
            self.pos = i;
            while self.is(self.pos, "@") {
                self.skip_annotation();
            }
            let mut label = self.text(self.pos).to_owned();
            self.pos += 1;
            if self.is(self.pos, "extends") {
                self.pos += 1;
                let mut bounds = Vec::new();
                loop {
                    match self.type_ref() {
                        Some(t) => bounds.push(t.label()),
                        None => break,
                    }
                    if self.is(self.pos, "&") {
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if !bounds.is_empty() {
                    label.push_str(" extends ");
                    label.push_str(&bounds.join(" & "));
                }
            }
            out.push(label);
            while self.pos < end && !self.is(self.pos, ",") {
                self.pos += 1;
            }
            i = self.pos + 1;
            self.pos = save;
        }
        self.pos = close;
        out
    }

    fn type_ref(&mut self) -> Option<TypeRef> {
        while self.is(self.pos, "@") && !self.is(self.pos + 1, "interface") {
            self.skip_annotation();
        }
        let mut segments = Vec::new();
        if PRIMITIVES.contains(&self.text(self.pos)) {
            segments.push((self.text(self.pos).to_owned(), Vec::new()));
            self.pos += 1;
        } else {
            loop {
                if !self.is_ident(self.pos) {
                    break;
                }
                let name = self.text(self.pos).to_owned();
                self.pos += 1;
                let args = if self.is(self.pos, "<") { self.type_args() } else { Vec::new() };
                segments.push((name, args));
                if self.is(self.pos, ".") && (self.is_ident(self.pos + 1) || self.is(self.pos + 1, "@")) {
                    self.pos += 1;
                    while self.is(self.pos, "@") {
                        self.skip_annotation();
                    }
                } else {
                    break;
                }
            }
            if segments.is_empty() {
                return None;
            }
        }
        let mut dims = 0;
        loop {
            while self.is(self.pos, "@") && self.is(self.skip_annotation_lookahead(self.pos), "[") {
                self.skip_annotation();
            }
            if self.is(self.pos, "[") && self.is(self.pos + 1, "]") {
                dims += 1;
                self.pos += 2;
            } else {
                break;
            }
        }
        Some(TypeRef { segments, dims })
    }

    fn skip_annotation_lookahead(&self, i: usize) -> usize {
        let mut j = i + 1;
        while self.is_ident(j) {
            j += 1;
            if self.is(j, ".") {
                j += 1;
            } else {
                break;
            }
        }
        if self.is(j, "(") {
            j = self.skip_balanced(j);
        }
        j
    }

    fn type_args(&mut self) -> Vec<TypeArg> {
        // pos at '<'
        self.pos += 1;
        let mut args = Vec::new();
        loop {
            while self.is(self.pos, "@") {
                self.skip_annotation();
            }
            if self.is(self.pos, ">") {
                self.pos += 1;
                break;
            }
            if self.is(self.pos, "?") {
                self.pos += 1;
                if self.is(self.pos, "extends") || self.is(self.pos, "super") {
                    let ext = self.is(self.pos, "extends");
                    self.pos += 1;
                    match self.type_ref() {
                        Some(t) => args.push(TypeArg::Wildcard(Some((ext, t)))),
                        None => args.push(TypeArg::Wildcard(None)),
                    }
                } else {
                    args.push(TypeArg::Wildcard(None));
                }
            } else {
                match self.type_ref() {
                    Some(t) => args.push(TypeArg::Type(t)),
                    None => break,
                }
            }
            if self.is(self.pos, ",") {
                self.pos += 1;
                continue;
            }
            if self.is(self.pos, ">") {
                self.pos += 1;
            }
            break;
        }
        args
    }

    /// Parameters (or record components) between token indices `[a, b)`.
    fn params(&mut self, a: usize, b: usize) -> Vec<Param> {
        let save = self.pos;
        let mut out = Vec::new();
        let mut i = a;
        while i < b {
            // Find the end of this parameter.
            let mut j = i;
            let mut depth = 0i32;
            while j < b {
                match self.text(j) {
                    "(" | "[" | "<" | "{" => depth += 1,
                    ")" | "]" | ">" | "}" => depth -= 1,
                    "," if depth <= 0 => break,
                    _ => {}
                }
                j += 1;
            }
            self.pos = i;
            let first = i;
            loop {
                if self.is(self.pos, "final") {
                    self.pos += 1;
                } else if self.is(self.pos, "@") {
                    self.skip_annotation();
                } else {
                    break;
                }
            }
            if let Some(mut t) = self.type_ref() {
                while self.is(self.pos, "@") {
                    self.skip_annotation();
                }
                let varargs = if self.is(self.pos, "...") {
                    self.pos += 1;
                    true
                } else {
                    false
                };
                if self.is_ident(self.pos) && self.pos < j {
                    let name = self.text(self.pos).to_owned();
                    let name_range = (self.start_of(self.pos), self.end_of(self.pos));
                    self.pos += 1;
                    while self.is(self.pos, "[") && self.is(self.pos + 1, "]") {
                        t.dims += 1;
                        self.pos += 2;
                    }
                    let mut label = t.label();
                    if varargs {
                        label.push_str("...");
                    }
                    out.push(Param {
                        name,
                        name_range,
                        source: (self.start_of(first), self.end_of(j - 1)),
                        label,
                        signature: format!(
                            "{}{}", t.signature(), if varargs { "[]" } else { "" }
                        ),
                        varargs,
                    });
                }
                // `this` receiver parameters are not parameters.
            }
            i = j + 1;
        }
        self.pos = save;
        out
    }

    /// Parses a type body; `pos` is just after `{`.  Returns the end offset.
    fn body(&mut self, kind: TypeKind, type_name: &str, members: &mut Vec<Member>) -> usize {
        if kind == TypeKind::Enum {
            if let Some(end) = self.enum_constants(members) {
                return end;
            }
        }
        while self.pos < self.len() {
            let first = self.pos;
            if self.is(first, "}") {
                self.pos += 1;
                return self.end_of(first);
            }
            if self.is(first, ";") {
                self.pos += 1;
                continue;
            }
            let mods = self.modifiers();
            if self.is(self.pos, "{") {
                let (start, _) = self.decl_start(first);
                let open = self.pos;
                let close = self.skip_balanced(open);
                let children = self.local_types(open + 1, close.saturating_sub(1));
                self.pos = close;
                members.push(Member::Initializer(InitializerDecl {
                    source: (start, self.end_of(close - 1)),
                    flags: mods.flags,
                    children,
                }));
                continue;
            }
            if self.is_type_keyword(self.pos) {
                let t = self.type_decl(first, mods.flags);
                members.push(Member::Type(t));
                continue;
            }
            let type_params = if self.is(self.pos, "<") { self.type_parameters() } else { Vec::new() };
            // Constructor (incl. compact record constructors).
            if self.is_ident(self.pos)
                && (self.is(self.pos + 1, "(") || (kind == TypeKind::Record && self.text(self.pos) == type_name && self.is(self.pos + 1, "{")))
            {
                let name_idx = self.pos;
                self.pos += 1;
                let m = self.method_rest(first, mods.flags, name_idx, type_params, None, true);
                members.push(Member::Method(m));
                continue;
            }
            let before_type = self.pos;
            let Some(t) = self.type_ref() else {
                self.pos = (before_type + 1).max(first + 1);
                if self.is(before_type, "}") {
                    self.pos = before_type;
                }
                continue;
            };
            if !self.is_ident(self.pos) {
                if self.pos == first {
                    self.pos += 1;
                }
                continue;
            }
            let name_idx = self.pos;
            if self.is(name_idx + 1, "(") {
                self.pos += 1;
                let m = self.method_rest(first, mods.flags, name_idx, type_params, Some(t.label()), false);
                members.push(Member::Method(m));
                continue;
            }
            self.fields(first, mods.flags, t, members);
        }
        self.src.len()
    }

    /// Method after its name; `pos` is at `(` (or `{` for compact constructors).
    fn method_rest(&mut self, first: usize, mod_flags: u32, name_idx: usize, type_params: Vec<String>, return_type: Option<String>, constructor: bool) -> MethodDecl {
        let (start, doc_flags) = self.decl_start(first);
        let mut params = Vec::new();
        let mut f = mod_flags | doc_flags;
        if self.is(self.pos, "(") {
            let close = self.skip_balanced(self.pos);
            let ps = self.params(self.pos + 1, close.saturating_sub(1));
            if ps.last().is_some_and(|p| p.varargs) {
                f |= flags::VARARGS;
            }
            params = ps.into_iter().map(|p| p.label).collect();
            self.pos = close;
        }
        // dims, throws, default value
        while self.pos < self.len() && !self.is(self.pos, "{") && !self.is(self.pos, ";") && !self.is(self.pos, "}") {
            if self.is(self.pos, "(") {
                self.pos = self.skip_balanced(self.pos);
            } else {
                self.pos += 1;
            }
        }
        let mut children = Vec::new();
        let end = if self.is(self.pos, "{") {
            let open = self.pos;
            let close = self.skip_balanced(open);
            children = self.local_types(open + 1, close.saturating_sub(1));
            self.pos = close;
            self.end_of(close - 1)
        } else if self.is(self.pos, ";") {
            self.pos += 1;
            self.end_of(self.pos - 1)
        } else {
            self.end_of(self.pos.saturating_sub(1))
        };
        MethodDecl {
            name: self.text(name_idx).to_owned(),
            name_range: (self.start_of(name_idx), self.end_of(name_idx)),
            source: (start, end),
            flags: f,
            constructor,
            params,
            type_params,
            return_type,
            children,
        }
    }

    fn fields(&mut self, first: usize, mod_flags: u32, t: TypeRef, members: &mut Vec<Member>) {
        let (start, doc_flags) = self.decl_start(first);
        let mut pending: Vec<FieldDecl> = Vec::new();
        loop {
            if !self.is_ident(self.pos) {
                break;
            }
            let name_idx = self.pos;
            self.pos += 1;
            let mut ft = t.clone();
            while self.is(self.pos, "[") && self.is(self.pos + 1, "]") {
                ft.dims += 1;
                self.pos += 2;
            }
            let mut children = Vec::new();
            if self.is(self.pos, "=") {
                self.pos += 1;
                let init_start = self.pos;
                let mut depth = 0i32;
                while self.pos < self.len() {
                    match self.text(self.pos) {
                        "(" | "[" | "{" => depth += 1,
                        ")" | "]" => depth -= 1,
                        "}" => {
                            if depth == 0 {
                                break;
                            }
                            depth -= 1
                        }
                        "," | ";" if depth <= 0 => break,
                        _ => {}
                    }
                    self.pos += 1;
                }
                children = self.local_types(init_start, self.pos);
            }
            pending.push(FieldDecl {
                name: self.text(name_idx).to_owned(),
                name_range: (self.start_of(name_idx), self.end_of(name_idx)),
                source: (start, self.end_of(self.pos.saturating_sub(1))),
                flags: mod_flags | doc_flags,
                enum_constant: false,
                record_component: false,
                type_label: Some(ft.label()),
                type_signature: Some(ft.signature()),
                children,
            });
            if self.is(self.pos, ",") {
                self.pos += 1;
                continue;
            }
            break;
        }
        if self.is(self.pos, ";") {
            if let Some(last) = pending.last_mut() {
                last.source.1 = self.end_of(self.pos);
            }
            self.pos += 1;
        } else if pending.is_empty() && self.pos == first {
            self.pos += 1;
        }
        members.extend(pending.into_iter().map(Member::Field));
    }

    /// Enum constants; returns `Some(end)` if the body ended within them.
    fn enum_constants(&mut self, members: &mut Vec<Member>) -> Option<usize> {
        loop {
            let first = self.pos;
            let mut f = 0;
            while self.is(self.pos, "@") {
                if self.skip_annotation() {
                    f |= flags::DEPRECATED;
                }
            }
            if self.is(self.pos, "}") {
                self.pos += 1;
                return Some(self.end_of(self.pos - 1));
            }
            if self.is(self.pos, ";") {
                self.pos += 1;
                return None;
            }
            if self.is(self.pos, ",") {
                self.pos += 1;
                continue;
            }
            if !self.is_ident(self.pos) {
                return None;
            }
            let (start, doc_flags) = self.decl_start(first);
            let name_idx = self.pos;
            self.pos += 1;
            if self.is(self.pos, "(") {
                self.pos = self.skip_balanced(self.pos);
            }
            let mut children = Vec::new();
            if self.is(self.pos, "{") {
                let open = self.pos;
                self.pos += 1;
                let mut body_members = Vec::new();
                let end = self.body(TypeKind::Class, "", &mut body_members);
                children.push(TypeDecl {
                    kind: TypeKind::Class,
                    name: String::new(),
                    name_range: (self.start_of(name_idx), self.end_of(name_idx)),
                    source: (self.start_of(open), end),
                    flags: 0,
                    type_params: Vec::new(),
                    anonymous: true,
                    anon_super: None,
                    enum_body: true,
                    members: body_members,
                });
            }
            members.push(Member::Field(FieldDecl {
                name: self.text(name_idx).to_owned(),
                name_range: (self.start_of(name_idx), self.end_of(name_idx)),
                source: (start, self.end_of(self.pos - 1)),
                flags: f | doc_flags | flags::PUBLIC | flags::STATIC | flags::FINAL,
                enum_constant: true,
                record_component: false,
                type_label: None,
                type_signature: None,
                children,
            }));
            if self.is(self.pos, ",") {
                self.pos += 1;
            }
        }
    }

    /// Local and anonymous types in the token range `[a, b)`.
    fn local_types(&mut self, a: usize, b: usize) -> Vec<TypeDecl> {
        let save = self.pos;
        let mut out = Vec::new();
        let mut i = a;
        while i < b {
            if self.is(i, "new") {
                // new <T>? @A? Name(.Name)* <..>? ( args ) {
                let mut j = i + 1;
                if self.is(j, "<") {
                    j = self.skip_balanced(j);
                }
                while self.is(j, "@") {
                    j = self.skip_annotation_lookahead(j);
                }
                let mut last_ident = None;
                while self.is_ident(j) {
                    last_ident = Some(j);
                    j += 1;
                    if self.is(j, "<") {
                        j = self.skip_balanced(j);
                    }
                    if self.is(j, ".") {
                        j += 1;
                        while self.is(j, "@") {
                            j = self.skip_annotation_lookahead(j);
                        }
                    } else {
                        break;
                    }
                }
                if let Some(name_idx) = last_ident {
                    if self.is(j, "(") {
                        let close = self.skip_balanced(j);
                        if self.is(close, "{") && close < b {
                            out.extend(self.local_types(j + 1, close - 1));
                            self.pos = close + 1;
                            let mut members = Vec::new();
                            let end = self.body(TypeKind::Class, "", &mut members);
                            out.push(TypeDecl {
                                kind: TypeKind::Class,
                                name: String::new(),
                                name_range: (self.start_of(name_idx), self.end_of(name_idx)),
                                source: (self.start_of(i), end),
                                flags: 0,
                                type_params: Vec::new(),
                                anonymous: true,
                                anon_super: Some(self.text(name_idx).to_owned()),
                                enum_body: false,
                                members,
                            });
                            i = self.pos;
                            continue;
                        }
                    }
                }
                i += 1;
                continue;
            }
            let at_statement_start = i == a || self.is(i - 1, "{") || self.is(i - 1, "}") || self.is(i - 1, ";") || self.is(i - 1, ":") || self.is(i - 1, "->");
            if at_statement_start {
                // Local type declaration (with optional modifiers/annotations).
                self.pos = i;
                let mods = self.modifiers();
                if self.is_type_keyword(self.pos) && !(self.is(self.pos, "@") && false) {
                    let t = self.type_decl(i, mods.flags);
                    out.push(t);
                    i = self.pos;
                    continue;
                }
            }
            i += 1;
        }
        self.pos = save;
        out
    }
}

struct Param {
    name: String,
    name_range: Span,
    source: Span,
    label: String,
    signature: String,
    varargs: bool,
}

pub fn javadoc_has_deprecated(doc: &str) -> bool {
    doc.lines().any(|l| {
        let l = l.trim_start();
        let l = l.strip_prefix("/**").unwrap_or(l).trim_start();
        let l = l.trim_start_matches('*').trim_start();
        l.strip_prefix("@deprecated").is_some_and(|rest| rest.is_empty() || !rest.starts_with(|c: char| c.is_alphanumeric()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_members() {
        let src = "package a.b;\nimport x.Y;\nimport z.*;\n/** doc */\npublic class A<T extends Comparable<T>> {\n  int x = 1, y;\n  public static void main(String args[]) { Runnable r = new Runnable() { public void run() {} }; class L {} }\n  static { }\n  A(int... a) {}\n  enum E { X, Y { }, ; void f() {} }\n}\n";
        let cu = parse(src);
        assert_eq!(cu.package.as_ref().unwrap().name, "a.b");
        assert_eq!(cu.imports.len(), 2);
        let a = &cu.types[0];
        assert_eq!(a.name, "A");
        assert_eq!(a.type_params, vec!["T extends Comparable<T>"]);
        assert!(src[a.source.0..].starts_with("/** doc */"));
        let names: Vec<String> = a
            .members
            .iter()
            .map(|m| match m {
                Member::Type(t) => t.name.clone(),
                Member::Method(m) => format!("{}({})", m.name, m.params.join(", ")),
                Member::Field(f) => f.name.clone(),
                Member::Initializer(_) => "<init>".into(),
            })
            .collect();
        assert_eq!(names, vec!["x", "y", "main(String[])", "<init>", "A(int...)", "E"]);
        let Member::Method(main) = &a.members[2] else { panic!() };
        assert_eq!(main.children.len(), 2);
        assert!(main.children[0].anonymous);
        assert_eq!(main.children[1].name, "L");
    }
}
