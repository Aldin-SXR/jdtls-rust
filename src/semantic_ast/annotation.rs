//! Compiler annotation facts; Rust constructs imports and annotation nodes.
use super::{wire, BindingId};

#[derive(Clone, Debug)]
pub struct Annotation {
    pub annotation_type: BindingId,
    pub members: Vec<(String, Value)>,
    pub all_members: Vec<(String, Value)>,
}
#[derive(Clone, Debug)]
pub enum Value {
    Missing,
    Boolean(bool),
    Number(String),
    Character(u16),
    String(String),
    Type(BindingId),
    Enum(BindingId),
    Annotation(Box<Annotation>),
    Array(Vec<Value>),
}
pub fn decode(
    o: &wire::AnnotationOut,
    string: &impl Fn(i32) -> Option<String>,
) -> Option<Annotation> {
    let annotation_type = binding(o.annotation_type)?;
    let members = o
        .members
        .iter()
        .filter_map(|m| Some((string(m.name)?, value(&m.value, string))))
        .collect();
    Some(Annotation {
        annotation_type,
        members,
        all_members: o
            .all_members
            .iter()
            .filter_map(|m| Some((string(m.name)?, value(&m.value, string))))
            .collect(),
    })
}
fn binding(id: i32) -> Option<BindingId> {
    (id >= 0).then_some(BindingId(id as u32))
}
fn value(o: &wire::AnnotationValueOut, string: &impl Fn(i32) -> Option<String>) -> Value {
    match o.kind {
        1 => Value::Boolean(string(o.text).as_deref() == Some("true")),
        2 => Value::Number(string(o.text).unwrap_or_default()),
        3 => string(o.text)
            .and_then(|s| s.parse().ok())
            .map(Value::Character)
            .unwrap_or(Value::Missing),
        4 => Value::String(string(o.text).unwrap_or_default()),
        5 => binding(o.binding)
            .map(Value::Type)
            .unwrap_or(Value::Missing),
        6 => binding(o.binding)
            .map(Value::Enum)
            .unwrap_or(Value::Missing),
        7 => o
            .annotation
            .as_ref()
            .and_then(|a| decode(a, string))
            .map(|a| Value::Annotation(Box::new(a)))
            .unwrap_or(Value::Missing),
        8 => Value::Array(
            o.values
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|v| value(v, string))
                .collect(),
        ),
        _ => Value::Missing,
    }
}
