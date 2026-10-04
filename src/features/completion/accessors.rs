//! Getter/setter names and stubs (`GetterSetterUtil`, `NamingConventions`
//! with jdt.ls' default code templates) for `GetterSetterCompletionProposal`.

use super::proposal::{flags, EnclosingField};
use super::signature as sig;

fn is_boolean(type_signature: &str) -> bool {
    type_signature == "Z"
}

/// `NamingConventions.suggestAccessorName` (no field prefixes/suffixes configured).
fn accessor_name(field: &str) -> String {
    let mut c: Vec<char> = field.chars().collect();
    if !c.is_empty() && c[0].is_lowercase() && (c.len() == 1 || !c[1].is_uppercase()) {
        c[0] = c[0].to_uppercase().next().unwrap_or(c[0]);
    }
    c.into_iter().collect()
}

/// `GetterSetterUtil.getGetterName(field, null)`.
pub fn getter_name(field: &str, type_signature: &str) -> String {
    if is_boolean(type_signature) {
        let chars: Vec<char> = field.chars().collect();
        if field.starts_with("is") && chars.len() > 2 && chars[2].is_uppercase() {
            return field.to_owned();
        }
        return format!("is{}", accessor_name(field));
    }
    format!("get{}", accessor_name(field))
}

/// `GetterSetterUtil.getSetterName(field, null)`.
pub fn setter_name(field: &str) -> String {
    format!("set{}", accessor_name(field))
}

fn setter_name_for(field: &EnclosingField) -> String {
    if is_boolean(&field.type_signature) {
        let chars: Vec<char> = field.name.chars().collect();
        if field.name.starts_with("is") && chars.len() > 2 && chars[2].is_uppercase() {
            return format!("set{}", &field.name[2..]);
        }
    }
    setter_name(&field.name)
}

/// `StubUtility.getBaseName(field)`.
fn base_name(field: &str) -> String {
    field.to_owned()
}

fn visibility(f: i32) -> &'static str {
    if flags::is(f, flags::PUBLIC) {
        "public"
    } else if flags::is(f, flags::PROTECTED) {
        "protected"
    } else if flags::is(f, flags::PRIVATE) {
        "private"
    } else {
        ""
    }
}

/// Unformatted `GetterSetterUtil.getGetterStub` / `getSetterStub` (line delimiter "\n").
pub fn stub(field: &EnclosingField, is_getter: bool, add_comments: bool, type_qualified_name: &str) -> String {
    let method_flags = flags::PUBLIC | (field.flags & flags::STATIC);
    let is_static = flags::is(method_flags, flags::STATIC);
    let type_name = sig::to_string(&field.type_signature).unwrap_or_default();
    let mut buf = String::new();
    if is_getter {
        let name = getter_name(&field.name, &field.type_signature);
        if add_comments {
            buf.push_str(&format!("/**\n * @return the {}\n */", base_name(&field.name)));
            buf.push('\n');
        }
        buf.push_str(visibility(method_flags));
        buf.push(' ');
        if is_static {
            buf.push_str("static ");
        }
        buf.push_str(&type_name);
        buf.push(' ');
        buf.push_str(&name);
        buf.push_str("() {\n");
        buf.push_str(&format!("return {};\n", field.name));
        buf.push_str("}\n");
    } else {
        let name = setter_name_for(field);
        let argname = field.name.clone();
        if add_comments {
            buf.push_str(&format!("/**\n * @param {argname} the {} to set\n */", base_name(&field.name)));
            buf.push('\n');
        }
        buf.push_str(visibility(method_flags));
        buf.push(' ');
        if is_static {
            buf.push_str("static ");
        }
        buf.push_str("void ");
        buf.push_str(&name);
        buf.push('(');
        buf.push_str(&type_name);
        buf.push(' ');
        buf.push_str(&argname);
        buf.push_str(") {\n");
        let field_name = if argname == field.name {
            if is_static {
                format!("{type_qualified_name}.{}", field.name)
            } else {
                format!("this.{}", field.name)
            }
        } else {
            field.name.clone()
        };
        buf.push_str(&format!("{field_name} = {argname};\n"));
        buf.push_str("}\n");
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(getter_name("strField", "QString;"), "getStrField");
        assert_eq!(getter_name("boolField", "Z"), "isBoolField");
        assert_eq!(getter_name("isOpen", "Z"), "isOpen");
        assert_eq!(setter_name("strField"), "setStrField");
        assert_eq!(getter_name("xPos", "I"), "getxPos");
    }
}
