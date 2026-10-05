//! Wire format of the bridge `semanticAst` response (`SemanticAstService`).

use serde::Deserialize;

fn minus_one() -> i32 {
    -1
}

#[derive(Debug, Default, Deserialize)]
pub struct NodeOut {
    pub t: i32,
    pub s: i32,
    pub l: i32,
    pub p: i32,
    pub loc: i32,
    pub es: i32,
    pub el: i32,
    pub b: i32,
    pub tb: i32,
    pub mb: i32,
    pub f: i32,
    #[serde(default)]
    pub pr: Vec<i32>,
    #[serde(default)]
    pub ls: Vec<Vec<i32>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct BindingOut {
    pub k: i32,
    pub key: i32,
    pub n: i32,
    pub m: i32,
    pub f: i64,
    pub qn: i32,
    pub bn: i32,
    pub pkg: i32,
    pub er: i32,
    pub td: i32,
    pub dc: i32,
    pub dm: i32,
    pub sc: i32,
    pub el: i32,
    pub cmp: i32,
    pub bound: i32,
    #[serde(default = "minus_one")]
    pub wc: i32,
    pub gt: i32,
    pub dim: i32,
    pub it: Option<Vec<i32>>,
    pub ta: Option<Vec<i32>>,
    pub tp: Option<Vec<i32>>,
    pub tbs: Option<Vec<i32>>,
    pub dmeth: Option<Vec<i32>>,
    pub dfld: Option<Vec<i32>>,
    pub dtyp: Option<Vec<i32>>,
    pub ctors: Option<Vec<i32>>,
    #[serde(rename = "type")]
    pub typ: i32,
    pub vid: i32,
    pub cv: i32,
    pub vd: i32,
    pub rt: i32,
    pub md: i32,
    pub pt: Option<Vec<i32>>,
    pub et: Option<Vec<i32>>,
    pub pn: Option<Vec<i32>>,
    pub ss: Option<Vec<i32>>,
    pub ov: Option<Vec<i32>>,
    pub assign: Option<Vec<i32>>,
    #[serde(default = "minus_one")]
    pub fim: i32,
    pub ann: Option<Vec<AnnotationOut>>,
    pub tann: Option<Vec<AnnotationOut>>,
    pub pann: Option<Vec<Vec<AnnotationOut>>>,
    #[serde(default = "minus_one", rename = "nameOffset")]
    pub name_offset: i32,
    #[serde(default = "minus_one", rename = "sourceOffset")]
    pub source_offset: i32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AnnotationOut {
    #[serde(rename = "annotationType")]
    pub annotation_type: i32,
    pub members: Vec<MemberValueOut>,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct MemberValueOut {
    pub name: i32,
    pub value: AnnotationValueOut,
}
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct AnnotationValueOut {
    pub kind: i32,
    pub text: i32,
    pub binding: i32,
    pub annotation: Option<AnnotationOut>,
    pub values: Option<Vec<AnnotationValueOut>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProblemOut {
    pub id: i32,
    pub s: i32,
    pub e: i32,
    pub line: i32,
    pub sev: i32,
    pub msg: i32,
    pub cat: i32,
    pub args: Vec<i32>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SemanticAstData {
    pub strings: Vec<String>,
    pub nodes: Vec<NodeOut>,
    pub bindings: Vec<BindingOut>,
    pub problems: Vec<ProblemOut>,
    pub comments: Vec<i32>,
    pub cache_key: Option<String>,
}
