//! Port of jdt.ls `DocumentSymbolHandler` (source compilation units):
//! hierarchical `DocumentSymbol`s when the client supports them, flat
//! `SymbolInformation`s otherwise.  Names and details follow
//! `JavaElementLabels` with `ALL_DEFAULT` (and `M_APP_RETURNTYPE` for the
//! detail), kinds follow `SymbolUtils.mapKind`.

use tower_lsp::lsp_types::{
    DocumentSymbol, DocumentSymbolResponse, Location, Range, SymbolInformation, SymbolKind, SymbolTag, Url,
};

use super::client_caps;
use super::java_model::{self, flags, CompilationUnit, FieldDecl, Member, MethodDecl, Span, TypeDecl, TypeKind};
use super::scanner::LineIndex;

pub fn document_symbols(uri: &Url, src: &str) -> DocumentSymbolResponse {
    let cu = java_model::parse(src);
    let ctx = Ctx { src, li: LineIndex::new(src), tags: client_caps::symbol_tags() };
    if client_caps::hierarchical_document_symbols() {
        DocumentSymbolResponse::Nested(ctx.hierarchical(&cu))
    } else {
        let mut out = Vec::new();
        let cu_name = uri.path_segments().and_then(|mut s| s.next_back()).unwrap_or("").to_owned();
        for t in &cu.types {
            ctx.flat_type(uri, t, &cu_name, &mut out);
        }
        DocumentSymbolResponse::Flat(out)
    }
}

struct Ctx<'a> {
    src: &'a str,
    li: LineIndex,
    tags: bool,
}

