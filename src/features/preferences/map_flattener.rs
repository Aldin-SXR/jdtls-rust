//! Port of jdt.ls `MapFlattener`: access to nested configuration maps via
//! chained keys, so `{"java": {"format": {"enabled": true}}}` and
//! `{"java.format.enabled": true}` both answer `java.format.enabled`.

use serde_json::{Map, Value};

/// `MapFlattener.getValue`: the flat key first, then the nested maps.
/// A JSON `null` reads as absent, like a Java `null`.
pub fn get_value<'a>(configuration: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(v) = configuration.get(key).filter(|v| !v.is_null()) {
        return Some(v);
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = configuration.as_object()?;
    for (i, part) in parts.iter().enumerate() {
        let val = current.get(*part);
        if i == parts.len() - 1 {
            return val.filter(|v| !v.is_null());
        }
        current = val?.as_object()?;
    }
    None
}

/// `MapFlattener.containsKey`: a key that is present with a `null` value
/// counts.
pub fn contains_key(configuration: &Value, key: &str) -> bool {
    let Some(map) = configuration.as_object() else { return false };
    if map.contains_key(key) {
        return true;
    }
    let parts: Vec<&str> = key.split('.').collect();
    let mut current = map;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            return current.contains_key(*part);
        }
        match current.get(*part).and_then(Value::as_object) {
            Some(m) => current = m,
            None => return false,
        }
    }
    false
}

/// `MapFlattener.setValue`: sets a chained key, creating nested maps as
/// needed (an existing non-map value on the path is replaced).
#[allow(dead_code)]
pub fn set_value(configuration: &mut Value, chained_key: &str, value: Value) {
    if !configuration.is_object() {
        *configuration = Value::Object(Map::new());
    }
    let mut current = configuration.as_object_mut().expect("object");
    let parts: Vec<&str> = chained_key.split('.').collect();
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current.insert((*part).to_owned(), value);
            return;
        }
        let slot = current.entry((*part).to_owned()).or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        current = slot.as_object_mut().expect("object");
    }
}

/// `MapFlattener.getString(configuration, key, def)`.
pub fn get_string(configuration: &Value, key: &str, def: Option<&str>) -> Option<String> {
    match get_value(configuration, key) {
        Some(Value::String(s)) => Some(s.clone()),
        _ => def.map(str::to_owned),
    }
}

/// `MapFlattener.getBoolean(configuration, key, def)` (`Boolean.parseBoolean`
/// for strings).
pub fn get_boolean(configuration: &Value, key: &str, def: bool) -> bool {
    match get_value(configuration, key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s.eq_ignore_ascii_case("true"),
        _ => def,
    }
}

/// `MapFlattener.getInt(configuration, key, def)`.
pub fn get_int(configuration: &Value, key: &str, def: i32) -> i32 {
    match get_value(configuration, key) {
        Some(Value::Number(n)) => n.as_i64().map(|n| n as i32).or_else(|| n.as_f64().map(|f| f as i32)).unwrap_or(def),
        Some(Value::String(s)) => s.parse().unwrap_or(def),
        _ => def,
    }
}

/// `MapFlattener.getList(configuration, key, def)`: a JSON array, a JSON
/// array encoded as a string, a comma-separated string or a
/// space-separated string.
pub fn get_list(configuration: &Value, key: &str, def: Option<Vec<String>>) -> Option<Vec<String>> {
    let strings = |a: &[Value]| a.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>();
    match get_value(configuration, key) {
        Some(Value::String(s)) => {
            let mut s = s.clone();
            if !s.trim().starts_with('[') {
                if s.contains(',') {
                    s = format!("[{s}]");
                } else {
                    return Some(s.split(' ').filter(|e| !e.is_empty()).map(str::to_owned).collect());
                }
            }
            parse_lenient_string_list(&s).unwrap_or(def)
        }
        Some(Value::Array(a)) => Some(strings(a)),
        _ => def,
    }
}

/// `new Gson().fromJson(str, List<String>)`: Gson reads leniently, so
/// single-quoted and unquoted elements are accepted (`['a', 'b']`,
/// `[c, d]`). `None` is a `JsonSyntaxException`; `null` elements are dropped.
fn parse_lenient_string_list(s: &str) -> Option<Option<Vec<String>>> {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let skip_ws = |i: &mut usize| {
        while *i < chars.len() && matches!(chars[*i], ' ' | '\t' | '\n' | '\r' | '\u{c}') {
            *i += 1;
        }
    };
    skip_ws(&mut i);
    if i == chars.len() {
        // An empty document is `null`.
        return Some(None);
    }
    if chars[i] != '[' {
        return None;
    }
    i += 1;
    let mut out = Vec::new();
    skip_ws(&mut i);
    if i < chars.len() && chars[i] == ']' {
        return Some(Some(out));
    }
    loop {
        skip_ws(&mut i);
        let c = *chars.get(i)?;
        match c {
            '"' | '\'' => {
                i += 1;
                let mut v = String::new();
                loop {
                    let ch = *chars.get(i)?;
                    i += 1;
                    if ch == c {
                        break;
                    }
                    if ch == '\\' {
                        let e = *chars.get(i)?;
                        i += 1;
                        match e {
                            'n' => v.push('\n'),
                            't' => v.push('\t'),
                            'r' => v.push('\r'),
                            'b' => v.push('\u{8}'),
                            'f' => v.push('\u{c}'),
                            'u' => {
                                let hex: String = chars.get(i..i + 4)?.iter().collect();
                                v.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
                                i += 4;
                            }
                            other => v.push(other),
                        }
                    } else {
                        v.push(ch);
                    }
                }
                out.push(v);
            }
            '[' | '{' | ']' | ',' | ';' => {
                // A missing element reads as `null` in lenient mode.
                if c != ']' && c != ',' && c != ';' {
                    return None;
                }
            }
            _ => {
                let start = i;
                while i < chars.len()
                    && !matches!(
                        chars[i],
                        '/' | '\\' | ';' | '#' | '=' | '{' | '}' | '[' | ']' | ':' | ',' | ' ' | '\t' | '\u{c}' | '\r' | '\n'
                    )
                {
                    i += 1;
                }
                let lit: String = chars[start..i].iter().collect();
                if lit.is_empty() {
                    return None;
                }
                if lit != "null" {
                    out.push(lit);
                }
            }
        }
        skip_ws(&mut i);
        match chars.get(i)? {
            ',' | ';' => i += 1,
            ']' => return Some(Some(out)),
            _ => return None,
        }
    }
}

