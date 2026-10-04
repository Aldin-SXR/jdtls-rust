//! A port of the `org.eclipse.text.edits` tree semantics that
//! `ASTRewrite` produces: insert / delete / replace edits, copy and move
//! source/target pairs (with indentation-changing source modifiers), range
//! markers and multi edits.  Offsets are UTF-16 code units.

use super::indent;

#[derive(Clone, Debug)]
pub enum EditKind {
    /// `MultiTextEdit` without a defined region (its region is its children's).
    Multi,
    Insert(String),
    Delete,
    Replace(String),
    RangeMarker,
    CopySource,
    MoveSource,
    /// Target of the copy / move source edit with the given index.
    CopyTarget(usize),
    MoveTarget(usize),
}

/// `SourceModifier`: re-indents the copied text.
#[derive(Clone, Debug)]
pub struct SourceModifier {
    pub source_indent_level: i32,
    pub destination_indent: String,
    pub tab_width: i32,
    pub indent_width: i32,
}

impl SourceModifier {
    /// `SourceModifier.getModifications(source)` applied to `source`.
    pub fn apply(&self, source: &[u16]) -> Vec<u16> {
        let dest = String::from_utf16_lossy(&indent::to_u16(&self.destination_indent));
        let dest_level = indent::measure_indent_units(&indent::to_u16(&dest), self.tab_width, self.indent_width);
        if dest_level == self.source_indent_level {
            return source.to_vec();
        }
        let edits = indent::get_change_indent_edits(source, self.source_indent_level, self.tab_width, self.indent_width, &self.destination_indent);
        apply_flat(source, &edits)
    }
}

#[derive(Clone, Debug)]
pub struct Edit {
    pub offset: i32,
    pub length: i32,
    pub kind: EditKind,
    pub children: Vec<usize>,
    pub parent: Option<usize>,
    pub modifier: Option<SourceModifier>,
    /// `MultiTextEdit.fDefined`: the region was fixed when it was added to
    /// a parent (`defineRegion`).
    pub defined: bool,
}

/// Overlapping or misplaced edits (`MalformedTreeException`).
#[derive(Debug, Clone)]
pub struct MalformedTree(pub String);

#[derive(Clone, Debug)]
pub struct EditTree {
    pub edits: Vec<Edit>,
}

impl Default for EditTree {
    fn default() -> Self {
        Self::new()
    }
}

impl EditTree {
    /// A tree whose root (index 0) is an undefined `MultiTextEdit`.
    pub fn new() -> Self {
        EditTree { edits: vec![Edit { offset: 0, length: 0, kind: EditKind::Multi, children: Vec::new(), parent: None, modifier: None, defined: false }] }
    }

    pub const ROOT: usize = 0;

    pub fn new_edit(&mut self, offset: i32, length: i32, kind: EditKind) -> usize {
        self.edits.push(Edit { offset, length, kind, children: Vec::new(), parent: None, modifier: None, defined: false });
        self.edits.len() - 1
    }

    /// `TextEdit.getOffset()` (undefined multi edits span their children).
    pub fn offset(&self, e: usize) -> i32 {
        let ed = &self.edits[e];
        match ed.kind {
            EditKind::Multi if !ed.defined => ed.children.first().map(|&c| self.offset(c)).unwrap_or(0),
            _ => ed.offset,
        }
    }

    pub fn length(&self, e: usize) -> i32 {
        let ed = &self.edits[e];
        match ed.kind {
            EditKind::Multi if !ed.defined => match (ed.children.first(), ed.children.last()) {
                (Some(&f), Some(&l)) => self.offset(l) - self.offset(f) + self.length(l),
                _ => 0,
            },
            _ => ed.length,
        }
    }

    fn exclusive_end(&self, e: usize) -> i32 {
        self.offset(e) + self.length(e)
    }

    fn is_defined(&self, e: usize) -> bool {
        !matches!(self.edits[e].kind, EditKind::Multi) || self.edits[e].defined
    }

