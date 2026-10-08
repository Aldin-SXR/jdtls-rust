//! Port of jdt.ls `CreateModuleInfoHandler` (`java.project.createModuleInfo`).

use std::path::Path;

pub const MODULE_INFO_JAVA: &str = "module-info.java";

const KEYWORDS: &[&str] = &[
    "abstract", "assert", "boolean", "break", "byte", "case", "catch", "char", "class", "const", "continue",
    "default", "do", "double", "else", "enum", "extends", "false", "final", "finally", "float", "for", "goto",
    "if", "implements", "import", "instanceof", "int", "interface", "long", "native", "new", "null", "package",
    "private", "protected", "public", "return", "short", "static", "strictfp", "super", "switch", "synchronized",
    "this", "throw", "throws", "transient", "true", "try", "void", "volatile", "while", "_",
];

/// `CreateModuleInfoHandler.convertToModuleName`.
pub fn convert_to_module_name(name: &str) -> String {
    let mut replaced = String::new();
    let mut last_dot = false;
    for c in name.chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '.' };
        if c == '.' && last_dot {
            continue;
        }
        last_dot = c == '.';
        replaced.push(c);
    }
    replaced.trim_matches('.').to_owned()
}

/// `JavaConventionsUtil.validateModuleName(name, project) == VERIFIED_OK`.
fn is_valid_module_name(name: &str) -> bool {
    !name.is_empty()
        && name.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars.next().is_some_and(|c| c.is_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '$')
                && !KEYWORDS.contains(&segment)
        })
}

pub fn module_name(project_name: &str) -> String {
    let name = convert_to_module_name(project_name);
    if is_valid_module_name(&name) {
        name
    } else {
        "module.name".to_owned()
    }
}

/// `JavaModelUtil.is9OrHigher(String compliance)`.
pub fn is_9_or_higher(compliance: &str) -> bool {
    let version = compliance.strip_prefix("1.").map_or(compliance, |_| "1");
    version.split('.').next().and_then(|major| major.parse::<u32>().ok()).is_some_and(|major| major >= 9)
}

/// Iteration order of a `java.util.HashSet<String>` filled with `items` in order.
pub fn java_hash_set_order(items: &[String]) -> Vec<String> {
    let mut unique: Vec<&String> = Vec::new();
    for item in items {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    let mut capacity = 16usize;
    while unique.len() * 4 > capacity * 3 {
        capacity *= 2;
    }
    let bucket = |s: &str| {
        let h = s.encode_utf16().fold(0i32, |h, c| h.wrapping_mul(31).wrapping_add(i32::from(c))) as u32;
        ((h ^ (h >> 16)) as usize) & (capacity - 1)
    };
    let mut ordered: Vec<(usize, usize, &String)> = unique.iter().enumerate().map(|(i, s)| (bucket(s), i, *s)).collect();
    ordered.sort();
    ordered.into_iter().map(|(_, _, s)| s.clone()).collect()
}

/// The packages (dotted names, depth first) of a source folder that hold compilation units.
pub fn packages_with_units(source_folder: &Path) -> Vec<String> {
    fn visit(dir: &Path, package: &str, out: &mut Vec<String>) {
        let Ok(read) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(|e| e.file_name());
        let has_units = entries.iter().any(|e| {
            e.file_type().is_ok_and(|t| t.is_file()) && e.file_name().to_string_lossy().ends_with(".java")
        });
        if !package.is_empty() && has_units {
            out.push(package.to_owned());
        }
        for entry in entries {
            if entry.file_type().is_ok_and(|t| t.is_dir()) {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let child = if package.is_empty() { name } else { format!("{package}.{name}") };
                visit(&entry.path(), &child, out);
            }
        }
    }
    let mut out = Vec::new();
    visit(source_folder, "", &mut out);
    out
}

/// The unformatted `module-info.java` text.
pub fn module_info_text(module_name: &str, exported_packages: &[String], required_modules: &[String], line_delimiter: &str) -> String {
    let mut text = format!("module {module_name} {{{line_delimiter}");
    for package in exported_packages {
        text.push_str(&format!("\texports {package};{line_delimiter}"));
    }
    for module in required_modules {
        text.push_str(&format!("\trequires {module};{line_delimiter}"));
    }
    text.push('}');
    text.push_str(line_delimiter);
    text
}

#[cfg(test)]
mod create_module_info_handler_test {
    use super::*;

    #[test]
    fn test_convert_to_module_name() {
        assert_eq!("a.b", convert_to_module_name("..a-b.."));
    }

}

#[cfg(test)]
mod hash_order_tests {
    use super::*;

    #[test]
    fn hash_set_order_follows_java_buckets() {
        let items: Vec<String> = ["com.example", "org.sample", "a"].iter().map(|s| s.to_string()).collect();
        // "a" hashes to 97 (bucket 1); the order is by bucket, then insertion.
        assert_eq!("a", java_hash_set_order(&items)[0]);
    }
}