#[cfg(test)]
mod map_flattener_test {
    //! Port of `org.eclipse.jdt.ls.core.internal.handlers.MapFlattenerTest`.

    use super::*;
    use serde_json::json;

    fn get_string1(c: &Value, k: &str) -> Option<String> {
        get_string(c, k, None)
    }

    #[test]
    fn test_get_string() {
        let mut config = json!({});
        config["foo"] = json!("bar");
        let mut middle = json!({});
        middle["thing"] = json!("value");
        let mut bottom = json!({});
        bottom["another"] = json!("thing");
        middle["bottom"] = bottom;
        config["java"] = middle;

        assert_eq!(get_string1(&config, "missing"), None, "default");
        assert_eq!(get_string(&config, "missing", Some("default")).as_deref(), Some("default"));
        assert_eq!(get_string1(&config, "foo").as_deref(), Some("bar"));
        assert_eq!(get_string1(&config, "java.thing").as_deref(), Some("value"));
        assert_eq!(get_string1(&config, "java.bottom.another").as_deref(), Some("thing"));
    }

    #[test]
    fn test_get_int() {
        let mut config = json!({});
        config["foo"] = json!(1);
        let mut middle = json!({});
        middle["nope"] = json!("not an int");
        let mut bottom = json!({});
        bottom["another"] = json!("3");
        middle["bottom"] = bottom;
        config["java"] = middle;

        assert_eq!(0, get_int(&config, "missing", 0));
        assert_eq!(1, get_int(&config, "foo", 0));
        assert_eq!(2, get_int(&config, "java.nope", 2));
        assert_eq!(3, get_int(&config, "java.bottom.another", 0));
    }

    #[test]
    fn test_get_boolean() {
        let mut config = json!({});
        config["foo"] = json!(true);
        let mut middle = json!({});
        middle["thing"] = json!("TRUE");
        let mut bottom = json!({});
        bottom["another"] = json!(true);
        middle["bottom"] = bottom;
        config["java"] = middle;

        assert!(get_boolean(&config, "foo", false));
        assert!(get_boolean(&config, "java.thing", false));
        assert!(get_boolean(&config, "java.default", true));
        assert!(!get_boolean(&config, "java.missing", false));
        assert!(get_boolean(&config, "java.bottom.another", false));
    }

    #[test]
    fn test_get_list() {
        let mut config = json!({});
        config["foo"] = json!("['a', 'b']");
        let middle = json!({});
        config["java"] = middle;
        let mut bottom = json!({});
        bottom["another"] = json!("c, d");
        config["java"]["bottom"] = bottom;

        let foo = get_list(&config, "foo", None);
        assert!(foo.is_some());
        let foo = foo.unwrap();
        assert_eq!("a", foo[0]);
        assert_eq!("b", foo[1]);

        let nope = get_list(&config, "java", None);
        assert!(nope.is_none());

        let def: Vec<String> = Vec::new();
        let ptr = def.as_ptr();
        let same = get_list(&config, "java", Some(def)).unwrap();
        assert!(std::ptr::eq(ptr, same.as_ptr()));

        let bar = get_list(&config, "java.bottom.another", None);
        assert!(bar.is_some());
        let bar = bar.unwrap();
        assert_eq!("c", bar[0]);
        assert_eq!("d", bar[1]);

        config["args"] = json!("a  b");
        let args = get_list(&config, "args", None);
        assert!(args.is_some());
        let args = args.unwrap();
        assert_eq!(2, args.len());
        assert_eq!("a", args[0]);
        assert_eq!("b", args[1]);
        config["args"] = json!("a  b");

        config["args2"] = json!("a");
        let args2 = get_list(&config, "args2", None);
        assert!(args2.is_some());
        let args2 = args2.unwrap();
        assert_eq!(1, args2.len());
        assert_eq!("a", args2[0]);
    }

    #[test]
    fn test_get_value() {
        let mut config = json!({});
        config["foo"] = json!(1);
        let middle = json!({});
        config["java"] = middle;
        let mut bottom = json!({});
        bottom["another"] = json!(2);
        config["java"]["bottom"] = bottom;

        assert!(std::ptr::eq(&config["java"]["bottom"], get_value(&config, "java.bottom").unwrap()));
    }
}
