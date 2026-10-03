//! Rust port of `org.eclipse.jdt.internal.ui.preferences.formatter.ProfileVersionerCore`
//! (jdt.core.manipulation 1.24.100): upgrades formatter profiles written by
//! older Eclipse versions to the current settings version and completes them
//! with the Eclipse defaults.  The key renames below were generated from the
//! upstream source with the constant values substituted.

use std::collections::BTreeMap;

type Settings = BTreeMap<String, String>;

pub const CURRENT_VERSION: i32 = 23;

const JAVA_FORMATTER: &str = "org.eclipse.jdt.core.javaFormatter";
const INSERT: &str = "insert";
const DO_NOT_INSERT: &str = "do not insert";

/// `ProfileVersionerCore.updateAndComplete(oldSettings, version)`.
pub fn update_and_complete(old_settings: &Settings, version: i32) -> Settings {
    let mut new_settings = super::options::eclipse_defaults();
    let mut s = old_settings.clone();
    // `switch (version)` with fall-through; unknown versions only complete.
    if (1..=22).contains(&version) {
        let steps: &[(i32, fn(&mut Settings))] = &[
            (1, version_1_to_2),
            (2, version_2_to_3),
            (3, version_3_to_4),
            (4, version_4_to_5),
            (5, version_5_to_6),
            (6, version_6_to_7),
            (9, version_9_to_10),
            (10, version_10_to_11),
            (11, version_11_to_12),
            (12, version_12_to_13),
            (13, version_13_to_14),
            (14, version_14_to_15),
            (15, version_15_to_16),
            (16, version_16_to_17),
            (17, version_17_to_18),
            (18, version_18_to_19),
            (19, version_19_to_20),
            (20, version_20_to_21),
            (21, version_21_to_22),
            (22, version_22_to_23),
        ];
        for (from, step) in steps {
            if version <= *from {
                step(&mut s);
            }
        }
    }
    for (key, value) in &s {
        if new_settings.contains_key(key) {
            new_settings.insert(key.clone(), value.clone());
        }
    }
    if let Some(v) = s.get(JAVA_FORMATTER) {
        new_settings.insert(JAVA_FORMATTER.to_owned(), v.clone());
    }
    new_settings
}

fn check_and_replace(settings: &mut Settings, old_key: &str, new_keys: &[&str]) {
    let Some(value) = settings.get(old_key).cloned() else { return };
    for new_key in new_keys {
        settings.insert((*new_key).to_owned(), value.clone());
    }
}

fn map_old_value_range_to_new(settings: &mut Settings, old_key: &str, old_values: &[&str], new_key: &str, new_values: &[&str]) {
    let Some(value) = settings.get(old_key).cloned() else { return };
    for (i, old) in old_values.iter().enumerate() {
        if value == *old {
            settings.insert(new_key.to_owned(), new_values[i].to_owned());
        }
    }
}

fn check_and_replace_boolean_with_insert(settings: &mut Settings, old_key: &str, new_key: &str) {
    let Some(value) = settings.get(old_key).cloned() else { return };
    let value = if value == "true" { INSERT } else { DO_NOT_INSERT };
    settings.insert(new_key.to_owned(), value.to_owned());
}

fn version_13_to_14(s: &mut Settings) {
    if s.get("org.eclipse.jdt.core.formatter.comment.indent_root_tags").map(String::as_str) == Some("false") {
        s.insert("org.eclipse.jdt.core.formatter.comment.indent_parameter_description".to_owned(), "false".to_owned());
    }
    s.insert("org.eclipse.jdt.core.formatter.comment.align_tags_descriptions_grouped".to_owned(), "false".to_owned());
}

