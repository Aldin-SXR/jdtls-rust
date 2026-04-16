//! Static Java snippet completions.
//! Template content sourced directly from eclipse.jdt.ls:
//! org.eclipse.jdt.ls.core/src/org/eclipse/jdt/ls/core/internal/corext/template/java/CodeSnippetTemplate.java

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, InsertTextFormat, Documentation, MarkupContent, MarkupKind,
};

/// Keywords valid in expression position (after `=`, `return`, etc.).
/// Used instead of `java_snippets()` when the cursor is in an expression context.
pub fn expression_keywords() -> Vec<CompletionItem> {
    vec![
        keyword("new"), keyword("null"), keyword("true"), keyword("false"),
        keyword("this"), keyword("super"), keyword("instanceof"),
        snippet("new", "create new object", "${1:Object} ${2:foo} = new ${1}(${3});\n${0}"),
    ]
}

/// Snippets valid only in a class body (not inside a method).
/// Corresponds to JavaContextType.ID_MEMBERS in jdtls.
pub fn class_body_snippets() -> Vec<CompletionItem> {
    vec![
        // Note: "ctor" is generated dynamically in CompletionService.java
        // with the actual enclosing class name substituted in.
        snippet("main", "public static main method",
            "public static void main(String[] args) {\n\t${0}\n}"),
        snippet("psvm", "public static main method",
            "public static void main(String[] args) {\n\t${0}\n}"),
        snippet("method", "method",
            "${1|public,protected,private|}${2| , static |}${3:void} ${4:name}(${5}) {\n\t${0}\n}"),
        snippet("staticmethod", "static method",
            "${1|public,private|} static ${2:void} ${3:name}(${4}) {\n\t${0}\n}"),
        snippet("field", "field",
            "${1|public,protected,private|} ${2:String} ${3:name};"),
        // "new" is ID_ALL in jdtls — valid in both members and statements contexts.
        snippet("new", "create new object",
            "${1:Object} ${2:foo} = new ${1}(${3});\n${0}"),
    ]
}

/// Snippets valid inside a method body, matching jdtls template definitions.
pub fn method_body_snippets() -> Vec<CompletionItem> {
    vec![
        // ── Iteration ─────────────────────────────────────────────────────────
        snippet("foreach", "iterate over an array or Iterable",
            "for (${1:var} ${2:item} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("iter", "iterate over an array or Iterable",
            "for (${1:var} ${2:item} : ${3:iterable}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("fori", "iterate over array",
            "for (${1:int} ${2:i} = ${3:0}; ${2:i} < ${4:array.length}; ${2:i}++) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        // ── Conditionals ──────────────────────────────────────────────────────
        snippet("if", "if statement",
            "if (${1:condition}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("ifelse", "if-else statement",
            "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}"),
        snippet("ife", "if-else statement",
            "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}"),
        snippet("ifnull", "if statement checking for null",
            "if (${1:name} == null) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("ifnotnull", "if statement checking for not null",
            "if (${1:name} != null) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        // ── Loops ─────────────────────────────────────────────────────────────
        snippet("while", "while statement",
            "while (${1:condition}) {\n\t$TM_SELECTED_TEXT${0}\n}"),
        snippet("dowhile", "do-while statement",
            "do {\n\t$TM_SELECTED_TEXT${0}\n} while (${1:condition});"),
        snippet("do", "do-while statement",
            "do {\n\t$TM_SELECTED_TEXT${0}\n} while (${1:condition});"),
        // ── Switch ────────────────────────────────────────────────────────────
        snippet("switch", "switch statement",
            "switch (${1:key}) {\n\tcase ${2:value}:\n\t\t${0}\n\t\tbreak;\n\n\tdefault:\n\t\tbreak;\n}"),
        snippet("sw", "switch statement",
            "switch (${1:key}) {\n\tcase ${2:value}:\n\t\t${0}\n\t\tbreak;\n\n\tdefault:\n\t\tbreak;\n}"),
        // ── Exception handling ────────────────────────────────────────────────
        snippet("try", "try/catch block",
            "try {\n\t$TM_SELECTED_TEXT${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}// TODO: handle exception\n}"),
        snippet("trycatch", "try/catch block",
            "try {\n\t$TM_SELECTED_TEXT${1}\n} catch (${2:Exception} ${3:e}) {\n\t${0}// TODO: handle exception\n}"),
        snippet("tryresources", "try/catch block with resources",
            "try (${1}) {\n\t$TM_SELECTED_TEXT${2}\n} catch (${3:Exception} ${4:e}) {\n\t${0}// TODO: handle exception\n}"),
        // ── Output ────────────────────────────────────────────────────────────
        snippet("sysout", "print to standard out",   "System.out.println(${0});"),
        snippet("sout",   "print to standard out",   "System.out.println(${0});"),
        snippet("syso",   "print to standard out",   "System.out.println(${0});"),
        snippet("syserr", "print to standard err",   "System.err.println(${0});"),
        snippet("serr",   "print to standard err",   "System.err.println(${0});"),
        snippet("soutm",  "print current method to standard out",
            "System.out.println(\"${0}\");"),
        // ── Misc ──────────────────────────────────────────────────────────────
        snippet("new", "create new object",
            "${1:Object} ${2:foo} = new ${1}(${3});\n${0}"),
        // ── Keywords valid inside a method body ──────────────────────────────
        keyword("boolean"), keyword("byte"), keyword("char"), keyword("double"),
        keyword("float"), keyword("int"), keyword("long"), keyword("short"),
        keyword("void"), keyword("var"),
        keyword("null"), keyword("true"), keyword("false"),
        keyword("this"), keyword("super"), keyword("new"),
        keyword("return"), keyword("throw"), keyword("instanceof"),
        keyword("final"),
        keyword("yield"), keyword("break"), keyword("continue"), keyword("default"),
    ]
}

fn snippet(label: &str, detail: &str, insert_text: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::SNIPPET),
        detail: Some(detail.to_owned()),
        insert_text: Some(insert_text.to_owned()),
        insert_text_format: Some(InsertTextFormat::SNIPPET),
        documentation: Some(Documentation::MarkupContent(MarkupContent {
            kind: MarkupKind::Markdown,
            value: format!("```java\n{insert_text}\n```"),
        })),
        ..Default::default()
    }
}

fn keyword(label: &str) -> CompletionItem {
    CompletionItem {
        label: label.to_owned(),
        kind: Some(CompletionItemKind::KEYWORD),
        ..Default::default()
    }
}
