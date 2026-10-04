//! Port of `CoreJavaDocSnippetStringEvaluator` with the jdt.ls overrides from
//! `JavadocContentAccess2.JdtLsJavadocAccessImpl.createSnippetEvaluator`
//! (Markdown emphasis instead of HTML tags, `SNIPPET` line markers, `@link`
//! regions converted to Markdown).

use crate::javadoc::doc_ast::{DocNode, DocSource};
use crate::javadoc::SNIPPET;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntervalStatus {
    Before,
    After,
    Within,
    Encompass,
    PrevOverlap,
    PostOverlap,
    Default,
}

#[derive(Clone, Debug)]
struct ActionElement {
    start: i64,
    end: i64,
    start_tag: String,
    end_tag: String,
}

impl ActionElement {
    fn interval_status(&self, rs: i64, re: i64) -> IntervalStatus {
        let start_diff = rs - self.start;
        let end_diff = re - self.end;
        if self.end <= rs {
            IntervalStatus::After
        } else if self.start >= re {
            IntervalStatus::Before
        } else if start_diff <= 0 && end_diff >= 0 {
            IntervalStatus::Encompass
        } else if start_diff > 0 && end_diff < 0 {
            IntervalStatus::Within
        } else if start_diff > 0 {
            IntervalStatus::PostOverlap
        } else if end_diff < 0 {
            IntervalStatus::PrevOverlap
        } else {
            IntervalStatus::Default
        }
    }
}

pub struct SnippetEvaluator<'a> {
    doc: &'a DocSource,
}

