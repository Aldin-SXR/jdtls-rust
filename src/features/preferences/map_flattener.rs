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
            match serde_json::from_str::<Value>(&s) {
                Ok(Value::Array(a)) => Some(strings(&a)),
                _ => def,
            }
        }
        Some(Value::Array(a)) => Some(strings(a)),
        _ => def,
    }
}
