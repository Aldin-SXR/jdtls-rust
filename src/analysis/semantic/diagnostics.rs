//! Convert BridgeDiagnostic → lsp_types::Diagnostic.

use super::protocol::BridgeDiagnostic;
use tower_lsp::lsp_types::{Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Url};

pub fn to_lsp(d: &BridgeDiagnostic) -> Option<(Url, Diagnostic)> {
    let uri = Url::parse(&d.uri).ok()?;

    let severity = match d.severity {
        1 => DiagnosticSeverity::ERROR,
        2 => DiagnosticSeverity::WARNING,
        3 => DiagnosticSeverity::INFORMATION,
        4 => DiagnosticSeverity::HINT,
        _ => DiagnosticSeverity::ERROR,
    };

    let tags = d.tags.as_ref().map(|ts| {
        ts.iter().filter_map(|&t| match t {
            1 => Some(DiagnosticTag::UNNECESSARY),
            2 => Some(DiagnosticTag::DEPRECATED),
            _ => None,
        }).collect::<Vec<_>>()
    });

    let diag = Diagnostic {
        range: Range {
            start: Position { line: d.start_line, character: d.start_char },
            end: Position { line: d.end_line, character: d.end_char },
        },
        severity: Some(severity),
        // Prefer the explicit code string; fall back to the ECJ category ID.
        code: d.code.as_ref()
            .map(|c| NumberOrString::String(c.clone()))
            .or_else(|| if d.category_id > 0 {
                Some(NumberOrString::Number(d.category_id as i32))
            } else {
                None
            }),
        source: Some("jdtls-rust".to_owned()),
        message: d.message.clone(),
        tags,
        ..Default::default()
    };

    Some((uri, diag))
}