/// UTF-16 helpers: Java string indices.
fn u16s(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
fn from16(v: &[u16]) -> String {
    String::from_utf16_lossy(v)
}
fn byte_to_u16(s: &str, byte: usize) -> i64 {
    s[..byte].encode_utf16().count() as i64
}
/// `String.indexOf(needle, from)` in UTF-16 units.
fn index_of(hay: &[u16], needle: &[u16], from: i64) -> i64 {
    let from = from.max(0) as usize;
    if needle.is_empty() {
        return if from <= hay.len() { from as i64 } else { -1 };
    }
    if needle.len() > hay.len() {
        return -1;
    }
    for i in from..=hay.len() - needle.len() {
        if &hay[i..i + needle.len()] == needle {
            return i as i64;
        }
    }
    -1
}

/// `String.stripLeading()`
fn strip_leading(s: &str) -> &str {
    s.trim_start_matches(|c: char| c.is_whitespace())
}

impl<'a> SnippetEvaluator<'a> {
    pub fn new(doc: &'a DocSource) -> Self {
        Self { doc }
    }

    fn node(&self, id: usize) -> &'a DocNode {
        &self.doc.nodes[id]
    }

    /// `AddTagElementString(snippetTag, buffer)`
    pub fn add_tag_element_string(&self, snippet_tag: usize, buf: &mut String) {
        for &f in &self.node(snippet_tag).fragments {
            buf.push_str(&self.one_tag_element_string(snippet_tag, f));
        }
    }

    /// jdt.ls `getOneTagElementString`: `SNIPPET` + the upstream result.
    fn one_tag_element_string(&self, snippet_tag: usize, fragment: usize) -> String {
        let f = self.node(fragment);
        let mut s = String::new();
        if f.is_abstract_text() {
            let tags = self.tags_for_text_element(snippet_tag, fragment);
            s = self.modified_string(f.text(), &tags);
        } else if f.t == "region" {
            if f.dummy {
                let tags = self.tags_for_dummy_region(snippet_tag, fragment);
                let first = f.fragments.first().map(|&i| self.node(i).text()).unwrap_or("");
                s = self.modified_string(first, &tags);
            }
        } else if f.is_tag() {
            let tags = self.tags_for_tag_element(snippet_tag, fragment);
            s = self.modified_string_for_tag_element(fragment, &tags);
        }
        format!("{SNIPPET}{}", strip_leading(&s))
    }

    /// jdt.ls `getModifiedStringForTagElement`
    fn modified_string_for_tag_element(&self, tag: usize, tags: &[usize]) -> String {
        let t = self.node(tag);
        let first = t.fragments.first().map(|&i| self.original_snippet_text(self.node(i))).unwrap_or_default();
        let mut s = self.modified_string(&first, tags);
        if t.tag_name() == Some("@link") {
            let units = u16s(&s);
            let mut leading = 0usize;
            while units.len() > leading + 1 && units[leading] == b' ' as u16 {
                leading += 1;
            }
            if let Some(md) = crate::javadoc::converter::javadoc_to_markdown(Some(&s)) {
                s = format!("{}{}  \n", " ".repeat(leading), md);
            } else {
                s = format!("{}  \n", " ".repeat(leading));
            }
        }
        s
    }

    /// JDT 3.46's later Maven build removes the whitespace before an inline
    /// directive from TextElement.text. Its source range still contains it;
    /// restore that data to match the JDT build bundled with jdt.ls 1.58.
    fn original_snippet_text(&self, node: &DocNode) -> String {
        let text = node.text();
        let raw = u16s(&self.doc.raw);
        let (start, end) = (node.s.max(0) as usize, (node.s + node.l).max(0) as usize);
        let Some(span) = raw.get(start..end) else { return text.to_owned() };
        let span = from16(span);
        let suffix_start = span.trim_end_matches([' ', '\t']).len();
        let suffix = &span[suffix_start..];
        let body = text.trim_end_matches(['\r', '\n']);
        if !suffix.is_empty() && !body.ends_with(suffix)
            && span[..suffix_start].ends_with(body.trim_start())
        {
            format!("{body}{suffix}{}", &text[body.len()..])
        } else {
            text.to_owned()
        }
    }

    /// `getModifiedString(String, List<TagElement>)`
    fn modified_string(&self, s: &str, tags: &[usize]) -> String {
        let mut actions: Vec<ActionElement> = Vec::new();
        let mut modified = s.to_owned();
        for &t in tags {
            match self.node(t).tag_name() {
                Some("@highlight") => self.handle_highlight(&modified, t, &mut actions),
                Some("@replace") => modified = self.handle_replace(&modified, t, &mut actions),
                Some("@link") => self.handle_link(&modified, t, &mut actions),
                _ => {}
            }
        }
        get_string(&modified, &actions)
    }

    fn tag_regions(&self, snippet_tag: usize) -> Vec<usize> {
        self.node(snippet_tag)
            .fragments
            .iter()
            .copied()
            .filter(|&f| self.node(f).t == "region" && !self.node(f).dummy)
            .collect()
    }

    fn regions_containing(&self, snippet_tag: usize, elem: usize) -> Vec<usize> {
        let e = self.node(elem);
        let doc_elem = matches!(e.t.as_str(), "text" | "doctext" | "tag" | "region" | "name" | "memberRef" | "methodRef" | "tagProperty");
        if !doc_elem {
            return Vec::new();
        }
        self.tag_regions(snippet_tag)
            .into_iter()
            .filter(|&r| {
                let rn = self.node(r);
                rn.s <= e.s && rn.end() >= e.end()
            })
            .collect()
    }

    fn regions_starting_at(&self, snippet_tag: usize, elem: usize) -> Vec<usize> {
        if !self.node(elem).is_abstract_text() {
            return Vec::new();
        }
        self.tag_regions(snippet_tag)
            .into_iter()
            .filter(|&r| self.node(r).props.as_ref().and_then(|p| p.region_text) == Some(elem))
            .collect()
    }

    fn inline_count(&self, t: usize) -> Option<i64> {
        self.node(t).props.as_ref().and_then(|p| p.inline_tag_count)
    }

    /// Inserts `tag` before the first element with a larger inline tag count.
    fn insert_ordered(&self, list: &mut Vec<usize>, tag: usize, val: i64, buggy_compare: bool) {
        let mut add_before = None;
        for (i, &e) in list.iter().enumerate() {
            if let Some(v2) = self.inline_count(e) {
                let v2 = if buggy_compare { val } else { v2 };
                if v2 > val {
                    add_before = Some(i);
                    break;
                }
            }
        }
        match add_before {
            None => list.push(tag),
            Some(i) => list.insert(i, tag),
        }
    }

    fn collect(&self, master: &[usize], regions: &[usize], buggy_compare: bool) -> Vec<usize> {
        let mut tags = Vec::new();
        for &r in master {
            tags.extend(self.node(r).tags.iter().copied());
        }
        for &r in regions {
            for &t in &self.node(r).tags {
                if self.node(t).is_tag() {
                    if let Some(val) = self.inline_count(t) {
                        self.insert_ordered(&mut tags, t, val, buggy_compare);
                    }
                }
            }
        }
        tags
    }

    fn tags_for_text_element(&self, snippet_tag: usize, te: usize) -> Vec<usize> {
        let regions = self.regions_starting_at(snippet_tag, te);
        let master: Vec<usize> =
            self.regions_containing(snippet_tag, te).into_iter().filter(|r| !regions.contains(r)).collect();
        self.collect(&master, &regions, false)
    }

    fn tags_for_dummy_region(&self, snippet_tag: usize, region: usize) -> Vec<usize> {
        let Some(&te) = self.node(region).fragments.first() else { return Vec::new() };
        let mut regions = self.regions_starting_at(snippet_tag, te);
        let master: Vec<usize> =
            self.regions_containing(snippet_tag, te).into_iter().filter(|r| !regions.contains(r)).collect();
        regions.push(region);
        self.collect(&master, &regions, false)
    }

    fn tags_for_tag_element(&self, snippet_tag: usize, tag: usize) -> Vec<usize> {
        let Some(&te) = self.node(tag).fragments.first() else { return Vec::new() };
        let regions = self.regions_starting_at(snippet_tag, te);
        let master: Vec<usize> =
            self.regions_containing(snippet_tag, te).into_iter().filter(|r| !regions.contains(r)).collect();
        // upstream compares with the wrong variable here (`prop` instead of `prop2`)
        let mut tags = self.collect(&master, &regions, true);
        if let Some(val) = self.inline_count(tag) {
            self.insert_ordered(&mut tags, tag, val, false);
        }
        tags
    }

    fn property(&self, tag: usize, name: &str) -> Option<&'a DocNode> {
        self.node(tag)
            .tag_properties
            .iter()
            .map(|&p| self.node(p))
            .find(|p| p.t == "tagProperty" && p.name.as_deref() == Some(name))
    }

    fn property_value(&self, tag: usize, name: &str) -> Option<String> {
        self.property(tag, name).and_then(|p| p.string_value.clone())
    }

    fn highlight_tag(&self, tag: usize) -> &'static str {
        match self.property_value(tag, "type").as_deref() {
            None => "**",
            Some("bold") => "**",
            Some("italic") => "*",
            Some("highlighted") => "***",
            Some(_) => "",
        }
    }

    fn handle_highlight(&self, text: &str, tag: usize, actions: &mut Vec<ActionElement>) {
        let def = self.highlight_tag(tag);
        let (st, et) = (def.to_owned(), def.to_owned());
        if let Some(re) = self.property_value(tag, "regex") {
            let Ok(re) = regex::Regex::new(&re) else { return };
            for m in re.find_iter(text) {
                actions.push(ActionElement {
                    start: byte_to_u16(text, m.start()),
                    end: byte_to_u16(text, m.end()),
                    start_tag: st.clone(),
                    end_tag: et.clone(),
                });
            }
        } else if let Some(sub) = self.property_value(tag, "substring") {
            let hay = u16s(text);
            let needle = u16s(&sub);
            let mut start = 0i64;
            loop {
                start = index_of(&hay, &needle, start);
                if start == -1 {
                    break;
                }
                actions.push(ActionElement { start, end: start + needle.len() as i64, start_tag: st.clone(), end_tag: et.clone() });
                start += needle.len() as i64;
                if needle.is_empty() {
                    break;
                }
            }
        } else {
            actions.push(ActionElement { start: 0, end: u16s(text).len() as i64, start_tag: st, end_tag: et });
        }
    }

    fn handle_replace(&self, text: &str, tag: usize, actions: &mut Vec<ActionElement>) -> String {
        let substitution = self.property_value(tag, "replacement").unwrap_or_default();
        let sub_len = u16s(&substitution).len() as i64;
        if let Some(re) = self.property_value(tag, "regex") {
            let Ok(re) = regex::Regex::new(&re) else { return text.to_owned() };
            let mut out = String::new();
            let mut last = 0usize;
            for caps in re.captures_iter(text) {
                let m = caps.get(0).unwrap();
                modify_prev_action_items(byte_to_u16(text, m.start()), byte_to_u16(text, m.end()), sub_len, actions);
                out.push_str(&text[last..m.start()]);
                let mut rep = String::new();
                caps.expand(&substitution.replace("\\$", "$$"), &mut rep);
                out.push_str(&rep);
                last = m.end();
            }
            out.push_str(&text[last..]);
            out
        } else if let Some(sub) = self.property_value(tag, "substring") {
            let needle = u16s(&sub);
            let subst = u16s(&substitution);
            let mut modified = u16s(text);
            let mut start = 0i64;
            loop {
                start = index_of(&modified, &needle, start);
                if start == -1 {
                    break;
                }
                modify_prev_action_items(start, start + needle.len() as i64, sub_len, actions);
                let s = start as usize;
                let mut next = modified[..s].to_vec();
                next.extend_from_slice(&subst);
                next.extend_from_slice(&modified[s + needle.len()..]);
                modified = next;
                start += subst.len() as i64;
                if needle.is_empty() {
                    break;
                }
            }
            from16(&modified)
        } else {
            actions.clear();
            substitution
        }
    }

    fn handle_link(&self, text: &str, tag: usize, actions: &mut Vec<ActionElement>) {
        let mut add_start = match self.property_value(tag, "type").as_deref() {
            Some("linkplain") => String::new(),
            _ => "code".to_owned(),
        };
        let mut add_end = String::new();
        if !add_start.is_empty() {
            add_end = format!("</{add_start}>");
            add_start = format!("<{add_start}>");
        }
        let target = self.property(tag, "target").and_then(|p| p.node_value);
        let link_ref = self.link_ref(target);
        let start_tag = format!("{link_ref}{add_start}");
        let end_tag = format!("{add_end}</a>");
        if let Some(re) = self.property_value(tag, "regex") {
            let Ok(re) = regex::Regex::new(&re) else { return };
            for m in re.find_iter(text) {
                actions.push(ActionElement {
                    start: byte_to_u16(text, m.start()),
                    end: byte_to_u16(text, m.end()),
                    start_tag: start_tag.clone(),
                    end_tag: end_tag.clone(),
                });
            }
        } else if let Some(sub) = self.property_value(tag, "substring") {
            let hay = u16s(text);
            let needle = u16s(&sub);
            let mut start = 0i64;
            loop {
                start = index_of(&hay, &needle, start);
                if start == -1 {
                    break;
                }
                actions.push(ActionElement { start, end: start + needle.len() as i64, start_tag: start_tag.clone(), end_tag: end_tag.clone() });
                start += needle.len() as i64;
                if needle.is_empty() {
                    break;
                }
            }
        } else {
            let sub_text = text.trim_matches(|c: char| c <= ' ');
            let tl = u16s(text).len();
            let sl = u16s(sub_text).len();
            if sl < tl {
                let start = index_of(&u16s(text), &u16s(sub_text), 0);
                actions.push(ActionElement { start, end: start + sl as i64, start_tag, end_tag });
            }
        }
    }

    /// `getLinkRef`: an `eclipse-javadoc:` anchor (unwrapped by the Markdown
    /// converter), or nothing for non-reference targets.
    fn link_ref(&self, target: Option<usize>) -> String {
        let Some(t) = target.map(|t| self.node(t)) else { return String::new() };
        let ref_type = match t.t.as_str() {
            "name" => t.fqn.clone(),
            "memberRef" | "methodRef" => Some(t.qualifier.clone().unwrap_or_default()),
            _ => None,
        };
        match ref_type {
            Some(name) => format!("<a href='eclipse-javadoc:%E2%98%82{}'>", name),
            None => String::new(),
        }
    }
}

