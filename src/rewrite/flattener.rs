//! Port of `ASTRewriteFlattener` (and `ASTRewriteFormatter.ExtendedFlattener`):
//! prints a (new) node with the rewrite's new values, recording markers for
//! placeholders.

use super::{ASTRewrite, Placeholder, RNode, Value};
use crate::semantic_ast::NodeKind;

/// Data of a `NodeMarker`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MarkerData {
    /// Copy / move placeholder: index of the copy source.
    Copy(usize),
    /// String placeholder code.
    Str(String),
}

/// `ASTRewriteFormatter.NodeMarker`: a position in the flattened string.
#[derive(Clone, Debug)]
pub struct NodeMarker {
    pub offset: i32,
    pub length: i32,
    pub data: MarkerData,
}

pub struct Flattener<'r> {
    rw: &'r ASTRewrite,
    /// UTF-16 result.
    result: Vec<u16>,
    /// Record placeholder markers (`ExtendedFlattener`).
    extended: bool,
    pub markers: Vec<NodeMarker>,
}

/// `ASTRewriteFlattener.printModifiers`.
pub fn print_modifiers(flags: i32, buf: &mut String) {
    use crate::semantic_ast::modifier as m;
    let order: [(i32, &str); 13] = [
        (m::PUBLIC, "public "),
        (m::PROTECTED, "protected "),
        (m::PRIVATE, "private "),
        (m::STATIC, "static "),
        (m::ABSTRACT, "abstract "),
        (m::FINAL, "final "),
        (m::SYNCHRONIZED, "synchronized "),
        (m::VOLATILE, "volatile "),
        (m::NATIVE, "native "),
        (m::STRICTFP, "strictfp "),
        (m::TRANSIENT, "transient "),
        (m::SEALED, "sealed "),
        (m::NON_SEALED, "non-sealed "),
    ];
    for (f, s) in order {
        if flags & f != 0 {
            buf.push_str(s);
        }
    }
}

impl<'r> Flattener<'r> {
    pub fn new(rw: &'r ASTRewrite, extended: bool) -> Self {
        Flattener { rw, result: Vec::new(), extended, markers: Vec::new() }
    }

    /// `ASTRewriteFlattener.asString(node, store)`.
    pub fn as_string(rw: &ASTRewrite, node: RNode) -> String {
        let mut f = Flattener::new(rw, false);
        f.accept(node);
        f.result()
    }

    pub fn result(&self) -> String {
        String::from_utf16_lossy(&self.result)
    }

    pub fn len(&self) -> usize {
        self.result.len()
    }

    pub fn is_empty(&self) -> bool {
        self.result.is_empty()
    }

    fn push(&mut self, s: &str) {
        self.result.extend(s.encode_utf16());
    }

    fn attr(&self, n: RNode, prop: &str) -> Value {
        self.rw.new_value(n, prop)
    }

    fn child(&self, n: RNode, prop: &str) -> Option<RNode> {
        self.attr(n, prop).node()
    }

    fn list(&self, n: RNode, prop: &str) -> Vec<RNode> {
        self.attr(n, prop).list()
    }

    fn simple(&self, n: RNode, prop: &str) -> String {
        self.attr(n, prop).simple().unwrap_or("").to_owned()
    }

    fn flag(&self, n: RNode, prop: &str) -> bool {
        self.attr(n, prop).flag()
    }

    /// Visits a required child; a missing one prints JDT's default node.
    fn required(&mut self, n: RNode, prop: &str) {
        match self.child(n, prop) {
            Some(c) => self.accept(c),
            None => {
                let text = match prop {
                    "type" if self.rw.kind(n) == NodeKind::ClassInstanceCreation => "MISSING",
                    "type" | "returnType2" | "elementType" | "rightOperand" if self.rw.kind(n) != NodeKind::InfixExpression => "int",
                    "body" | "finally" => "{}",
                    _ => "MISSING",
                };
                self.push(text);
            }
        }
    }

