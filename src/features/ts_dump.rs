//! Debug helper: `TS_DUMP=<file> cargo test --bin jdtls-rust ts_dump -- --nocapture`
//! prints the tree-sitter CST of a Java file.

#[cfg(test)]
mod tests {
    #[test]
    fn ts_dump() {
        let path = std::env::var("TS_DUMP").unwrap_or_default();
        if path.is_empty() {
            return;
        }
        let src = std::fs::read_to_string(path).unwrap();
        let mut p = tree_sitter::Parser::new();
        p.set_language(&tree_sitter_java::language()).unwrap();
        let t = p.parse(&src, None).unwrap();
        fn walk(n: tree_sitter::Node, src: &str, d: usize, field: Option<&str>) {
            let txt: String = src[n.byte_range()].chars().take(40).collect::<String>().replace('\n', "\\n");
            println!(
                "{}{}{} [{:?}-{:?}] {}",
                "  ".repeat(d),
                field.map(|f| format!("{f}: ")).unwrap_or_default(),
                n.kind(),
                (n.start_position().row, n.start_position().column),
                (n.end_position().row, n.end_position().column),
                txt
            );
            let mut c = n.walk();
            for (i, ch) in n.children(&mut c).enumerate() {
                walk(ch, src, d + 1, n.field_name_for_child(i as u32));
            }
        }
        walk(t.root_node(), &src, 0, None);
    }
}
