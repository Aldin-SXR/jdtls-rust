//! Port of jdt.ls `JavaLanguageServerTemplateStore`: the code snippet
//! templates followed by the postfix templates, each stored under its id.

use super::snippets::Template;
use std::sync::OnceLock;

/// `JavaLanguageServerTemplateStore.loadContributedTemplates`.
fn store() -> &'static [Template] {
    static STORE: OnceLock<Vec<Template>> = OnceLock::new();
    STORE.get_or_init(|| {
        let mut all = super::snippets::templates();
        all.extend(super::postfix::templates());
        all
    })
}

/// `getTemplates()`.
pub fn templates() -> &'static [Template] {
    store()
}

/// `getTemplates(contextTypeId)`.
pub fn templates_of(context_type: &str) -> Vec<Template> {
    store().iter().filter(|t| t.context_type == context_type).cloned().collect()
}

/// `getTemplateData(id).getTemplate()`.
pub fn template_data(id: &str) -> Option<&'static Template> {
    store().iter().find(|t| t.id == id)
}

/// Port of `org.eclipse.jdt.ls.core.internal.corext.template.java.JavaLanguageServerTemplateStoreTest`.
#[cfg(test)]
mod java_language_server_template_store_test {
    use super::*;

    /// `PostfixTemplate.values()` constant names.
    const POSTFIX_TEMPLATE_NAMES: [&str; 20] = [
        "ASSERT", "CAST", "IF", "ELSE", "FOR", "FORI", "FORR", "FORMAT", "NNULL", "NULL", "NOT", "OPT", "SYSOUT", "SYSOUF", "SYSOUTV",
        "SYSERR", "THROW", "VAR", "PAR", "WHILE",
    ];

    #[test]
    fn test_template_store_content() {
        let templates = templates();
        let snippets = crate::features::completion::snippets::templates();
        let postfixes = crate::features::completion::postfix::templates();

        assert_eq!(templates.len(), snippets.len() + postfixes.len());

        for snippet in &snippets {
            let template_data = template_data(&snippet.id);
            assert!(template_data.is_some(), "{}", snippet.id);
            assert_eq!(template_data.unwrap().name, snippet.name);
        }

        assert_eq!(postfixes.len(), POSTFIX_TEMPLATE_NAMES.len());
        for (postfix, enum_name) in postfixes.iter().zip(POSTFIX_TEMPLATE_NAMES) {
            let template_data = template_data(&postfix.id);
            assert!(template_data.is_some(), "{}", postfix.id);
            assert_eq!(template_data.unwrap().name, enum_name.to_lowercase());
        }
    }
}
