//! Rust port of JDT LS SmartDetectionHandler. Returns a caret destination;
//! the editor inserts the semicolon and owns the working copy.

mod partitions;
use super::completion::doc::Doc;
use crate::analysis::dispatcher::Dispatcher;
use crate::semantic_ast::{finder::NodeFinder, NodeKind};
use once_cell::sync::Lazy;
use partitions::{Kind, Partitions, Region};
use regex::Regex;
use serde::{Deserialize, Serialize};
use tower_lsp::lsp_types::Url;

#[derive(Debug, Deserialize, Serialize)]
pub struct SmartDetectionParams {
    uri: Option<String>,
    position: Option<SmartPosition>,
}

// lsp4j Position uses signed Java ints. JsonRpcHelpers permits a character
// outside its line, including a negative character that lands on a prior line.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
struct SmartPosition {
    line: i32,
    character: i32,
}

pub async fn handle(d: &Dispatcher, params: SmartDetectionParams) -> Option<SmartDetectionParams> {
    let uri = Url::parse(params.uri.as_deref()?).ok()?;
    let pos = params.position?;
    let source = super::source_text(&d.store, &uri)?;
    let doc = Doc::new(&source);
    let line = usize::try_from(pos.line).ok()?;
    if line >= doc.line_count() {
        return None;
    }
    // JsonRpcHelpers adds the character to the line start without clamping it.
    let caret = doc
        .line_offset(line)
        .checked_add_signed(pos.character as isize)?;
    if caret > doc.len() {
        return None;
    }
    let (options, fallback_level) = d.options_for(Some(&uri)).await;
    let level = options
        .get("org.eclipse.jdt.core.compiler.source")
        .unwrap_or(&fallback_level);
    let source_level = level
        .strip_prefix("1.")
        .unwrap_or(level)
        .parse::<u32>()
        .unwrap_or(21);
    let partitions = Partitions::new(&doc.units, source_level >= 15);
    let candidate = character_position(&doc, &partitions, caret)?;
    if candidate <= caret {
        return None;
    }
    if first_non_whitespace(&doc.units, &partitions, candidate..doc.len())
        .is_some_and(|p| doc.units[p] == b';' as u16)
    {
        return None;
    }
    // WAIT_ACTIVE_ONLY: an unrelated working copy has no hovered AST node.
    // A failed/cancelled AST lookup also leaves the lexical result usable.
    if d.store.is_active_java_uri(&uri) {
        if let Ok(ast) = crate::semantic_ast::fetch(d, &uri).await {
            if let Some(node) = NodeFinder::perform(ast.root(), candidate, 1) {
                let kind = node.kind();
                if kind.is_comment()
                    || matches!(kind, NodeKind::StringLiteral | NodeKind::TextBlock)
                    || (kind == NodeKind::MethodInvocation && node.end() > candidate)
                {
                    return None;
                }
            }
        }
    }
    let position = doc.position(candidate);
    Some(SmartDetectionParams {
        uri: params.uri,
        position: Some(SmartPosition {
            line: position.line as i32,
            character: position.character as i32,
        }),
    })
}

fn character_position(doc: &Doc, partitions: &Partitions, caret: usize) -> Option<usize> {
    if partitions.at(caret, false).kind != Kind::Java {
        return None;
    }
    let (start, length) = doc.line_info_of_offset(caret);
    let line = &doc.units[start..start + length];
    // The upstream heuristic examines only the first occurrence, even inside
    // a literal/comment and even when that first occurrence is an identifier.
    if let Some(p) = line.windows(3).position(|w| w == [102, 111, 114]) {
        if (p == 0 || !identifier_part(line[p - 1]))
            && (p + 3 == line.len() || !identifier_part(line[p + 3]))
        {
            return None;
        }
    }
    let mut partition = partitions.at(start + length, true);
    let valid = loop {
        if let Some(end) = valid_position(doc, partition, start + length) {
            break end.max(caret);
        }
        let Some(previous) = partition.start.checked_sub(1).filter(|p| *p >= caret) else {
            break caret;
        };
        partition = partitions.at(previous, false);
    };
    let mut insert = (valid - start).min(line.len());
    while insert > 0 && whitespace(line[insert - 1]) {
        insert -= 1;
    }
    match insert.checked_sub(1).map(|p| line[p]) {
        Some(59) => insert -= 1, // ';': alreadyPresent will suppress it.
        Some(125) => {
            // '}'
            let opening = (0..start + insert).rev().find(|&p| {
                doc.units[p] == b'{' as u16 && partitions.at(p, false).kind == Kind::Java
            });
            if let Some(opening) = opening.filter(|p| *p < caret) {
                if !array_initialization(&doc.units, partitions, opening) {
                    return Some(caret);
                }
            }
        }
        Some(61 | 46 | 123) => return None, // '=', '.', '{'
        _ => {}
    }
    Some(start + insert)
}

fn valid_position(doc: &Doc, partition: Region, max: usize) -> Option<usize> {
    if matches!(
        partition.kind,
        Kind::Javadoc | Kind::BlockComment | Kind::LineComment
    ) {
        return None;
    }
    let end = partition.end.min(max);
    // Java String.trim removes only code units <= U+0020.
    if partition.kind == Kind::Java && !doc.units[partition.start..end].iter().any(|c| *c > 0x20) {
        None
    } else {
        Some(end)
    }
}

fn first_non_whitespace(
    text: &[u16],
    partitions: &Partitions,
    positions: impl Iterator<Item = usize>,
) -> Option<usize> {
    positions
        .into_iter()
        .find(|&p| !whitespace(text[p]) && partitions.at(p, false).kind == Kind::Java)
}

fn array_initialization(text: &[u16], partitions: &Partitions, opening: usize) -> bool {
    let Some(p) = first_non_whitespace(text, partitions, (0..opening).rev()) else {
        return false;
    };
    if !matches!(text[p], 61 | 93) {
        return false;
    } // '=' or ']'
    if p == 0 {
        return true;
    }
    first_non_whitespace(text, partitions, (0..p).rev())
        .is_some_and(|p| identifier_part(text[p]) || matches!(text[p], 91 | 93))
}

/// Character.isWhitespace(char), including the four separator controls but
/// excluding nonbreaking spaces and NEXT LINE (Rust considers NEL whitespace).
fn whitespace(c: u16) -> bool {
    matches!(c, 0x9..=0xd | 0x1c..=0x20 | 0x1680 | 0x2000..=0x2006 | 0x2008..=0x200a | 0x2028 | 0x2029 | 0x205f | 0x3000)
}

fn identifier_part(c: u16) -> bool {
    static PART: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\A[\p{L}\p{Nl}\p{Sc}\p{Pc}\p{Nd}\p{Mn}\p{Mc}\p{Cf}\x{0000}-\x{0008}\x{000E}-\x{001B}\x{007F}-\x{009F}]\z").unwrap()
    });
    char::from_u32(c as u32).is_some_and(|ch| PART.is_match(ch.encode_utf8(&mut [0; 4])))
}
