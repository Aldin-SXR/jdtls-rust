use tree_sitter::{Parser, Tree};

/// Wraps a tree-sitter Parser for Java.
/// Not Send — keep one per thread or behind a Mutex.
pub struct JavaParser {
    inner: Parser,
}

impl JavaParser {
    pub fn new() -> Self {
        let mut p = Parser::new();
        p.set_language(&tree_sitter_java::language()).expect("tree-sitter-java language init failed");
        Self { inner: p }
    }

    pub fn parse(&mut self, source: &str, old_tree: Option<&Tree>) -> Option<Tree> {
        self.inner.parse(source.as_bytes(), old_tree)
    }

    /// Convenience: parse and return tree, panicking only if source is empty.
    pub fn parse_fresh(&mut self, source: &str) -> Option<Tree> {
        self.inner.parse(source.as_bytes(), None)
    }

}

impl Default for JavaParser {
    fn default() -> Self {
        Self::new()
    }
}