    /// `INSERTION_COMPARATOR`.
    fn compare(&self, a: usize, b: usize) -> Result<std::cmp::Ordering, MalformedTree> {
        let (o1, l1, o2, l2) = (self.offset(a), self.length(a), self.offset(b), self.length(b));
        if o1 == o2 && l1 == 0 && l2 == 0 {
            return Ok(std::cmp::Ordering::Equal);
        }
        if o1 + l1 <= o2 {
            return Ok(std::cmp::Ordering::Less);
        }
        if o2 + l2 <= o1 {
            return Ok(std::cmp::Ordering::Greater);
        }
        Err(MalformedTree(format!("Overlapping text edits: [{o1},{l1}] and [{o2},{l2}]")))
    }

    /// `TextEdit.addChild(child)`.
    pub fn add_child(&mut self, parent: usize, child: usize) -> Result<(), MalformedTree> {
        if self.is_defined(parent) {
            let (po, pl) = (self.offset(parent), self.length(parent));
            let (co, cl) = (self.offset(child), self.length(child));
            if !(po <= co && co + cl <= po + pl) {
                return Err(MalformedTree("Range of child edit lies outside of parent edit".into()));
            }
        }
        let index = self.insertion_index(parent, child)?;
        self.edits[parent].children.insert(index, child);
        self.edits[child].parent = Some(parent);
        Ok(())
    }