/// An element of the Java model as seen by the symbol handlers.
enum Elem<'a> {
    Type(&'a TypeDecl),
    Method(&'a MethodDecl),
    Field(&'a FieldDecl),
    Initializer(&'a [TypeDecl]),
}

fn member_elem(m: &Member) -> Elem<'_> {
    match m {
        Member::Type(t) => Elem::Type(t),
        Member::Method(m) => Elem::Method(m),
        Member::Field(f) => Elem::Field(f),
        Member::Initializer(i) => Elem::Initializer(&i.children),
    }
}

pub fn type_label(t: &TypeDecl) -> String {
    if t.anonymous {
        return if t.enum_body {
            "{...}".to_owned()
        } else {
            match &t.anon_super {
                Some(s) => format!("new {s}() {{...}}"),
                None => "new Anonymous".to_owned(),
            }
        };
    }
    let mut s = t.name.clone();
    if !t.type_params.is_empty() {
        s.push('<');
        s.push_str(&t.type_params.join(", "));
        s.push('>');
    }
    s
}

pub fn method_label(m: &MethodDecl) -> String {
    let mut s = format!("{}({})", m.name, m.params.join(", "));
    if !m.type_params.is_empty() {
        s.push_str(" <");
        s.push_str(&m.type_params.join(", "));
        s.push('>');
    }
    s
}

fn type_kind(t: &TypeDecl) -> SymbolKind {
    match t.kind {
        TypeKind::Interface | TypeKind::Annotation => SymbolKind::INTERFACE,
        TypeKind::Enum => SymbolKind::ENUM,
        _ => SymbolKind::CLASS,
    }
}

fn field_kind(f: &FieldDecl) -> SymbolKind {
    if f.enum_constant {
        SymbolKind::ENUM_MEMBER
    } else if f.flags & flags::STATIC != 0 && f.flags & flags::FINAL != 0 {
        SymbolKind::CONSTANT
    } else {
        SymbolKind::FIELD
    }
}

impl Ctx<'_> {
    fn range(&self, s: Span) -> Range {
        self.li.range(self.src, s.0, s.1)
    }

    fn deprecation(&self, f: u32) -> (Option<Vec<SymbolTag>>, Option<bool>) {
        if f & flags::DEPRECATED == 0 {
            (None, None)
        } else if self.tags {
            (Some(vec![SymbolTag::DEPRECATED]), None)
        } else {
            (None, Some(true))
        }
    }

    // ── Hierarchical ─────────────────────────────────────────────────────────

    #[allow(deprecated)]
    fn hierarchical(&self, cu: &CompilationUnit) -> Vec<DocumentSymbol> {
        let mut out = Vec::new();
        if let Some(p) = &cu.package {
            out.push(DocumentSymbol {
                name: p.name.clone(),
                detail: Some(String::new()),
                kind: SymbolKind::PACKAGE,
                tags: None,
                deprecated: None,
                range: self.range(p.source),
                selection_range: self.range(p.name_range),
                children: None,
            });
        }
        out.extend(cu.types.iter().filter_map(|t| self.symbol(&Elem::Type(t))));
        out
    }

    #[allow(deprecated)]
    fn symbol(&self, e: &Elem) -> Option<DocumentSymbol> {
        let (name, detail, kind, source, name_range, fl, children): (String, String, SymbolKind, Span, Span, u32, Vec<Elem>) = match e {
            Elem::Initializer(_) => return None,
            Elem::Type(t) => (
                type_label(t),
                String::new(),
                type_kind(t),
                t.source,
                t.name_range,
                t.flags,
                t.members.iter().map(member_elem).collect(),
            ),
            Elem::Method(m) => (
                method_label(m),
                match (&m.return_type, m.constructor) {
                    (Some(r), false) => format!(" : {r}"),
                    _ => String::new(),
                },
                if m.constructor { SymbolKind::CONSTRUCTOR } else { SymbolKind::METHOD },
                m.source,
                m.name_range,
                m.flags,
                m.children.iter().map(Elem::Type).collect(),
            ),
            Elem::Field(f) => (
                f.name.clone(),
                String::new(),
                field_kind(f),
                f.source,
                f.name_range,
                f.flags,
                f.children.iter().map(Elem::Type).collect(),
            ),
        };
        let (tags, deprecated) = self.deprecation(fl);
        Some(DocumentSymbol {
            name,
            detail: Some(detail),
            kind,
            tags,
            deprecated,
            range: self.range(source),
            selection_range: self.range(name_range),
            children: if children.is_empty() { None } else { Some(children.iter().filter_map(|c| self.symbol(c)).collect()) },
        })
    }

    // ── Flat ─────────────────────────────────────────────────────────────────

    fn flat_children(&self, uri: &Url, children: &[Elem], parent_name: &str, out: &mut Vec<SymbolInformation>) {
        for c in children {
            match c {
                Elem::Type(t) => self.flat_type(uri, t, parent_name, out),
                Elem::Method(m) => {
                    let kids: Vec<Elem> = m.children.iter().map(Elem::Type).collect();
                    self.flat_children(uri, &kids, &m.name, out);
                    let kind = if m.constructor { SymbolKind::CONSTRUCTOR } else { SymbolKind::METHOD };
                    self.push(uri, method_label(m), kind, m.name_range, m.flags, parent_name, out);
                }
                Elem::Field(f) => {
                    let kids: Vec<Elem> = f.children.iter().map(Elem::Type).collect();
                    self.flat_children(uri, &kids, &f.name, out);
                    self.push(uri, f.name.clone(), field_kind(f), f.name_range, f.flags, parent_name, out);
                }
                Elem::Initializer(types) => {
                    let kids: Vec<Elem> = types.iter().map(Elem::Type).collect();
                    self.flat_children(uri, &kids, "", out);
                }
            }
        }
    }

    fn flat_type(&self, uri: &Url, t: &TypeDecl, parent_name: &str, out: &mut Vec<SymbolInformation>) {
        let kids: Vec<Elem> = t.members.iter().map(member_elem).collect();
        self.flat_children(uri, &kids, &t.name, out);
        self.push(uri, type_label(t), type_kind(t), t.name_range, t.flags, parent_name, out);
    }

    #[allow(deprecated, clippy::too_many_arguments)]
    fn push(&self, uri: &Url, name: String, kind: SymbolKind, name_range: Span, fl: u32, container: &str, out: &mut Vec<SymbolInformation>) {
        let (tags, deprecated) = self.deprecation(fl);
        let si = SymbolInformation {
            name,
            kind,
            tags,
            deprecated,
            location: Location { uri: uri.clone(), range: self.range(name_range) },
            container_name: Some(container.to_owned()),
        };
        if !out.contains(&si) {
            out.push(si);
        }
    }
}
