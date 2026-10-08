//! `RefactoringAnalyzeUtil.checkNewSource`: compiles the changed unit and
//! reports the errors it introduces.

use super::Status;
use crate::correction::edit::{cu_tree, Env};
use crate::correction::CuChange;

pub async fn check_new_source(env: &Env<'_>, cu: &mut CuChange) -> Status {
    let mut result = Status::ok();
    let Ok(tree) = cu_tree(env, cu).await else { return result };
    let new_source = String::from_utf16_lossy(&tree.apply(&cu.ast.source));
    let Ok(url) = tower_lsp::lsp_types::Url::parse(&cu.ast.uri) else { return result };
    let mut ctx = env.dispatcher.context_for(Some(&url)).await;
    ctx.files.insert(cu.ast.uri.clone(), new_source);
    let Ok(new_ast) = crate::semantic_ast::fetch_with(env.dispatcher, &cu.ast.uri, ctx).await else { return result };
    for problem in &new_ast.problems {
        let known = cu.ast.problems.iter().any(|old| old.id == problem.id && old.message == problem.message);
        if !known && problem.is_error {
            result.add_error(problem.message.clone());
        }
    }
    result
}