    fn insertion_index(&self, parent: usize, child: usize) -> Result<usize, MalformedTree> {
        let children = &self.edits[parent].children;
        let size = children.len();
        if size == 0 {
            return Ok(0);
        }
        let last = children[size - 1];
        if self.exclusive_end(last) <= self.offset(child) {
            return Ok(size);
        }
        // Binary search with the insertion comparator.
        let (mut lo, mut hi) = (0usize, size);
        let mut found: Option<usize> = None;
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.compare(children[mid], child)? {
                std::cmp::Ordering::Less => lo = mid + 1,
                std::cmp::Ordering::Greater => hi = mid,
                std::cmp::Ordering::Equal => {
                    found = Some(mid);
                    break;
                }
            }
        }
        match found {
            None => Ok(lo),
            Some(mut index) => {
                while index < size - 1 && self.compare(children[index], children[index + 1])? == std::cmp::Ordering::Equal {
                    index += 1;
                }
                Ok(index + 1)
            }
        }
    }

    // ── Application ─────────────────────────────────────────────────────────

    /// Applies the tree to `text` (`TextEdit.apply`), returning the new text.
    pub fn apply(&self, text: &[u16]) -> Vec<u16> {
        let mut sources: Vec<Option<Vec<u16>>> = vec![None; self.edits.len()];
        self.render(Self::ROOT, text, &mut sources, true)
    }

    fn source_content(&self, e: usize, text: &[u16], sources: &mut Vec<Option<Vec<u16>>>) -> Vec<u16> {
        if let Some(s) = &sources[e] {
            return s.clone();
        }
        let content = self.render_region(e, text, sources);
        let content = match &self.edits[e].modifier {
            Some(m) => m.apply(&content),
            None => content,
        };
        sources[e] = Some(content.clone());
        content
    }

    /// The region of `e` in `text` with its children applied.
    fn render_region(&self, e: usize, text: &[u16], sources: &mut Vec<Option<Vec<u16>>>) -> Vec<u16> {
        let start = self.offset(e).max(0) as usize;
        let end = (self.exclusive_end(e).max(0) as usize).min(text.len());
        let mut out = Vec::new();
        let mut pos = start;
        let children = self.edits[e].children.clone();
        for c in children {
            let cs = (self.offset(c).max(0) as usize).min(text.len());
            let ce = (self.exclusive_end(c).max(0) as usize).min(text.len());
            if cs > pos {
                out.extend_from_slice(&text[pos..cs]);
            }
            out.extend(self.render(c, text, sources, false));
            pos = pos.max(ce);
        }
        if end > pos {
            out.extend_from_slice(&text[pos..end]);
        }
        out
    }

    /// The text that replaces `e`'s region once the tree is executed.
    fn render(&self, e: usize, text: &[u16], sources: &mut Vec<Option<Vec<u16>>>, root: bool) -> Vec<u16> {
        match &self.edits[e].kind {
            EditKind::Multi if root => {
                // The whole document.
                let mut out = Vec::new();
                let mut pos = 0usize;
                for &c in &self.edits[e].children.clone() {
                    let cs = (self.offset(c).max(0) as usize).min(text.len());
                    let ce = (self.exclusive_end(c).max(0) as usize).min(text.len());
                    if cs > pos {
                        out.extend_from_slice(&text[pos..cs]);
                    }
                    out.extend(self.render(c, text, sources, false));
                    pos = pos.max(ce);
                }
                if pos < text.len() {
                    out.extend_from_slice(&text[pos..]);
                }
                out
            }
            EditKind::Multi | EditKind::RangeMarker | EditKind::CopySource => {
                if matches!(self.edits[e].kind, EditKind::CopySource) {
                    // Make sure the source content is computed from the
                    // original region (children applied).
                    let _ = self.source_content(e, text, sources);
                }
                self.render_region(e, text, sources)
            }
            EditKind::MoveSource => {
                let _ = self.source_content(e, text, sources);
                Vec::new()
            }
            EditKind::Insert(s) | EditKind::Replace(s) => {
                // Children of a replace are overwritten, but sources nested
                // in it must still be computed.
                for &c in &self.edits[e].children.clone() {
                    self.compute_nested_sources(c, text, sources);
                }
                indent::to_u16(s)
            }
            EditKind::Delete => {
                for &c in &self.edits[e].children.clone() {
                    self.compute_nested_sources(c, text, sources);
                }
                Vec::new()
            }
            EditKind::CopyTarget(src) | EditKind::MoveTarget(src) => {
                let src = *src;
                self.source_content(src, text, sources)
            }
        }
    }

    fn compute_nested_sources(&self, e: usize, text: &[u16], sources: &mut Vec<Option<Vec<u16>>>) {
        for &c in &self.edits[e].children.clone() {
            self.compute_nested_sources(c, text, sources);
        }
        if matches!(self.edits[e].kind, EditKind::CopySource | EditKind::MoveSource) {
            let _ = self.source_content(e, text, sources);
        }
    }

    /// Region covered by the root (`MultiTextEdit.getOffset/getLength`), if
    /// the tree has any edits.
    pub fn covered_region(&self) -> Option<(usize, usize)> {
        if self.edits[Self::ROOT].children.is_empty() {
            return None;
        }
        let o = self.offset(Self::ROOT).max(0) as usize;
        Some((o, o + self.length(Self::ROOT).max(0) as usize))
    }

    /// Adds every child of `other`'s root under this tree's root (keeps
    /// `other`'s nesting), like `editRoot.addChild(otherRoot)`.
    pub fn add_tree(&mut self, other: &EditTree) -> Result<(), MalformedTree> {
        let base = self.edits.len();
        for (i, ed) in other.edits.iter().enumerate() {
            let mut ed = ed.clone();
            ed.children = ed.children.iter().map(|c| c + base).collect();
            ed.parent = ed.parent.map(|p| p + base);
            ed.kind = match ed.kind {
                EditKind::CopyTarget(s) => EditKind::CopyTarget(s + base),
                EditKind::MoveTarget(s) => EditKind::MoveTarget(s + base),
                k => k,
            };
            let _ = i;
            self.edits.push(ed);
        }
        // `MultiTextEdit.defineRegion(parent.getOffset())`.
        let (o, l) = if self.edits[base].children.is_empty() {
            (self.offset(Self::ROOT), 0)
        } else {
            (self.offset(base), self.length(base))
        };
        self.edits[base].offset = o;
        self.edits[base].length = l;
        self.edits[base].defined = true;
        self.add_child(Self::ROOT, base)
    }
}

/// Applies non-overlapping flat replace edits `(offset, length, text)`.
pub fn apply_flat(text: &[u16], edits: &[(usize, usize, String)]) -> Vec<u16> {
    let mut sorted: Vec<&(usize, usize, String)> = edits.iter().collect();
    sorted.sort_by_key(|e| e.0);
    let mut out = Vec::with_capacity(text.len());
    let mut pos = 0;
    for (o, l, t) in sorted {
        let o = (*o).min(text.len());
        if o > pos {
            out.extend_from_slice(&text[pos..o]);
        }
        out.extend(t.encode_utf16());
        pos = pos.max((o + l).min(text.len()));
    }
    if pos < text.len() {
        out.extend_from_slice(&text[pos..]);
    }
    out
}