    fn visit_list(&mut self, n: RNode, prop: &str, separator: Option<&str>) {
        let list = self.list(n, prop);
        for (i, c) in list.into_iter().enumerate() {
            if let (Some(s), true) = (separator, i > 0) {
                self.push(s);
            }
            self.accept(c);
        }
    }

    fn visit_list_wrapped(&mut self, n: RNode, prop: &str, separator: &str, lead: &str, post: &str) {
        let list = self.list(n, prop);
        if list.is_empty() {
            return;
        }
        self.push(lead);
        for (i, c) in list.into_iter().enumerate() {
            if i > 0 {
                self.push(separator);
            }
            self.accept(c);
        }
        self.push(post);
    }

    fn visit_extra_dimensions(&mut self, n: RNode) {
        self.visit_list_wrapped(n, "extraDimensions2", " ", " ", "");
    }

    fn placeholder_data(&self, n: RNode) -> Option<MarkerData> {
        match &self.rw.new_node_data(n)?.placeholder {
            Some(Placeholder::Copy(i)) => Some(MarkerData::Copy(*i)),
            Some(Placeholder::Str(s)) => Some(MarkerData::Str(s.clone())),
            None => None,
        }
    }

    /// `node.accept(flattener)` with `ExtendedFlattener.preVisit/postVisit`.
    pub fn accept(&mut self, n: RNode) {
        let marker = if self.extended { self.placeholder_data(n) } else { None };
        let marker_index = marker.map(|data| {
            self.markers.push(NodeMarker { offset: self.result.len() as i32, length: 0, data });
            self.markers.len() - 1
        });
        self.visit(n);
        if let Some(i) = marker_index {
            self.markers[i].length = self.result.len() as i32 - self.markers[i].offset;
        }
    }

