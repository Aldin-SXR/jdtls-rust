//! ToStringTemplateParser's exact tokenization and four template regions.
const OBJECT: &[&str] = &[
    "${object.className}",
    "${object.getClassName}",
    "${object.superToString}",
    "${object.hashCode}",
    "${object.identityHashCode}",
];
const MEMBER: &[&str] = &["${member.name}", "${member.name()}", "${member.value}"];
const OTHER: &str = "${otherMembers}";
pub(super) const DEFAULT: &str =
    "${object.className} [${member.name()}=${member.value}, ${otherMembers}]";
pub(super) struct Template {
    pub beginning: Vec<String>,
    pub body: Vec<String>,
    pub separator: String,
    pub ending: Vec<String>,
}
fn extract(mut s: &str, variables: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    while !s.is_empty() {
        let found = variables
            .iter()
            .filter_map(|v| s.find(v).map(|i| (i, *v)))
            .min_by_key(|(i, _)| *i);
        let Some((i, v)) = found else {
            out.push(s.into());
            break;
        };
        if i > 0 {
            out.push(s[..i].into())
        }
        out.push(v.into());
        s = &s[i + v.len()..];
    }
    out
}
impl Template {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        // firstOccuranceOf deliberately ignores a new zero index once a
        // previous variable has already been found, as the upstream parser does.
        let mut first = None;
        for v in MEMBER {
            if let Some(i) = s.find(v) {
                if first.is_none() || i > 0 && i < first.unwrap() {
                    first = Some(i)
                }
            }
        }
        let start = first.unwrap_or(0);
        let end = s.find(OTHER).unwrap_or(s.len());
        anyhow::ensure!(
            start <= end && end + OTHER.len() <= s.len(),
            "Invalid toString template"
        );
        let beginning = if first.is_some() {
            extract(&s[..start], OBJECT)
        } else {
            Vec::new()
        };
        let all: Vec<_> = OBJECT.iter().chain(MEMBER).copied().collect();
        let mut body = extract(&s[start..end], &all);
        let separator = body.pop().unwrap_or_default();
        let ending = extract(&s[end + OTHER.len()..], OBJECT);
        Ok(Self {
            beginning,
            body,
            separator,
            ending,
        })
    }
}
