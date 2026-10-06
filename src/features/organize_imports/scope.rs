//! `collectCompilationUnits(IPackageFragmentRoot, result, prefix)`.
//! The caller supplies the source root's compilation units and package names.

use tower_lsp::lsp_types::Url;

pub fn collect_compilation_units(units: &[(Url, String)], prefix: Option<&str>) -> Vec<Url> {
    units
        .iter()
        .filter(|(_, package)| {
            prefix.map_or(true, |prefix| {
                prefix.trim().is_empty() || package.contains(prefix)
            })
        })
        .map(|(uri, _)| uri.clone())
        .collect()
}