    fn visit(&mut self, n: RNode) {
        use NodeKind::*;
        let kind = self.rw.kind(n);
        match kind {
            AnonymousClassDeclaration => {
                self.push("{");
                self.visit_list(n, "bodyDeclarations", None);
                self.push("}");
            }
            ArrayAccess => {
                self.required(n, "array");
                self.push("[");
                self.required(n, "index");
                self.push("]");
            }
            ArrayCreation => {
                self.push("new ");
                let array_type = self.child(n, "type");
                let (element, dims) = match array_type {
                    Some(t) => (self.child(t, "elementType"), self.list(t, "dimensions")),
                    None => (None, Vec::new()),
                };
                match element {
                    Some(e) => self.accept(e),
                    None => self.push("int"),
                }
                let list = self.list(n, "dimensions");
                for (i, d) in list.iter().enumerate() {
                    if let Some(dim) = dims.get(i) {
                        self.visit_list_wrapped(*dim, "annotations", " ", "", " ");
                    }
                    self.push("[");
                    self.accept(*d);
                    self.push("]");
                }
                for i in list.len()..dims.len() {
                    self.visit_list_wrapped(dims[i], "annotations", " ", "", " ");
                    self.push("[]");
                }
                if let Some(init) = self.child(n, "initializer") {
                    self.accept(init);
                }
            }
            ArrayInitializer => {
                self.push("{");
                self.visit_list(n, "expressions", Some(","));
                self.push("}");
            }
            ArrayType => {
                self.required(n, "elementType");
                self.visit_list_wrapped(n, "dimensions", "", "", "");
            }
            AssertStatement => {
                self.push("assert ");
                self.required(n, "expression");
                if let Some(m) = self.child(n, "message") {
                    self.push(":");
                    self.accept(m);
                }
                self.push(";");
            }
            Assignment => {
                self.required(n, "leftHandSide");
                let op = self.simple(n, "operator");
                self.push(&op);
                self.required(n, "rightHandSide");
            }
            Block => {
                let collapsed = self.extended && self.rw.new_node_data(n).is_some_and(|d| d.collapsed);
                if collapsed {
                    self.visit_list(n, "statements", None);
                } else {
                    self.push("{");
                    self.visit_list(n, "statements", None);
                    self.push("}");
                }
            }
            BooleanLiteral => {
                let v = self.flag(n, "booleanValue");
                self.push(if v { "true" } else { "false" });
            }
            BreakStatement | ContinueStatement => {
                self.push(if kind == BreakStatement { "break" } else { "continue" });
                if let Some(l) = self.child(n, "label") {
                    self.push(" ");
                    self.accept(l);
                }
                self.push(";");
            }
            CaseDefaultExpression => self.push("default"),
            CastExpression => {
                self.push("(");
                self.required(n, "type");
                self.push(")");
                self.required(n, "expression");
            }
            CatchClause => {
                self.push("catch (");
                self.required(n, "exception");
                self.push(")");
                self.required(n, "body");
            }
            CharacterLiteral | StringLiteral | TextBlock => {
                let v = self.simple(n, "escapedValue");
                self.push(&v);
            }
            ClassInstanceCreation => {
                if let Some(e) = self.child(n, "expression") {
                    self.accept(e);
                    self.push(".");
                }
                self.push("new ");
                self.visit_list_wrapped(n, "typeArguments", ",", "<", ">");
                self.required(n, "type");
                self.push("(");
                self.visit_list(n, "arguments", Some(","));
                self.push(")");
                if let Some(d) = self.child(n, "anonymousClassDeclaration") {
                    self.accept(d);
                }
            }
            CompilationUnit => {
                if let Some(m) = self.child(n, "module") {
                    self.accept(m);
                }
                if let Some(p) = self.child(n, "package") {
                    self.accept(p);
                }
                self.visit_list(n, "imports", None);
                self.visit_list(n, "types", None);
            }
            ConditionalExpression => {
                self.required(n, "expression");
                self.push("?");
                self.required(n, "thenExpression");
                self.push(":");
                self.required(n, "elseExpression");
            }
            ConstructorInvocation => {
                self.visit_list_wrapped(n, "typeArguments", ",", "<", ">");
                self.push("this(");
                self.visit_list(n, "arguments", Some(","));
                self.push(");");
            }
            CreationReference => {
                self.required(n, "type");
                self.push("::");
                self.visit_list_wrapped(n, "typeArguments", "", "<", ">");
                self.push("new");
            }
            Dimension => {
                self.visit_list_wrapped(n, "annotations", " ", " ", " ");
                self.push("[]");
            }
            DoStatement => {
                self.push("do ");
                self.required(n, "body");
                self.push(" while (");
                self.required(n, "expression");
                self.push(");");
            }
            EmptyStatement => self.push(";"),
            ExportsDirective | OpensDirective => {
                self.push(if kind == ExportsDirective { "exports " } else { "opens " });
                self.required(n, "name");
                if !self.list(n, "modules").is_empty() {
                    self.push(" to ");
                    self.visit_list_wrapped(n, "modules", ", ", "", "");
                }
                self.push(";");
            }
            ExpressionStatement => {
                self.required(n, "expression");
                self.push(";");
            }
            FieldAccess => {
                self.required(n, "expression");
                self.push(".");
                self.required(n, "name");
            }
            FieldDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "type");
                self.push(" ");
                self.visit_list(n, "fragments", Some(","));
                self.push(";");
            }
            ForStatement => {
                self.push("for (");
                self.visit_list(n, "initializers", Some(","));
                self.push(";");
                if let Some(e) = self.child(n, "expression") {
                    self.accept(e);
                }
                self.push(";");
                self.visit_list(n, "updaters", Some(","));
                self.push(")");
                self.required(n, "body");
            }
            GuardedPattern => {
                self.required(n, "pattern");
                self.push(" when ");
                self.required(n, "expression");
            }
            IfStatement => {
                self.push("if (");
                self.required(n, "expression");
                self.push(")");
                self.required(n, "thenStatement");
                if let Some(e) = self.child(n, "elseStatement") {
                    self.push(" else ");
                    self.accept(e);
                }
            }
            ImportDeclaration => {
                self.push("import ");
                if self.flag(n, "static") {
                    self.push("static ");
                }
                self.required(n, "name");
                if self.flag(n, "onDemand") {
                    self.push(".*");
                }
                self.push(";");
            }
            InfixExpression => {
                self.required(n, "leftOperand");
                self.push(" ");
                let op = self.simple(n, "operator");
                self.push(&op);
                self.push(" ");
                self.required(n, "rightOperand");
                let sep = format!(" {op} ");
                self.visit_list_wrapped(n, "extendedOperands", &sep, &sep, "");
            }
            Initializer => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "body");
            }
            InstanceofExpression => {
                self.required(n, "leftOperand");
                self.push(" instanceof ");
                self.required(n, "rightOperand");
            }
            PatternInstanceofExpression => {
                self.required(n, "leftOperand");
                self.push(" instanceof ");
                if self.child(n, "pattern").is_some() {
                    self.required(n, "pattern");
                } else {
                    self.required(n, "rightOperand");
                }
            }
            IntersectionType => self.visit_list_wrapped(n, "types", " & ", "", ""),
            Javadoc => {
                self.push("/**");
                for t in self.list(n, "tags") {
                    self.push("\n * ");
                    self.accept(t);
                }
                self.push("\n */");
            }
            JavaDocTextElement | TextElement => {
                let v = self.simple(n, "text");
                self.push(&v);
            }
            LabeledStatement => {
                self.required(n, "label");
                self.push(": ");
                self.required(n, "body");
            }
            LambdaExpression => {
                let mut parens = self.flag(n, "parentheses");
                if !parens {
                    let params = self.list(n, "parameters");
                    parens = params.len() != 1 || self.rw.kind(params[0]) != VariableDeclarationFragment;
                }
                if parens {
                    self.push("(");
                }
                self.visit_list(n, "parameters", Some(","));
                if parens {
                    self.push(")");
                }
                self.push("->");
                self.required(n, "body");
            }
            MethodDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.visit_list_wrapped(n, "typeParameters", ",", "<", ">");
                if !self.flag(n, "constructor") {
                    match self.child(n, "returnType2") {
                        Some(rt) => self.accept(rt),
                        None => self.push("void"),
                    }
                    self.push(" ");
                }
                self.required(n, "name");
                if !self.flag(n, "compactConstructor") {
                    self.push("(");
                    if let Some(rt) = self.child(n, "receiverType") {
                        self.accept(rt);
                        self.push(" ");
                        if let Some(q) = self.child(n, "receiverQualifier") {
                            self.accept(q);
                            self.push(".");
                        }
                        self.push("this");
                        if !self.list(n, "parameters").is_empty() {
                            self.push(",");
                        }
                    }
                    self.visit_list(n, "parameters", Some(","));
                    self.push(")");
                }
                self.visit_extra_dimensions(n);
                self.visit_list_wrapped(n, "thrownExceptionTypes", ",", " throws ", "");
                match self.child(n, "body") {
                    None => self.push(";"),
                    Some(b) => self.accept(b),
                }
            }
            ModuleDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "annotations", " ", "", " ");
                if self.flag(n, "open") {
                    self.push("open ");
                }
                self.push("module ");
                self.required(n, "name");
                self.push("{");
                self.visit_list(n, "moduleDirectives", None);
                self.push("}");
            }
            MethodInvocation => {
                if let Some(e) = self.child(n, "expression") {
                    self.accept(e);
                    self.push(".");
                }
                self.visit_list_wrapped(n, "typeArguments", ",", "<", ">");
                self.required(n, "name");
                self.push("(");
                self.visit_list(n, "arguments", Some(","));
                self.push(")");
            }
            NullLiteral | NullPattern => self.push("null"),
            NumberLiteral => {
                let v = self.simple(n, "token");
                self.push(&v);
            }
            PackageDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list(n, "annotations", Some(" "));
                self.push("package ");
                self.required(n, "name");
                self.push(";");
            }
            ParenthesizedExpression => {
                self.push("(");
                self.required(n, "expression");
                self.push(")");
            }
            PostfixExpression => {
                self.required(n, "operand");
                let op = self.simple(n, "operator");
                self.push(&op);
            }
            PrefixExpression => {
                let op = self.simple(n, "operator");
                self.push(&op);
                self.required(n, "operand");
            }
            ProvidesDirective => {
                self.push("provides ");
                self.required(n, "name");
                self.push(" with ");
                self.visit_list_wrapped(n, "implementations", "", ", ", "");
                self.push(";");
            }
            PrimitiveType => {
                self.visit_list_wrapped(n, "annotations", " ", "", " ");
                let v = self.simple(n, "primitiveTypeCode");
                self.push(&v);
            }
            QualifiedName => {
                self.required(n, "qualifier");
                self.push(".");
                self.required(n, "name");
            }
            RecordDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.push("record ");
                self.required(n, "name");
                self.push(" ");
                self.visit_list_wrapped(n, "typeParameters", ",", "<", ">");
                self.push("(");
                self.visit_list(n, "recordComponents", Some(","));
                self.push(")");
                self.push(" ");
                self.visit_list_wrapped(n, "superInterfaceTypes", ",", "implements ", "");
                self.push("{");
                self.visit_list(n, "bodyDeclarations", Some(""));
                self.push("}");
            }
            RequiresDirective => {
                self.push("requires ");
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "name");
                self.push(";");
            }
            ReturnStatement => {
                self.push("return");
                if let Some(e) = self.child(n, "expression") {
                    self.push(" ");
                    self.accept(e);
                }
                self.push(";");
            }
            SimpleName => {
                let v = self.attr(n, "identifier").simple().unwrap_or("MISSING").to_owned();
                self.push(&v);
            }
            SimpleType => {
                self.visit_list_wrapped(n, "annotations", " ", "", " ");
                self.required(n, "name");
            }
            SingleVariableDeclaration => {
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "type");
                let varargs = self.flag(n, "varargs");
                if varargs {
                    self.visit_list_wrapped(n, "varargsAnnotations", " ", "", " ");
                    self.push("...");
                }
                self.push(" ");
                self.required(n, "name");
                self.visit_extra_dimensions(n);
                if let Some(i) = self.child(n, "initializer") {
                    self.push("=");
                    self.accept(i);
                }
            }
            SuperConstructorInvocation => {
                if let Some(e) = self.child(n, "expression") {
                    self.accept(e);
                    self.push(".");
                }
                self.visit_list_wrapped(n, "typeArguments", ",", "<", ">");
                self.push("super(");
                self.visit_list(n, "arguments", Some(","));
                self.push(");");
            }
            SuperFieldAccess => {
                if let Some(q) = self.child(n, "qualifier") {
                    self.accept(q);
                    self.push(".");
                }
                self.push("super.");
                self.required(n, "name");
            }
            SuperMethodInvocation => {
                if let Some(q) = self.child(n, "qualifier") {
                    self.accept(q);
                    self.push(".");
                }
                self.push("super.");
                self.visit_list_wrapped(n, "typeArguments", ",", "<", ">");
                self.required(n, "name");
                self.push("(");
                self.visit_list(n, "arguments", Some(","));
                self.push(")");
            }
            SwitchCase => {
                let exprs = self.list(n, "expression");
                let rule = self.flag(n, "switchLabeledRule");
                if exprs.is_empty() {
                    self.push("default");
                } else {
                    self.push("case ");
                    let len = exprs.len();
                    for (i, e) in exprs.into_iter().enumerate() {
                        self.accept(e);
                        if i + 1 < len {
                            self.push(", ");
                        }
                    }
                }
                self.push(if rule { " ->" } else { ":" });
            }
            SwitchExpression | SwitchStatement => {
                self.push("switch (");
                self.required(n, "expression");
                self.push(")");
                self.push("{");
                self.visit_list(n, "statements", None);
                self.push("}");
            }
            SynchronizedStatement => {
                self.push("synchronized (");
                self.required(n, "expression");
                self.push(")");
                self.required(n, "body");
            }
            ThisExpression => {
                if let Some(q) = self.child(n, "qualifier") {
                    self.accept(q);
                    self.push(".");
                }
                self.push("this");
            }
            ThrowStatement => {
                self.push("throw ");
                self.required(n, "expression");
                self.push(";");
            }
            TryStatement => {
                self.push("try ");
                self.visit_list_wrapped(n, "resources", ";", "(", ")");
                self.push(" ");
                self.required(n, "body");
                self.push(" ");
                self.visit_list(n, "catchClauses", None);
                if let Some(f) = self.child(n, "finally") {
                    self.push(" finally ");
                    self.accept(f);
                }
            }
            ImplicitTypeDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list(n, "bodyDeclarations", None);
            }
            TypeDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                let is_interface = self.flag(n, "interface");
                self.push(if is_interface { "interface " } else { "class " });
                self.required(n, "name");
                self.visit_list_wrapped(n, "typeParameters", ",", "<", ">");
                self.push(" ");
                if let Some(s) = self.child(n, "superclassType") {
                    self.push("extends ");
                    self.accept(s);
                    self.push(" ");
                }
                let lead = if is_interface { "extends " } else { "implements " };
                self.visit_list_wrapped(n, "superInterfaceTypes", ",", lead, "");
                if !self.list(n, "permitsTypes").is_empty() {
                    self.visit_list_wrapped(n, "permitsTypes", ",", lead, "");
                }
                self.push("{");
                self.visit_list(n, "bodyDeclarations", None);
                self.push("}");
            }
            TypeDeclarationStatement => self.required(n, "declaration"),
            TypeLiteral => {
                self.required(n, "type");
                self.push(".class");
            }
            UnionType => self.visit_list_wrapped(n, "types", " | ", "", ""),
            UsesDirective => {
                self.push("uses ");
                self.required(n, "name");
                self.push(";");
            }
            VariableDeclarationExpression | VariableDeclarationStatement => {
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "type");
                self.push(" ");
                self.visit_list(n, "fragments", Some(","));
                if kind == VariableDeclarationStatement {
                    self.push(";");
                }
            }
            VariableDeclarationFragment => {
                self.required(n, "name");
                self.visit_extra_dimensions(n);
                if let Some(i) = self.child(n, "initializer") {
                    self.push("=");
                    self.accept(i);
                }
            }
            WhileStatement => {
                self.push("while (");
                self.required(n, "expression");
                self.push(")");
                self.required(n, "body");
            }
            BlockComment | LineComment => {}
            MemberRef => {
                if let Some(q) = self.child(n, "qualifier") {
                    self.accept(q);
                }
                self.push("#");
                self.required(n, "name");
            }
            MethodRef => {
                if let Some(q) = self.child(n, "qualifier") {
                    self.accept(q);
                }
                self.push("#");
                self.required(n, "name");
                self.push("(");
                self.visit_list(n, "parameters", Some(","));
                self.push(")");
            }
            MethodRefParameter => {
                self.required(n, "type");
                if self.flag(n, "varargs") {
                    self.push("...");
                }
                if let Some(nm) = self.child(n, "name") {
                    self.push(" ");
                    self.accept(nm);
                }
            }
            TagElement => {
                let tag = self.attr(n, "tagName").simple().map(str::to_owned);
                if let Some(t) = &tag {
                    self.push(t);
                }
                for (i, c) in self.list(n, "fragments").into_iter().enumerate() {
                    if i > 0 || tag.is_some() {
                        self.push(" ");
                    }
                    if self.rw.kind(c) == TagElement {
                        self.push("{");
                        self.accept(c);
                        self.push("}");
                    } else {
                        self.accept(c);
                    }
                }
            }
            AnnotationTypeDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.push("@interface ");
                self.required(n, "name");
                self.push("{");
                self.visit_list(n, "bodyDeclarations", Some(""));
                self.push("}");
            }
            AnnotationTypeMemberDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "type");
                self.push(" ");
                self.required(n, "name");
                self.push("()");
                if let Some(d) = self.child(n, "default") {
                    self.push(" default ");
                    self.accept(d);
                }
                self.push(";");
            }
            EnhancedForStatement => {
                self.push("for (");
                self.required(n, "parameter");
                self.push(":");
                self.required(n, "expression");
                self.push(")");
                self.required(n, "body");
            }
            EnumConstantDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "name");
                self.visit_list_wrapped(n, "arguments", ",", "(", ")");
                if let Some(d) = self.child(n, "anonymousClassDeclaration") {
                    self.accept(d);
                }
            }
            EnumDeclaration => {
                if let Some(j) = self.child(n, "javadoc") {
                    self.accept(j);
                }
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.push("enum ");
                self.required(n, "name");
                self.push(" ");
                self.visit_list_wrapped(n, "superInterfaceTypes", ",", "implements ", "");
                self.push("{");
                self.visit_list_wrapped(n, "enumConstants", ",", "", "");
                self.visit_list_wrapped(n, "bodyDeclarations", "", ";", "");
                self.push("}");
            }
            ExpressionMethodReference => {
                self.required(n, "expression");
                self.push("::");
                self.visit_list_wrapped(n, "typeArguments", "", "<", ">");
                self.required(n, "name");
            }
            MarkerAnnotation => {
                self.push("@");
                self.required(n, "typeName");
            }
            MemberValuePair => {
                self.required(n, "name");
                self.push("=");
                self.required(n, "value");
            }
            Modifier | ModuleModifier => {
                let v = self.simple(n, "keyword");
                self.push(&v);
            }
            NormalAnnotation => {
                self.push("@");
                self.required(n, "typeName");
                self.push("(");
                self.visit_list(n, "values", Some(", "));
                self.push(")");
            }
            NameQualifiedType | QualifiedType => {
                self.required(n, "qualifier");
                self.push(".");
                self.visit_list_wrapped(n, "annotations", " ", "", " ");
                self.required(n, "name");
            }
            ParameterizedType => {
                self.required(n, "type");
                self.push("<");
                self.visit_list(n, "typeArguments", Some(", "));
                self.push(">");
            }
            SingleMemberAnnotation => {
                self.push("@");
                self.required(n, "typeName");
                self.push("(");
                self.required(n, "value");
                self.push(")");
            }
            SuperMethodReference => {
                if let Some(q) = self.child(n, "qualifier") {
                    self.accept(q);
                    self.push(".");
                }
                self.push("super ::");
                self.visit_list_wrapped(n, "typeArguments", "", "<", ">");
                self.required(n, "name");
            }
            TypeMethodReference => {
                self.required(n, "type");
                self.push("::");
                self.visit_list_wrapped(n, "typeArguments", "", "<", ">");
                self.required(n, "name");
            }
            TypeParameter => {
                self.visit_list_wrapped(n, "modifiers", " ", "", " ");
                self.required(n, "name");
                self.visit_list_wrapped(n, "typeBounds", " & ", " extends ", "");
            }
            TypePattern => {
                if self.child(n, "patternVariable2").is_some() {
                    self.required(n, "patternVariable2");
                } else {
                    self.required(n, "patternVariable");
                }
            }
            WildcardType => {
                self.visit_list_wrapped(n, "annotations", " ", "", " ");
                self.push("?");
                if let Some(b) = self.child(n, "bound") {
                    self.push(if self.flag(n, "upperBound") { " extends " } else { " super " });
                    self.accept(b);
                }
            }
            YieldStatement => {
                let implicit = self.flag(n, "implicit");
                if !implicit {
                    self.push("yield");
                }
                if let Some(e) = self.child(n, "expression") {
                    if !implicit {
                        self.push(" ");
                    }
                    self.accept(e);
                }
                self.push(";");
            }
            _ => {
                // Unknown node types: fall back to the original source.
                if let RNode::Orig(id) = n {
                    let t = self.rw.ast.node(id).source_text();
                    self.push(&t);
                }
            }
        }
    }
}
