//! Semantic tokens (syntax highlighting) derived from tree-sitter CST.

use tower_lsp::lsp_types::{SemanticToken, SemanticTokenType, SemanticTokensLegend};
use tree_sitter::{Node, Tree};

/// Returns the SemanticTokensLegend advertised in server capabilities.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::NAMESPACE,       // 0
            SemanticTokenType::TYPE,            // 1
            SemanticTokenType::CLASS,           // 2
            SemanticTokenType::ENUM,            // 3
            SemanticTokenType::INTERFACE,       // 4
            SemanticTokenType::STRUCT,          // 5
            SemanticTokenType::TYPE_PARAMETER,  // 6
            SemanticTokenType::PARAMETER,       // 7
            SemanticTokenType::VARIABLE,        // 8
            SemanticTokenType::PROPERTY,        // 9
            SemanticTokenType::ENUM_MEMBER,     // 10
            SemanticTokenType::FUNCTION,        // 11
            SemanticTokenType::METHOD,          // 12
            SemanticTokenType::MACRO,           // 13
            SemanticTokenType::KEYWORD,         // 14
            SemanticTokenType::MODIFIER,        // 15
            SemanticTokenType::COMMENT,         // 16
            SemanticTokenType::STRING,          // 17
            SemanticTokenType::NUMBER,          // 18
            SemanticTokenType::REGEXP,          // 19
            SemanticTokenType::OPERATOR,        // 20
        ],
        token_modifiers: vec![],
    }
}

/// Walk the tree-sitter CST and emit LSP SemanticTokens (delta-encoded).
pub fn semantic_tokens_full(tree: &Tree, source: &str) -> Vec<SemanticToken> {
    let mut collector = TokenCollector::new(source);
    collect_tokens(tree.root_node(), &collector.source_bytes, &mut collector);
    collector.finish()
}

struct RawToken {
    line: u32,
    start_char: u32,
    length: u32,
    token_type: u32,
}

struct TokenCollector<'a> {
    source_bytes: &'a [u8],
    raw: Vec<RawToken>,
}

impl<'a> TokenCollector<'a> {
    fn new(source: &'a str) -> Self {
        Self { source_bytes: source.as_bytes(), raw: Vec::new() }
    }

    fn push(&mut self, node: Node, token_type: u32) {
        let start = node.start_position();
        let end = node.end_position();
        if start.row != end.row {
            return; // skip multi-line tokens for now
        }
        self.raw.push(RawToken {
            line: start.row as u32,
            start_char: start.column as u32,
            length: (end.column - start.column) as u32,
            token_type,
        });
    }

    fn finish(mut self) -> Vec<SemanticToken> {
        self.raw.sort_by_key(|t| (t.line, t.start_char));
        let mut result = Vec::with_capacity(self.raw.len());
        let mut prev_line = 0u32;
        let mut prev_char = 0u32;
        for t in &self.raw {
            let delta_line = t.line - prev_line;
            let delta_start = if delta_line == 0 { t.start_char - prev_char } else { t.start_char };
            result.push(SemanticToken {
                delta_line,
                delta_start,
                length: t.length,
                token_type: t.token_type,
                token_modifiers_bitset: 0,
            });
            prev_line = t.line;
            prev_char = t.start_char;
        }
        result
    }
}

fn collect_tokens(node: Node, src: &[u8], col: &mut TokenCollector) {
    let kind = node.kind();
    match kind {
        // Keywords — 14
        "abstract" | "assert" | "break" | "case" | "catch" | "class" | "continue"
        | "default" | "do" | "else" | "enum" | "extends" | "final" | "finally"
        | "for" | "if" | "implements" | "import" | "instanceof" | "interface"
        | "new" | "package" | "return" | "static" | "super" | "switch" | "this"
        | "throw" | "throws" | "try" | "void" | "while" | "var" | "record"
        | "sealed" | "permits" | "yield" | "non_sealed" => {
            col.push(node, 14);
        }
        // Modifiers — 15
        "public" | "private" | "protected" | "synchronized" | "volatile"
        | "transient" | "native" | "strictfp" => {
            col.push(node, 15);
        }
        // Comments — 16
        "line_comment" | "block_comment" => {
            col.push(node, 16);
        }
        // Strings — 17
        "string_literal" | "text_block" | "character_literal" => {
            col.push(node, 17);
        }
        // Numbers — 18
        "decimal_integer_literal" | "hex_integer_literal" | "octal_integer_literal"
        | "binary_integer_literal" | "decimal_floating_point_literal"
        | "hex_floating_point_literal" => {
            col.push(node, 18);
        }
        // Operators — 20
        "+" | "-" | "*" | "/" | "%" | "==" | "!=" | "<" | ">" | "<=" | ">="
        | "&&" | "||" | "!" | "&" | "|" | "^" | "~" | "<<" | ">>" | ">>>"
        | "+=" | "-=" | "*=" | "/=" | "%=" | "&=" | "|=" | "^=" | "<<="
        | ">>=" | ">>>=" | "++" | "--" | "?" | ":" | "->" | "::" => {
            col.push(node, 20);
        }
        // Class/interface/enum declarations — 2/4/3
        "class_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                col.push(name, 2);
            }
        }
        "interface_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                col.push(name, 4);
            }
        }
        "enum_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                col.push(name, 3);
            }
        }
        "record_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                col.push(name, 2);
            }
        }
        // Method declarations — 12
        "method_declaration" | "constructor_declaration" => {
            if let Some(name) = node.child_by_field_name("name") {
                col.push(name, 12);
            }
        }
        // Parameters — 7
        "formal_parameter" | "spread_parameter" => {
            if let Some(name) = node.child_by_field_name("name") {
                col.push(name, 7);
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_tokens(child, src, col);
    }
}
