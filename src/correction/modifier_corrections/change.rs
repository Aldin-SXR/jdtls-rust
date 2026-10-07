//! `ModifierChangeCorrectionProposalCore` and `ModifierRewrite`.

use std::sync::Arc;

use crate::correction::edit::Env;
use crate::correction::{kind, Change, CuChange, LazyChange, Proposal};
use crate::rewrite::{ASTRewrite, RNode};
use crate::semantic_ast::{modifier, Ast, Node, NodeKind};

/// `ModifierRewrite.VISIBILITY_MODIFIERS`.
pub const VISIBILITY_MODIFIERS: i32 = modifier::PUBLIC | modifier::PRIVATE | modifier::PROTECTED;

/// `ModifierRewrite.create(rewrite, declNode).setModifiers(included, excluded)`.
pub fn set_modifiers(rw: &mut ASTRewrite, decl: RNode, included: i32, excluded: i32) {
    internal_set_modifiers(rw, decl, included, included | excluded);
}

/// `ModifierRewrite.setModifiers(modifiers)`: sets the given modifiers and
/// removes all others.
pub fn set_all_modifiers(rw: &mut ASTRewrite, decl: RNode, modifiers: i32) {
    internal_set_modifiers(rw, decl, modifiers, -1);
}

/// `ModifierRewrite.internalSetModifiers`.
fn internal_set_modifiers(rw: &mut ASTRewrite, decl: RNode, modifiers: i32, considered: i32) {
    let mut new_modifiers = modifiers & considered;
    let original = rw.original_value(decl, "modifiers").list();
    for curr in original {
        if rw.kind(curr) == NodeKind::Modifier {
            let keyword = match curr {
                RNode::Orig(id) => rw.ast.node(id).simple("keyword").unwrap_or("").to_owned(),
                RNode::New(_) => String::new(),
            };
            let flag = modifier::flag_of(&keyword);
            if considered & flag != 0 {
                if new_modifiers & flag == 0 {
                    rw.list_remove(decl, "modifiers", curr);
                }
                new_modifiers &= !flag;
            }
        }
    }
    let last_annotation = rw
        .list_rewritten(decl, "modifiers")
        .into_iter()
        .filter(|n| rw.kind(*n).is_annotation())
        .last();
    for keyword in modifier::keywords(new_modifiers) {
        let curr = rw.new_modifier(keyword);
        if modifier::flag_of(keyword) & VISIBILITY_MODIFIERS != 0 {
            match last_annotation {
                Some(a) => rw.list_insert_after(decl, "modifiers", curr, a),
                None => rw.list_insert_first(decl, "modifiers", curr),
            }
        } else {
            rw.list_insert_last(decl, "modifiers", curr);
        }
    }
}

/// `ModifierChangeCorrectionProposalCore`: changes the modifiers of the
/// declaration of a binding, in the invocation's unit or in another unit.
pub struct ModifierChange {
    /// The AST of the invocation (`fNode`'s compilation unit).
    pub source: Arc<Ast>,
    /// The target unit when it is not the invocation's unit.
    pub target_uri: Option<String>,
    /// Key of `fBinding`.
    pub binding: String,
    pub included: i32,
    pub excluded: i32,
}

impl ModifierChange {
    /// A quick fix proposal (`modifierChangeCorrectionProposalCoreToT`).
    pub fn proposal(label: String, relevance: i32, change: ModifierChange) -> Proposal {
        Proposal::new(label, kind::QUICK_FIX, relevance, Change::Lazy(Box::new(change)))
    }

    /// An abstract method made static gets a body.
    fn needs_body(&self, decl: Node<'_>) -> bool {
        decl.is(NodeKind::MethodDeclaration)
            && !decl.flag("constructor")
            && decl.child("body").is_none()
            && decl.binding().is_some_and(|b| b.modifiers() & modifier::ABSTRACT != 0)
            && self.included & modifier::STATIC != 0
    }

