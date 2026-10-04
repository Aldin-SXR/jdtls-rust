//! Javadoc rendering: Rust ports of the eclipse.jdt.ls / jdt.core.manipulation
//! Javadoc pipeline.
//!
//! * `doc_ast`, `access`, `snippet`, `markdown_comment`, `path_handler`:
//!   Javadoc DOM (as returned by the bridge) → HTML, ported from
//!   `CoreJavadocAccessImpl` / `JavadocContentAccess2.JdtLsJavadocAccessImpl`.
//! * `text_reader`: `CoreJavaDoc2HTMLTextReader` (+ the jdt.ls subclass).
//! * `html`: a jsoup-compatible HTML DOM and parser.
//! * `html2md`: port of flexmark's `FlexmarkHtmlConverter`.
//! * `converter`: `JavaDoc2MarkdownConverter` / `JavaDoc2PlainTextConverter`.
//! * `labels`: `JavaElementLabelsCore` (signature labels) over bridge data.
#![allow(dead_code)]

pub mod access;
pub mod attached;
pub mod comment_reader;
pub mod converter;
pub mod doc_ast;
pub mod html;
pub mod html2md;
pub mod html_builder;
pub mod labels;
pub mod markdown_comment;
pub mod path_handler;
pub mod plain_text;
pub mod snippet;
pub mod table_helper;
pub mod text_reader;

/// `JavadocContentAccess2.SNIPPET`
pub const SNIPPET: &str = "SNIPPET";
