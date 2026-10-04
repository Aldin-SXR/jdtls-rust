//! Override completion text built from JDT method/type binding data.
use super::handler::import_element_names;
use super::replacement::OverrideStub;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TypeData {
    kind: String,
    name: String,
    qualified: String,
    dimensions: usize,
    upper: bool,
    element: Option<Box<TypeData>>,
    bound: Option<Box<TypeData>>,
    arguments: Vec<TypeData>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TypeParameter {
    name: String,
    bounds: Vec<TypeData>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MethodData {
    name: String,
    modifiers: i32,
    in_interface: bool,
    declaring_interface: bool,
    declaring_object: bool,
    varargs: bool,
    return_type: TypeData,
    interface_super: Option<TypeData>,
    parameters: Vec<TypeData>,
    exceptions: Vec<TypeData>,
    type_parameters: Vec<TypeParameter>,
    parameter_names: Vec<String>,
}

struct Imports {
    names: HashMap<String, String>,
    added: Vec<String>,
}

impl Imports {
    fn new(source: &str) -> Self {
        let names = import_element_names(source)
            .into_iter()
            .filter(|(is_static, name)| !is_static && !name.ends_with(".*"))
            .map(|(_, name)| (name.rsplit('.').next().unwrap_or(&name).to_owned(), name))
            .collect();
        Self {
            names,
            added: Vec::new(),
        }
    }

    fn add(&mut self, t: &TypeData) -> String {
        match t.kind.as_str() {
            "simple" => t.name.clone(),
            "array" => {
                let element = t
                    .element
                    .as_deref()
                    .map(|e| self.add(e))
                    .unwrap_or_default();
                format!("{element}{}", "[]".repeat(t.dimensions))
            }
            "wildcard" => match t.bound.as_deref() {
                None => "?".into(),
                Some(bound) => format!(
                    "? {} {}",
                    if t.upper { "extends" } else { "super" },
                    self.add(bound)
                ),
            },
            _ => {
                let mut name = if self.names.get(&t.name).is_some_and(|q| q != &t.qualified) {
                    t.qualified.clone()
                } else {
                    self.names.insert(t.name.clone(), t.qualified.clone());
                    if !self.added.contains(&t.qualified) {
                        self.added.push(t.qualified.clone());
                    }
                    t.name.clone()
                };
                if !t.arguments.is_empty() {
                    name.push('<');
                    name.push_str(
                        &t.arguments
                            .iter()
                            .map(|a| self.add(a))
                            .collect::<Vec<_>>()
                            .join(", "),
                    );
                    name.push('>');
                }
                name
            }
        }
    }
}

const PUBLIC: i32 = 1;
const PRIVATE: i32 = 2;
const PROTECTED: i32 = 4;
const NATIVE: i32 = 256;
const ABSTRACT: i32 = 1024;
const DEFAULT: i32 = 65536;

fn indent(options: &BTreeMap<String, String>) -> String {
    let get = |name: &str| options.get(&format!("org.eclipse.jdt.core.formatter.{name}"));
    let size = get("tabulation.size")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(4)
        .max(1);
    match get("tabulation.char").map(String::as_str) {
        Some("space") => " ".repeat(size),
        Some("mixed") => {
            let width = get("indentation.size")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(4);
            format!("{}{}", "\t".repeat(width / size), " ".repeat(width % size))
        }
        _ => "\t".into(),
    }
}

/// `OverrideCompletionProposal` / `StubUtility2Core.createImplementationStubCore`.
pub fn replacement(
    method: &MethodData,
    source: &str,
    snippets: bool,
    options: &BTreeMap<String, String>,
) -> OverrideStub {
    let mut imports = Imports::new(source);
    let mut text = String::new();
    let skip = method.in_interface && method.declaring_object && method.modifiers & PUBLIC == 0;
    let override_enabled = !method.declaring_interface || options.get(
        "org.eclipse.jdt.core.compiler.problem.missingOverrideAnnotationForInterfaceMethodImplementation"
    ).is_none_or(|s| s != "disabled");
    if !skip && override_enabled {
        text.push_str("@Override\n");
    }
    let mut modifiers = method.modifiers;
    if method.in_interface {
        modifiers &= !(PROTECTED | PUBLIC);
        if modifiers & ABSTRACT != 0 {
            modifiers |= DEFAULT;
        }
    } else {
        modifiers &= !DEFAULT;
    }
    modifiers &= !(ABSTRACT | NATIVE | PRIVATE);
    if !method.in_interface && method.declaring_interface {
        modifiers |= PUBLIC;
    }
    for (flag, name) in [
        (PUBLIC, "public"),
        (PROTECTED, "protected"),
        (DEFAULT, "default"),
        (8, "static"),
        (16, "final"),
        (32, "synchronized"),
        (2048, "strictfp"),
    ] {
        if modifiers & flag != 0 {
            text.push_str(name);
            text.push(' ');
        }
    }
    if !method.type_parameters.is_empty() {
        text.push('<');
        for (index, parameter) in method.type_parameters.iter().enumerate() {
            if index > 0 {
                text.push_str(", ");
            }
            text.push_str(&parameter.name);
            if parameter.bounds.len() != 1 || parameter.bounds[0].qualified != "java.lang.Object" {
                for (index, bound) in parameter.bounds.iter().enumerate() {
                    text.push_str(if index == 0 { " extends " } else { " & " });
                    text.push_str(&imports.add(bound));
                }
            }
        }
        text.push_str("> ");
    }
    text.push_str(&imports.add(&method.return_type));
    text.push(' ');
    text.push_str(&method.name);
    text.push('(');
    let names = (0..method.parameters.len())
        .map(|i| {
            method
                .parameter_names
                .get(i)
                .cloned()
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| format!("arg{i}"))
        })
        .collect::<Vec<_>>();
    for (index, parameter) in method.parameters.iter().enumerate() {
        if index > 0 {
            text.push_str(", ");
        }
        if method.varargs && index + 1 == method.parameters.len() && parameter.kind == "array" {
            let t = imports.add(parameter);
            text.push_str(t.strip_suffix("[]").unwrap_or(&t));
            text.push_str("...");
        } else {
            text.push_str(&imports.add(parameter));
        }
        text.push(' ');
        text.push_str(&names[index]);
    }
    text.push(')');
    if !method.exceptions.is_empty() {
        text.push_str(" throws ");
        text.push_str(
            &method
                .exceptions
                .iter()
                .map(|t| imports.add(t))
                .collect::<Vec<_>>()
                .join(", "),
        );
    }
    text.push_str(" {\n");
    if !method.in_interface || !method.declaring_object {
        let statement = if method.modifiers & ABSTRACT != 0 {
            match method.return_type.name.as_str() {
                "void" => String::new(),
                "boolean" => "return false;".into(),
                "byte" | "short" | "int" | "long" | "char" | "float" | "double" => {
                    "return 0;".into()
                }
                _ => "return null;".into(),
            }
        } else {
            let receiver = method
                .interface_super
                .as_ref()
                .map(|t| format!("{}.", imports.add(t)))
                .unwrap_or_default();
            let invocation = format!("{receiver}super.{}({});", method.name, names.join(", "));
            if method.return_type.name == "void" {
                invocation
            } else {
                format!("return {invocation}")
            }
        };
        let body = if method.in_interface {
            format!("// TODO Auto-generated method stub\nthrow new UnsupportedOperationException(\"Unimplemented method '{}'\");", method.name)
        } else {
            format!("// TODO Auto-generated method stub\n{statement}")
        };
        let body = if snippets {
            format!("${{0:{}}}", body.replace('$', "\\$"))
        } else {
            body
        };
        let indentation = indent(options);
        for line in body.split('\n') {
            text.push_str(&indentation);
            text.push_str(line);
            text.push('\n');
        }
    }
    text.push('}');
    OverrideStub {
        text,
        imports: imports.added,
    }
}
