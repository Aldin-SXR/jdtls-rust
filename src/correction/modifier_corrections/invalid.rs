//! `ModifierCorrectionSubProcessorCore.getRemoveInvalidModifiersProposal`.

use super::change::ModifierChange;
use crate::correction::{messages, Context, ProblemLocation, Proposal};
use crate::semantic_ast::{modifier as m, problem as p, NodeKind};

fn change(ctx: &Context, key: &str, included: i32, excluded: i32) -> ModifierChange {
    ModifierChange { source: ctx.ast.clone(), target_uri: None, binding: key.to_owned(), included, excluded }
}

pub fn remove_invalid_modifiers(ctx: &Context, problem: &ProblemLocation, proposals: &mut Vec<Proposal>, relevance: i32) {
    let Some(mut selected) = problem.covering_node(ctx.ast()) else { return };
    if selected.is(NodeKind::MethodDeclaration) {
        let Some(name) = selected.child("name") else { return };
        selected = name;
    }
    if !selected.is(NodeKind::SimpleName) {
        return;
    }
    let Some(binding) = selected.binding() else { return };
    let method_name = binding.name();
    let problem_id = problem.problem_id;
    let mut label: Option<String> = None;
    let included = 0;
    let excluded = match problem_id {
        p::CannotHideAnInstanceMethodWithAStaticMethod | p::UnexpectedStaticModifierForMethod => {
            label = Some(messages::format(messages::correction("ModifierCorrectionSubProcessor_changemethodtononstatic_description"), &[method_name]));
            m::STATIC
        }
        p::UnexpectedStaticModifierForField => {
            label = Some(messages::format(messages::correction("ModifierCorrectionSubProcessor_changefieldmodifiertononstatic_description"), &[method_name]));
            m::STATIC
        }
        p::IllegalModifierCombinationFinalVolatileForField => {
            label = Some(messages::correction("ModifierCorrectionSubProcessor_removevolatile_description").to_owned());
            m::VOLATILE
        }
        p::IllegalModifierForInterfaceMethod18 => {
            let mut excluded = !(m::PUBLIC | m::ABSTRACT | m::STRICTFP | m::DEFAULT | m::STATIC);
            if binding.modifiers() & m::ABSTRACT != 0 {
                excluded |= m::STRICTFP;
            }
            excluded
        }
        p::IllegalModifierForInterface => !(m::PUBLIC | m::ABSTRACT | m::STRICTFP),
        p::IllegalModifierForClass => !(m::PUBLIC | m::ABSTRACT | m::FINAL | m::STRICTFP),
        p::IllegalModifierForInterfaceField => !(m::PUBLIC | m::STATIC | m::FINAL),
        p::IllegalModifierForMemberInterface | p::IllegalVisibilityModifierForInterfaceMemberType => !(m::PUBLIC | m::STATIC | m::STRICTFP),
        p::IllegalModifierForMemberClass => !(m::PUBLIC | m::PROTECTED | m::PRIVATE | m::STATIC | m::ABSTRACT | m::FINAL | m::STRICTFP),
        p::IllegalModifierForLocalClass => !(m::ABSTRACT | m::FINAL | m::STRICTFP),
        p::IllegalModifierForArgument | p::IllegalModifierForVariable => !m::FINAL,
        p::IllegalModifierForField => !(m::PUBLIC | m::PROTECTED | m::PRIVATE | m::STATIC | m::FINAL | m::VOLATILE | m::TRANSIENT),
        p::IllegalModifierForMethod => {
            !(m::PUBLIC | m::PROTECTED | m::PRIVATE | m::STATIC | m::ABSTRACT | m::FINAL | m::NATIVE | m::STRICTFP | m::SYNCHRONIZED)
        }
        p::IllegalModifierForConstructor => !(m::PUBLIC | m::PROTECTED | m::PRIVATE),
        p::IllegalModifierForEnum => !(m::PUBLIC | m::STRICTFP),
        p::IllegalModifierForEnumConstant => !0,
        p::IllegalModifierForEnumConstructor => !m::PRIVATE,
        p::IllegalModifierForMemberEnum => !(m::PUBLIC | m::PRIVATE | m::PROTECTED | m::STATIC | m::STRICTFP),
        // Assert.isTrue(false, "not supported")
        _ => return,
    };
    let label = label.unwrap_or_else(|| {
        messages::format(messages::correction("ModifierCorrectionSubProcessor_removeinvalidmodifiers_description"), &[method_name])
    });
    let key = binding.key();
    proposals.push(ModifierChange::proposal(label, relevance, change(ctx, key, included, excluded)));

    if problem_id == p::IllegalModifierCombinationFinalVolatileForField {
        let label = messages::correction("ModifierCorrectionSubProcessor_removefinal_description").to_owned();
        proposals.push(ModifierChange::proposal(label, relevance + 1, change(ctx, key, 0, m::FINAL)));
    }

    if problem_id == p::UnexpectedStaticModifierForField && binding.is_variable() {
        if let Some(decl_class) = binding.declaring_class().filter(|c| c.is_member()) {
            if binding.modifiers() & m::STATIC == 0 {
                let label = messages::correction("ModifierCorrectionSubProcessor_changemodifiertostaticfinal_description").to_owned();
                proposals.push(ModifierChange::proposal(label, relevance + 1, change(ctx, key, m::FINAL, m::VOLATILE)));
            }
            if decl_class.declaring_node().is_some() {
                let label = messages::correction("ModifierCorrectionSubProcessor_addstatictoparenttype_description").to_owned();
                proposals.push(ModifierChange::proposal(label, relevance - 1, change(ctx, decl_class.key(), m::STATIC, 0)));
            }
        }
    }
    if problem_id == p::UnexpectedStaticModifierForMethod && binding.is_method() {
        if let Some(decl_class) = binding.declaring_class().filter(|c| c.is_member()) {
            if decl_class.declaring_node().is_some() {
                let label = messages::correction("ModifierCorrectionSubProcessor_addstatictoparenttype_description").to_owned();
                proposals.push(ModifierChange::proposal(label, relevance - 1, change(ctx, decl_class.key(), m::STATIC, 0)));
            }
        }
    }
}
