# jdtls-rust Progress

## Done

### Infrastructure
- [x] LSP server (tower-lsp 0.20, stdio, Content-Length framing)
- [x] Document store — incremental updates via ropey + tree-sitter
- [x] Config parsing from `initializationOptions` (javaHome, classpath, sourceCompatibility)
- [x] ECJ subprocess (spawn, newline-delimited JSON, oneshot response routing)
- [x] Embedded JAR via `include_bytes!` + `once_cell` extraction to `/tmp`

### Tree-sitter (syntax, no ECJ needed)
- [x] Semantic tokens (21-type legend)
- [x] Folding ranges (class/method bodies, import groups, block comments)
- [x] Document symbols / outline (nested: class → method → field)
- [x] Parse-error diagnostics (ERROR nodes → red squiggles)
- [x] Static Java snippets + keyword completions (for, if, sout, @Test, …)

### ECJ (semantic)
- [x] Full compilation diagnostics with precise line/col offsets
- [x] JDK boot classpath via `jrt:/` filesystem (Java 9+ modules)
- [x] serde `camelCase` on all bridge response structs (incl. BridgeRange)
- [x] AST-based completion: locals, params, fields, methods from all types in file
- [x] Import completions: top-level packages + classes per package from `jrt:/`
- [x] Import completions: correct word-range insertion (label=FQN, insertText=relative suffix)
- [x] Hover: renders method/field/type signature + Javadoc as Markdown
- [x] Goto-definition / goto-declaration (same-file AST)
- [x] Find references + document highlight (same-file AST, scope-aware)
- [x] Signature help (active parameter tracking, end-position based)
- [x] Code formatting (google-java-format)
- [x] Code actions: add import quick-fix (scans jrt:/ for unresolved type names)
- [x] Code actions: unused variable — remove all assignments (preferred), @SuppressWarnings (var + method scope), add final modifier
- [x] Code actions: add @Override, add throws clause, wrap with try/catch
- [x] Code actions: organize imports (via source.organizeImports)
- [x] Organize imports command (standalone)
- [x] Rename refactoring (text-based, same-file via AstNavigationService + ECJ WorkspaceEdit)
- [x] Inlay hints: parameter name hints at call sites (source-based, non-trivial args only)
- [x] 400 ms compile debounce (watch channel in server.rs)
- [x] `JrtClasspathEntry.moduleList` static cache (double-checked locking, per-JVM lifetime)
- [x] Workspace symbol search (fuzzy match against open-file document symbols)
- [x] Immediate diagnostic publish on save (`did_save` bypasses debounce when ECJ is ready)
- [x] `additional_edits` forwarded from ECJ completions as LSP `additionalTextEdits` (auto-import on accept)
- [x] ECJ `category_id` used as fallback diagnostic code when no string code is present
- [x] Format response URI validated against request URI
- [x] `organize_imports` wired into code action handler as `source.organizeImports`
- [x] Child process killed on `EcjInner` drop (no zombie processes)
- [x] `DocumentStore.all_contents()` filters to Java files only (non-Java files not sent to ECJ)
- [x] `DocumentStore` uses `JavaParser` directly (centralised tree-sitter init, no duplicate language setup)

### Bug fixes
- [x] **UTF-16 panic** — `detect_import_prefix` sliced `&str` using a UTF-16 column as a byte index; fixed with `utf16_col_to_byte()` helper (was: `panicked at … byte index 9 is not a char boundary; it is inside 'ž'`)

---

## Known Bugs

| # | Severity | Description |
|---|----------|-------------|
| 1 | Low | Goto-type-definition / goto-implementation return empty — `AstNavigationService.navigate()` only handles `Definition`/`Declaration` |
| 2 | Low | `CompletionService` jrt:/ index built lazily on first import completion — first call can block ~1 s |
| 3 | Low | Inlay hints only cover methods declared in open source files — JDK/library methods get no hints |

---

## Next Steps (priority order)

### 1 — Cross-file navigation
- [ ] `AstNavigationService.navigate()`: when same-file lookup fails, scan all open files for matching type/method declaration
- [ ] `AstNavigationService.findReferences()`: search all open files, not just target URI

### 2 — Call hierarchy
- [ ] `server.rs`: implement `prepare_call_hierarchy`, `incoming_calls`, `outgoing_calls`
- [ ] `AstNavigationService.java`: add `callHierarchy()` — find all call sites using multi-file find-references

### 3 — Type hierarchy
- [ ] `AstNavigationService.java`: add `typeHierarchy()` — walk type declarations, resolve extends/implements chains
- [ ] `server.rs`: implement `prepare_type_hierarchy`, `supertypes`, `subtypes`

### 4 — Goto-type-definition / goto-implementation
- [ ] `AstNavigationService.navigate()`: handle `TypeDefinition` kind (find declaring type of variable/param)
- [ ] Handle `Implementation` kind (find concrete implementations of interface method)

### 5 — Code lenses
- [ ] Reference count lenses above method/class declarations
- [ ] "Run / Debug" lenses above `main` methods and `@Test` methods

### 6 — "Add Javadoc comment" code action
- [ ] Detect missing Javadoc on public methods/classes
- [ ] Generate stub Javadoc with `@param` / `@return` tags