fn version_14_to_15(s: &mut Settings) {
    let transitions: &[(&str, &str)] = &[
        ("org.eclipse.jdt.core.formatter.keep_annotation_declaration_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_annotation_declaration"),
        ("org.eclipse.jdt.core.formatter.keep_anonymous_type_declaration_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_anonymous_type_declaration"),
        ("org.eclipse.jdt.core.formatter.keep_if_then_body_block_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_block"),
        ("org.eclipse.jdt.core.formatter.keep_loop_body_block_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_block"),
        ("org.eclipse.jdt.core.formatter.keep_lambda_body_block_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_block"),
        ("org.eclipse.jdt.core.formatter.keep_code_block_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_block"),
        ("org.eclipse.jdt.core.formatter.keep_enum_constant_declaration_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_enum_constant"),
        ("org.eclipse.jdt.core.formatter.keep_enum_declaration_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_enum_declaration"),
        ("org.eclipse.jdt.core.formatter.keep_method_body_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_method_body"),
        ("org.eclipse.jdt.core.formatter.keep_type_declaration_on_one_line", "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_type_declaration"),
    ];
    for (keep, insert_new_line) in transitions {
        if s.get(*insert_new_line).map(String::as_str) == Some(DO_NOT_INSERT) {
            s.insert((*keep).to_owned(), "one_line_if_empty".to_owned());
        }
    }
}

fn version_20_to_21(s: &mut Settings) {
    let derivations: &[(&str, &str)] = &[
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_enum_constant", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_enum_constant"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_field", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_field"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_method", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_method"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_package", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_package"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_type", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_type"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_type_annotation", "org.eclipse.jdt.core.formatter.alignment_for_type_annotations"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_parameter", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_parameter"),
        ("org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_local_variable", "org.eclipse.jdt.core.formatter.alignment_for_annotations_on_local_variable"),
    ];
    for (insert_new_line, alignment) in derivations {
        let split = s.get(*insert_new_line).map(String::as_str) == Some(INSERT);
        // createAlignmentValue(split, split ? WRAP_ONE_PER_LINE : WRAP_NO_SPLIT)
        let wrap = if split { "49" } else { "0" };
        s.insert((*alignment).to_owned(), wrap.to_owned());
    }
}

// ── Generated from ProfileVersionerCore ──────────────────────────────────────