// ── Position tracking (jface `Document` + `DefaultPositionUpdater`) ────────

/// A tracked position (`org.eclipse.jface.text.Position`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub offset: i32,
    pub length: i32,
    pub deleted: bool,
}

/// Replaces `[offset, offset+length)` of `doc` with `text`, updating the
/// positions like the `DefaultPositionUpdater` subclass of
/// `ASTRewriteFormatter.createDocument` (positions fully inside a deleted
/// range are moved to its end).
pub fn replace_tracking(doc: &mut Vec<u16>, offset: usize, length: usize, text: &str, positions: &mut [Position]) {
    let replace: Vec<u16> = text.encode_utf16().collect();
    let f_offset = offset as i32;
    let f_length = length as i32;
    let f_replace = replace.len() as i32;
    for pos in positions.iter_mut() {
        let original = *pos;
        // notDeleted()
        let start = f_offset;
        let end = start + f_length;
        if start < pos.offset && pos.offset + pos.length < end {
            pos.offset = end;
            continue;
        }
        // adaptToReplace()
        if pos.offset == f_offset && pos.length == f_length && pos.length > 0 {
            pos.length += f_replace - f_length;
            if pos.length < 0 {
                pos.offset += pos.length;
                pos.length = 0;
            }
        } else {
            if f_length > 0 {
                adapt_to_remove(pos, f_offset, f_length);
            }
            if f_replace > 0 {
                adapt_to_insert(pos, &original, f_offset, f_length, f_replace);
            }
        }
    }
    let end = (offset + length).min(doc.len());
    doc.splice(offset.min(doc.len())..end, replace);
}

fn adapt_to_insert(pos: &mut Position, original: &Position, f_offset: i32, f_length: i32, f_replace: i32) {
    let my_start = pos.offset;
    let my_end = (pos.offset + pos.length - 1).max(my_start);
    let yours_start = f_offset;
    if my_end < yours_start {
        return;
    }
    if f_length <= 0 {
        if my_start < yours_start {
            pos.length += f_replace;
        } else {
            pos.offset += f_replace;
        }
    } else if my_start <= yours_start && original.offset <= yours_start {
        pos.length += f_replace;
    } else {
        pos.offset += f_replace;
    }
}

fn adapt_to_remove(pos: &mut Position, f_offset: i32, f_length: i32) {
    let my_start = pos.offset;
    let my_end = (pos.offset + pos.length - 1).max(my_start);
    let yours_start = f_offset;
    let yours_end = (f_offset + f_length - 1).max(yours_start);
    if my_end < yours_start {
        return;
    }
    if my_start <= yours_start {
        if yours_end <= my_end {
            pos.length -= f_length;
        } else {
            pos.length -= my_end - yours_start + 1;
        }
    } else if yours_start < my_start {
        if yours_end < my_start {
            pos.offset -= f_length;
        } else {
            pos.offset -= my_start - yours_start;
            pos.length -= yours_end - my_start + 1;
        }
    }
    if pos.offset < 0 {
        pos.offset = 0;
    }
    if pos.length < 0 {
        pos.length = 0;
    }
}

/// `ASTRewriteFormatter.evaluateFormatterEdit(string, edit, positions)`:
/// applies the formatter's (flat) edits last to first, tracking positions.
pub fn evaluate_formatter_edits(string: &str, edits: &[(usize, usize, String)], positions: &mut [Position]) -> String {
    let mut doc: Vec<u16> = string.encode_utf16().collect();
    let mut sorted: Vec<&(usize, usize, String)> = edits.iter().collect();
    sorted.sort_by_key(|e| e.0);
    for (o, l, t) in sorted.into_iter().rev() {
        replace_tracking(&mut doc, *o, *l, t, positions);
    }
    String::from_utf16_lossy(&doc)
}