fn modify_prev_action_items(rs: i64, re: i64, new_len: i64, actions: &mut Vec<ActionElement>) {
    let old_len = re - rs;
    let diff = new_len - old_len;
    let mut i = 0;
    while i < actions.len() {
        let status = actions[i].interval_status(rs, re);
        match status {
            IntervalStatus::After | IntervalStatus::Default => {}
            IntervalStatus::Before => {
                if diff != 0 {
                    actions[i].start += diff;
                    actions[i].end += diff;
                }
            }
            IntervalStatus::Encompass => {
                actions.remove(i);
                continue;
            }
            IntervalStatus::PostOverlap => actions[i].end = rs,
            IntervalStatus::PrevOverlap => {
                actions[i].end += diff;
                actions[i].start = re + diff;
            }
            IntervalStatus::Within => {
                let new_end = actions[i].end + diff;
                actions[i].end = rs;
                let new_start = re + diff;
                let e = ActionElement {
                    start: new_start,
                    end: new_end,
                    start_tag: actions[i].start_tag.clone(),
                    end_tag: actions[i].end_tag.clone(),
                };
                // ListIterator.add inserts after the current element and the
                // iteration continues after the inserted one.
                actions.insert(i + 1, e);
                i += 1;
            }
        }
        i += 1;
    }
}