fn version_1_to_2(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_within_message_send", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_message_send", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_message_send"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_open_paren_in_parenthesized_expression", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_parenthesized_expression"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.inset_space_between_empty_arguments", &["org.eclipse.jdt.core.formatter.insert_space_between_empty_arguments"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_method_declaration_open_paren", &["org.eclipse.jdt.core.formatter.insert_space_before_constructor_declaration_open_paren"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_open_paren_in_parenthesized_expression", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_parenthesized_expression"]);
}

fn version_2_to_3(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.array_initializer_continuation_indentation", &["org.eclipse.jdt.core.formatter.continuation_indentation_for_array_initializer"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_block_close_brace", &["org.eclipse.jdt.core.formatter.insert_space_after_closing_brace_in_block"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_in_catch_expression", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_catch", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_catch"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_in_for_parens", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_for", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_for"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_in_if_condition", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_if", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_if"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_in_switch_condition", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_switch", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_switch"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_in_synchronized_condition", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_synchronized", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_synchronized"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_in_while_condition", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_while", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_while"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_constructor_arguments", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_constructor_declaration_parameters"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_messagesend_arguments", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_invocation_arguments"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_arguments", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_declaration_parameters"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_arguments", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_declaration_parameters"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_throws", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_declaration_throws"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_throws", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_declaration_throws"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_message_send", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_message_send", &["org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_constructor_arguments", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_constructor_declaration_parameters"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_constructor_throws", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_constructor_declaration_throws"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_constructor_throws", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_constructor_declaration_throws"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_explicitconstructorcall_arguments", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_explicitconstructorcall_arguments"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_explicitconstructorcall_arguments", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_explicitconstructorcall_arguments"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_messagesend_arguments", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_invocation_arguments"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_arguments", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_declaration_parameters"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_first_argument", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_constructor_declaration", "org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_method_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_message_send", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_anonymous_type_open_brace", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_anonymous_type_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_block_open_brace", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_block"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_catch_expression", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_catch"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_method_open_brace", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_method_declaration", "org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_constructor_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_constructor_declaration_open_paren", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_constructor_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_first_initializer", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_brace_in_array_initializer"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_for_paren", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_for"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_if_condition", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_if"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_message_send", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren", &["org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_method_declaration", "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_constructor_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_method_declaration_open_paren", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_method_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_open_paren_in_parenthesized_expression", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_parenthesized_expression"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_switch_condition", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_switch"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_switch_open_brace", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_switch"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_synchronized_condition", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_synchronized"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_type_open_brace", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_type_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_while_condition", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_while"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_between_brackets_in_array_reference", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_bracket_in_array_reference", "org.eclipse.jdt.core.formatter.insert_space_before_closing_bracket_in_array_reference"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_between_empty_arguments", &["org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_method_declaration", "org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_constructor_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_between_empty_array_initializer", &["org.eclipse.jdt.core.formatter.insert_space_between_empty_braces_in_array_initializer"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_between_empty_messagesend_arguments", &["org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.format_guardian_clause_on_one_line", &["org.eclipse.jdt.core.formatter.format_guardian_clause_on_one_line"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_bracket_in_array_reference", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_bracket_in_array_reference"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_bracket_in_array_type_reference", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_bracket_in_array_type_reference"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_assignment_operators", &["org.eclipse.jdt.core.formatter.insert_space_before_assignment_operator"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_assignment_operators", &["org.eclipse.jdt.core.formatter.insert_space_after_assignment_operator"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.allocation_expression_arguments_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_arguments_in_allocation_expression"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.compact_if_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_compact_if"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.message_send_arguments_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_arguments_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.qualified_allocation_expression_arguments_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_arguments_in_qualified_allocation_expression"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.binary_expression_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_binary_expression"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.compact_if_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_compact_if"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.conditional_expression_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_conditional_expression"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.array_initializer_expressions_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_expressions_in_array_initializer"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.method_declaration_arguments_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_parameters_in_constructor_declaration", "org.eclipse.jdt.core.formatter.alignment_for_parameters_in_method_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.message_send_selector_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_selector_in_method_invocation"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.type_declaration_superclass_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_superclass_in_type_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.type_declaration_superinterfaces_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_type_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.method_throws_clause_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_throws_clause_in_method_declaration", "org.eclipse.jdt.core.formatter.alignment_for_throws_clause_in_constructor_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.explicit_constructor_arguments_alignment", &["org.eclipse.jdt.core.formatter.alignment_for_arguments_in_explicit_constructor_call"]);
    map_old_value_range_to_new(s, "org.eclipse.jdt.core.formatter.type_member_alignment", &["0", "256"], "org.eclipse.jdt.core.formatter.align_type_members_on_columns", &["false", "true"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.anonymous_type_declaration_brace_position", &["org.eclipse.jdt.core.formatter.brace_position_for_anonymous_type_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.array_initializer_brace_position", &["org.eclipse.jdt.core.formatter.brace_position_for_array_initializer"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.block_brace_position", &["org.eclipse.jdt.core.formatter.brace_position_for_block"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.method_declaration_brace_position", &["org.eclipse.jdt.core.formatter.brace_position_for_method_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.type_declaration_brace_position", &["org.eclipse.jdt.core.formatter.brace_position_for_type_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.switch_brace_position", &["org.eclipse.jdt.core.formatter.brace_position_for_switch"]);
}

fn version_3_to_4(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.align_type_members_on_columns", &["org.eclipse.jdt.core.formatter.align_type_members_on_columns"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma__in_superinterfaces", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_superinterfaces"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma__in_superinterfaces", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_superinterfaces"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_between_empty_arguments_in_method_invocation", &["org.eclipse.jdt.core.formatter.insert_space_between_empty_parens_in_method_invocation"]);
}

fn version_4_to_5(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.indent_block_statements", &["org.eclipse.jdt.core.formatter.indent_statements_compare_to_body", "org.eclipse.jdt.core.formatter.indent_statements_compare_to_block"]);
}

fn version_5_to_6(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_new_line_in_control_statements", &["org.eclipse.jdt.core.formatter.insert_new_line_before_else_in_if_statement", "org.eclipse.jdt.core.formatter.insert_new_line_before_catch_in_try_statement", "org.eclipse.jdt.core.formatter.insert_new_line_before_finally_in_try_statement", "org.eclipse.jdt.core.formatter.insert_new_line_before_while_in_do_statement"]);
}

fn version_6_to_7(s: &mut Settings) {
    check_and_replace(s, "comment_format_comments", &["org.eclipse.jdt.core.formatter.comment.format_comments"]);
    check_and_replace(s, "comment_format_header", &["org.eclipse.jdt.core.formatter.comment.format_header"]);
    check_and_replace(s, "comment_format_source_code", &["org.eclipse.jdt.core.formatter.comment.format_source_code"]);
    check_and_replace(s, "comment_indent_parameter_description", &["org.eclipse.jdt.core.formatter.comment.indent_parameter_description"]);
    check_and_replace(s, "comment_indent_root_tags", &["org.eclipse.jdt.core.formatter.comment.indent_root_tags"]);
    check_and_replace(s, "comment_line_length", &["org.eclipse.jdt.core.formatter.comment.line_length"]);
    check_and_replace(s, "comment_clear_blank_lines", &["org.eclipse.jdt.core.formatter.comment.clear_blank_lines"]);
    check_and_replace(s, "comment_format_html", &["org.eclipse.jdt.core.formatter.comment.format_html"]);
    check_and_replace_boolean_with_insert(s, "comment_new_line_for_parameter", "org.eclipse.jdt.core.formatter.comment.insert_new_line_for_parameter");
    check_and_replace_boolean_with_insert(s, "comment_separate_root_tags", "org.eclipse.jdt.core.formatter.comment.insert_new_line_before_root_tags");
}

fn version_9_to_10(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_new_line_in_empty_type_declaration", &["org.eclipse.jdt.core.formatter.insert_new_line_in_empty_annotation_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_type_header", &["org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_annotation_declaration_header"]);
}

fn version_10_to_11(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.comment.format_comments", &["org.eclipse.jdt.core.formatter.comment.format_block_comments", "org.eclipse.jdt.core.formatter.comment.format_javadoc_comments", "org.eclipse.jdt.core.formatter.comment.format_line_comments"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.comment.clear_blank_lines", &["org.eclipse.jdt.core.formatter.comment.clear_blank_lines_in_block_comment", "org.eclipse.jdt.core.formatter.comment.clear_blank_lines_in_javadoc_comment"]);
}

fn version_11_to_12(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation", &["org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_member", "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_local_variable", "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_parameter"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_member", &["org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_field", "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_method", "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_package", "org.eclipse.jdt.core.formatter.insert_new_line_after_annotation_on_type"]);
}

fn version_12_to_13(s: &mut Settings) {
    s.insert("org.eclipse.jdt.core.formatter.comment.count_line_length_from_starting_position".to_owned(), "false".to_owned());
}

fn version_15_to_16(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.alignment_for_binary_expression", &["org.eclipse.jdt.core.formatter.alignment_for_multiplicative_operator", "org.eclipse.jdt.core.formatter.alignment_for_additive_operator", "org.eclipse.jdt.core.formatter.alignment_for_string_concatenation", "org.eclipse.jdt.core.formatter.alignment_for_bitwise_operator", "org.eclipse.jdt.core.formatter.alignment_for_logical_operator"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.wrap_before_binary_operator", &["org.eclipse.jdt.core.formatter.wrap_before_multiplicative_operator", "org.eclipse.jdt.core.formatter.wrap_before_additive_operator", "org.eclipse.jdt.core.formatter.wrap_before_string_concatenation", "org.eclipse.jdt.core.formatter.wrap_before_bitwise_operator", "org.eclipse.jdt.core.formatter.wrap_before_logical_operator"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_binary_operator", &["org.eclipse.jdt.core.formatter.insert_space_before_multiplicative_operator", "org.eclipse.jdt.core.formatter.insert_space_before_additive_operator", "org.eclipse.jdt.core.formatter.insert_space_before_string_concatenation", "org.eclipse.jdt.core.formatter.insert_space_before_shift_operator", "org.eclipse.jdt.core.formatter.insert_space_before_relational_operator", "org.eclipse.jdt.core.formatter.insert_space_before_bitwise_operator", "org.eclipse.jdt.core.formatter.insert_space_before_logical_operator"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_binary_operator", &["org.eclipse.jdt.core.formatter.insert_space_after_multiplicative_operator", "org.eclipse.jdt.core.formatter.insert_space_after_additive_operator", "org.eclipse.jdt.core.formatter.insert_space_after_string_concatenation", "org.eclipse.jdt.core.formatter.insert_space_after_shift_operator", "org.eclipse.jdt.core.formatter.insert_space_after_relational_operator", "org.eclipse.jdt.core.formatter.insert_space_after_bitwise_operator", "org.eclipse.jdt.core.formatter.insert_space_after_logical_operator"]);
}

fn version_16_to_17(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.blank_lines_before_method", &["org.eclipse.jdt.core.formatter.blank_lines_before_abstract_method"]);
}

fn version_17_to_18(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_unary_operator", &["org.eclipse.jdt.core.formatter.insert_space_after_not_operator"]);
}

fn version_18_to_19(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_type_header", &["org.eclipse.jdt.core.formatter.indent_body_declarations_compare_to_record_header"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.brace_position_for_type_declaration", &["org.eclipse.jdt.core.formatter.brace_position_for_record_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.brace_position_for_constructor_declaration", &["org.eclipse.jdt.core.formatter.brace_position_for_record_constructor"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.parentheses_positions_in_method_delcaration", &["org.eclipse.jdt.core.formatter.parentheses_positions_in_record_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_method_declaration", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_paren_in_record_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_method_declaration", &["org.eclipse.jdt.core.formatter.insert_space_after_opening_paren_in_record_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_method_declaration", &["org.eclipse.jdt.core.formatter.insert_space_before_closing_paren_in_record_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_comma_in_method_declaration_parameters", &["org.eclipse.jdt.core.formatter.insert_space_before_comma_in_record_components"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_after_comma_in_method_declaration_parameters", &["org.eclipse.jdt.core.formatter.insert_space_after_comma_in_record_components"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_method_declaration", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_record_declaration"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_constructor_declaration", &["org.eclipse.jdt.core.formatter.insert_space_before_opening_brace_in_record_constructor"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.keep_type_declaration_on_one_line", &["org.eclipse.jdt.core.formatter.keep_record_declaration_on_one_line"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.keep_method_body_on_one_line", &["org.eclipse.jdt.core.formatter.keep_record_constructor_on_one_line"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.alignment_for_parameters_in_constructor_declaration", &["org.eclipse.jdt.core.formatter.alignment_for_record_components"]);
    check_and_replace(s, "org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_type_declaration", &["org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_record_declaration"]);
}

fn version_19_to_20(s: &mut Settings) {
    s.insert("org.eclipse.jdt.core.formatter.alignment_for_assertion_message".to_owned(), "0".to_owned());
}

fn version_21_to_22(s: &mut Settings) {
    s.insert("org.eclipse.jdt.core.formatter.align_selector_in_method_invocation_on_expression_first_line".to_owned(), "false".to_owned());
    s.insert("org.eclipse.jdt.core.formatter.alignment_for_switch_case_with_arrow".to_owned(), "0".to_owned());
    s.insert("org.eclipse.jdt.core.formatter.alignment_for_expressions_in_switch_case_with_arrow".to_owned(), "0".to_owned());
    s.insert("org.eclipse.jdt.core.formatter.alignment_for_expressions_in_switch_case_with_colon".to_owned(), "0".to_owned());
}

fn version_22_to_23(s: &mut Settings) {
    check_and_replace(s, "org.eclipse.jdt.core.formatter.alignment_for_superinterfaces_in_type_declaration", &["org.eclipse.jdt.core.formatter.alignment_for_permitted_types_in_type_declaration"]);
}
