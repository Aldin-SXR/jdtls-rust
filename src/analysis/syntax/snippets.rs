//! Static Java snippet completions (keywords, common constructs).

use tower_lsp::lsp_types::{
    CompletionItem, CompletionItemKind, InsertTextFormat, Documentation, MarkupContent, MarkupKind,
};

/// Return the full set of static Java snippets.
pub fn java_snippets() -> Vec<CompletionItem> {
    vec![
        snippet("for", "for loop", "for (${1:int} ${2:i} = 0; ${2:i} < ${3:n}; ${2:i}++) {\n\t${0}\n}"),
        snippet("fori", "for-each loop", "for (${1:var} ${2:item} : ${3:items}) {\n\t${0}\n}"),
        snippet("while", "while loop", "while (${1:condition}) {\n\t${0}\n}"),
        snippet("do", "do-while loop", "do {\n\t${0}\n} while (${1:condition});"),
        snippet("if", "if statement", "if (${1:condition}) {\n\t${0}\n}"),
        snippet("ife", "if-else statement", "if (${1:condition}) {\n\t${2}\n} else {\n\t${0}\n}"),
        snippet("sw", "switch statement", "switch (${1:value}) {\n\tcase ${2:val}:\n\t\t${0}\n\t\tbreak;\n\tdefault:\n\t\tbreak;\n}"),
        snippet("try", "try-catch", "try {\n\t${1}\n} catch (${2:Exception} e) {\n\t${0}\n}"),
        snippet("tryf", "try-finally", "try {\n\t${1}\n} finally {\n\t${0}\n}"),
        snippet("class", "class declaration", "public class ${1:ClassName} {\n\t${0}\n}"),
        snippet("iface", "interface declaration", "public interface ${1:InterfaceName} {\n\t${0}\n}"),
        snippet("enum", "enum declaration", "public enum ${1:EnumName} {\n\t${2:VALUE};\n\t${0}\n}"),
        snippet("record", "record declaration", "public record ${1:RecordName}(${2:Type} ${3:field}) {\n\t${0}\n}"),
        snippet("main", "main method", "public static void main(String[] args) {\n\t${0}\n}"),
        snippet("psvm", "public static void main", "public static void main(String[] args) {\n\t${0}\n}"),
        snippet("sout", "System.out.println", "System.out.println(${0});"),
        snippet("serr", "System.err.println", "System.err.println(${0});"),
        snippet("syso", "System.out.println", "System.out.println(${0});"),
        snippet("lambda", "lambda expression", "(${1:params}) -> ${0}"),
        snippet("optional", "Optional usage", "Optional<${1:Type}> ${2:opt} = Optional.ofNullable(${3:value});"),
        snippet("stream", "stream chain", "${1:collection}.stream()\n\t.filter(${2:e} -> ${3:condition})\n\t.map(${4:e} -> ${5:e})\n\t.collect(Collectors.toList())"),
        snippet("@Override", "@Override annotation", "@Override\n${0}"),
        snippet("test", "JUnit test method", "@Test\npublic void ${1:testMethod}() {\n\t${0}\n}"),
        snippet("@Test", "@Test annotation + method", "@Test\npublic void ${1:testMethod}() {\n\t${0}\n}"),
        snippet("assert", "assertEquals assertion", "assertEquals(${1:expected}, ${2:actual});"),
        // Primitive types as keywords
        keyword("boolean"), keyword("byte"), keyword("char"), keyword("double"),
        keyword("float"), keyword("int"), keyword("long"), keyword("short"),
        keyword("void"), keyword("null"), keyword("true"), keyword("false"),
        keyword("this"), keyword("super"), keyword("new"), keyword("return"),
        keyword("throw"), keyword("throws"), keyword("import"), keyword("package"),
        keyword("extends"), keyword("implements"), keyword("instanceof"),
        keyword("static"), keyword("final"), keyword("abstract"), keyword("synchronized"),
        keyword("volatile"), keyword("transient"), keyword("native"),
        keyword("public"), keyword("private"), keyword("protected"),
        keyword("var"), keyword("sealed"), keyword("permits"), keyword("record"),
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