struct StringItem {
    index: i64,
    tag: String,
}

/// `getString(str, actionElements)`
fn get_string(s: &str, actions: &[ActionElement]) -> String {
    let mut items: Vec<StringItem> = Vec::new();
    for a in actions {
        let start_item = StringItem { index: a.start, tag: a.start_tag.clone() };
        let end_item = StringItem { index: a.end, tag: a.end_tag.clone() };
        let mut end_added = false;
        let mut start_added = false;
        let mut start_item = Some(start_item);
        let mut end_item = Some(end_item);
        let mut i = 0;
        while i < items.len() {
            if !end_added && items[i].index < end_item.as_ref().unwrap().index {
                items.insert(i, end_item.take().unwrap());
                end_added = true;
                i += 1;
            }
            if !start_added && items[i].index < start_item.as_ref().unwrap().index {
                items.insert(i, start_item.take().unwrap());
                start_added = true;
                i += 1;
            }
            if start_added && end_added {
                break;
            }
            i += 1;
        }
        if !end_added {
            items.push(end_item.take().unwrap());
        }
        if !start_added {
            items.push(start_item.take().unwrap());
        }
    }
    // stable sort: index descending, end tags ("</") first
    items.sort_by(|a, b| {
        if b.index != a.index {
            return b.index.cmp(&a.index);
        }
        let a_end = a.tag.starts_with("</");
        let b_end = b.tag.starts_with("</");
        b_end.cmp(&a_end)
    });
    let mut out = u16s(s);
    for item in items {
        let idx = (item.index.max(0) as usize).min(out.len());
        let tag = u16s(&item.tag);
        out.splice(idx..idx, tag);
    }
    from16(&out)
}
