//! `Preferences.setResourceFilters` and `ProjectsManager.configureFilters`.
//! Resource filters match complete resource names and inherit down the tree;
//! they are separate from Ant-style project-import and classpath exclusions.

use onig::{MatchParam, Regex, RegexOptions, SearchOptions, Syntax, SyntaxOperator};
use std::path::Path;
use std::sync::Arc;

pub const SETTING: &str = "java.project.resourceFilters";
pub const CREATED_BY_JAVA_LANGUAGE_SERVER: &str = "__CREATED_BY_JAVA_LANGUAGE_SERVER__";

#[derive(Debug, Clone, Default)]
pub struct ResourceFilters {
    patterns: Vec<String>,
    matcher: Option<Arc<Regex>>,
}
impl PartialEq for ResourceFilters {
    fn eq(&self, other: &Self) -> bool {
        self.patterns == other.patterns
    }
}
impl Eq for ResourceFilters {}

impl ResourceFilters {
    pub fn jdtls_default() -> Self {
        Self::new(Some(&["node_modules".into(), r"\.git".into()]))
    }
    /// Null clears the preference. Invalid expressions are removed individually.
    pub fn new(patterns: Option<&[String]>) -> Self {
        let patterns: Vec<String> = patterns
            .unwrap_or_default()
            .iter()
            .filter(|pattern| {
                if compile(pattern).is_ok() {
                    true
                } else {
                    tracing::info!("Invalid preference: {SETTING}={pattern}");
                    false
                }
            })
            .cloned()
            .collect();
        let matcher = if patterns.is_empty() {
            None
        } else {
            let expression = format!(
                r"\A(?:{}|{})\z",
                patterns.join("|"),
                CREATED_BY_JAVA_LANGUAGE_SERVER
            );
            compile(&expression).ok().map(Arc::new)
        };
        Self { patterns, matcher }
    }
    pub fn patterns(&self) -> &[String] {
        &self.patterns
    }

    /// `Preferences.updateFrom`: a missing/null list retains the preference,
    /// unlike calling `setResourceFilters(null)` directly.
    pub fn updated_from_settings(&self, settings: &serde_json::Value) -> Self {
        match super::pref_list(settings, SETTING) {
            Some(patterns) => Self::new(Some(&patterns)),
            None => self.clone(),
        }
    }

    /// `Resource.isFiltered`: each name is matched separately, including the
    /// ancestors of a queried resource and the project name.
    pub fn is_filtered(&self, project_root: &Path, resource: &Path) -> bool {
        let Some(matcher) = &self.matcher else {
            return false;
        };
        let Ok(relative) = resource.strip_prefix(project_root) else {
            return false;
        };
        project_root
            .file_name()
            .is_some_and(|name| matches_name(matcher, &name.to_string_lossy()))
            || relative
                .components()
                .any(|component| matches_name(matcher, &component.as_os_str().to_string_lossy()))
    }
}

fn matches_name(matcher: &Regex, name: &str) -> bool {
    match matcher.match_with_param(
        name,
        0,
        SearchOptions::SEARCH_OPTION_WHOLE_STRING,
        None,
        MatchParam::default(),
    ) {
        Ok(result) => result.is_some(),
        Err(error) => {
            tracing::info!("Unable to match resource filter: {error}");
            false
        }
    }
}

fn compile(pattern: &str) -> Result<Regex, onig::Error> {
    let mut syntax = *Syntax::java();
    syntax.enable_operators(
        SyntaxOperator::SYNTAX_OPERATOR_QMARK_LT_NAMED_GROUP
            | SyntaxOperator::SYNTAX_OPERATOR_ESC_K_NAMED_BACKREF,
    );
    // Java's predefined classes are ASCII unless Unicode character classes
    // are explicitly enabled. Retain these newer engine flags, which aren't
    // named by the Rust wrapper yet.
    let options = RegexOptions::from_bits_retain(
        onig_sys::ONIG_OPTION_WORD_IS_ASCII
            | onig_sys::ONIG_OPTION_DIGIT_IS_ASCII
            | onig_sys::ONIG_OPTION_SPACE_IS_ASCII
            | onig_sys::ONIG_OPTION_POSIX_IS_ASCII
            | onig_sys::ONIG_OPTION_CAPTURE_GROUP,
    );
    Regex::with_options(pattern, options, &syntax)
}