    /// The method body placeholder (`CodeGeneration.getMethodBodyContent`
    /// with the default return statement).
    async fn body_content(&self, env: &Env<'_>, ast: &Ast, decl: Node<'_>) -> anyhow::Result<Option<String>> {
        let uri = ast.uri.clone();
        let options = env.options(&uri).await;
        let profile = crate::features::accessors::profile(env.dispatcher, &tower_lsp::lsp_types::Url::parse(&uri)?).await;
        let mut body_statement = String::new();
        if let Some(typ) = decl.child("returnType2") {
            let expression = if decl.list("extraDimensions2").is_empty() && typ.is(NodeKind::PrimitiveType) {
                match typ.simple("primitiveTypeCode").unwrap_or("") {
                    "void" => None,
                    "boolean" => Some("false"),
                    _ => Some("0"),
                }
            } else {
                Some("null")
            };
            if let Some(e) = expression {
                body_statement = format!("return {e};");
            }
        }
        let binding = decl.binding();
        let type_name = binding.and_then(|b| b.declaring_class()).map(|c| c.name()).unwrap_or("");
        let method_name = binding.map(|b| b.name()).unwrap_or("");
        crate::correction::unresolved_elements::method_body_content(&profile, &options, false, type_name, method_name, &body_statement)
    }

    /// `getRewrite()`.
    fn rewrite(&self, ast: &Arc<Ast>, decl: Node<'_>, body: Option<String>) -> ASTRewrite {
        let mut rw = ASTRewrite::new(ast.clone());
        let mut decl = decl;
        if decl.is(NodeKind::VariableDeclarationFragment) {
            if let Some(parent) = decl.parent() {
                if parent.is(NodeKind::FieldDeclaration)
                    && parent.list("fragments").len() > 1
                    && parent.parent().is_some_and(|p| p.kind().is_abstract_type_declaration())
                {
                    super::split::rewrite_field_modifiers(&mut rw, parent, decl, self.included, self.excluded);
                    return rw;
                }
                if parent.is(NodeKind::VariableDeclarationStatement)
                    && parent.list("fragments").len() > 1
                    && parent.parent().is_some_and(|p| p.is(NodeKind::Block))
                {
                    super::split::rewrite_statement_modifiers(&mut rw, parent, decl, self.included, self.excluded);
                    return rw;
                }
                decl = parent;
            }
        } else if self.needs_body(decl) {
            // add body
            let block = rw.new_block(Vec::new());
            if let Some(placeholder) = body {
                let todo = rw.create_string_placeholder(&placeholder, NodeKind::ReturnStatement);
                rw.put_list(block, "statements", vec![todo]);
            }
            rw.set(RNode::Orig(decl.id), "body", Some(block));
        }
        set_modifiers(&mut rw, RNode::Orig(decl.id), self.included, self.excluded);
        // add abstract modifier to class if we added abstract modifier to method
        if decl.is(NodeKind::MethodDeclaration) && self.included & modifier::ABSTRACT != 0 {
            if let Some(type_decl) = decl.ancestors().find(|a| a.is(NodeKind::TypeDeclaration)) {
                if type_decl.simple("interface") != Some("true") && type_decl.modifiers() & modifier::ABSTRACT == 0 {
                    set_all_modifiers(&mut rw, RNode::Orig(type_decl.id), type_decl.modifiers() | modifier::ABSTRACT);
                }
            }
        }
        rw
    }
}

/// `CompilationUnit.findDeclaringNode(key)`.
pub fn find_declaring_node<'a>(ast: &'a Ast, key: &str) -> Option<Node<'a>> {
    ast.binding_by_key(key).and_then(|b| b.declaring_node())
}

#[tower_lsp::async_trait]
impl LazyChange for ModifierChange {
    async fn compute(&self, env: &Env<'_>) -> anyhow::Result<Vec<CuChange>> {
        let target = match (find_declaring_node(&self.source, &self.binding), &self.target_uri) {
            (Some(_), _) => self.source.clone(),
            (None, Some(uri)) => crate::semantic_ast::fetch(env.dispatcher, &tower_lsp::lsp_types::Url::parse(uri)?).await?,
            (None, None) => return Ok(Vec::new()),
        };
        let Some(decl) = find_declaring_node(&target, &self.binding) else { return Ok(Vec::new()) };
        let body = if self.needs_body(decl) { self.body_content(env, &target, decl).await? } else { None };
        let rw = self.rewrite(&target, decl, body);
        Ok(vec![CuChange::rewrite(rw)])
    }
}
