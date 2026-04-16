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
- [x] Completion scoped to enclosing class + superclass chain only (no sibling-class bleed)
- [x] Inherited methods sort before Object methods in member-access completions (`super.`, `this.`)
- [x] `super(args)` constructor call completions when typing `super` inside a constructor body
- [x] ECJ `category_id` used as fallback diagnostic code when no string code is present
- [x] Format response URI validated against request URI
- [x] `organize_imports` wired into code action handler as `source.organizeImports`
- [x] Child process killed on `EcjInner` drop (no zombie processes)
- [x] `DocumentStore.all_contents()` filters to Java files only (non-Java files not sent to ECJ)
- [x] `DocumentStore` uses `JavaParser` directly (centralised tree-sitter init, no duplicate language setup)
- [x] Call hierarchy: `prepareCallHierarchy`, `incomingCalls`, `outgoingCalls` (multi-file AST search)
- [x] Goto-type-definition: resolves declared type of variable/param at cursor
- [x] Goto-implementation: finds classes that extend/implement the type and override the method
- [x] Type hierarchy: `prepareTypeHierarchy`, `supertypes`, `subtypes` (AST extends/implements chain, open files)
- [x] Inlay hints: JDK/library methods covered via `IMethodBinding.getParameterNames()` (binding-based fallback)
- [x] jrt:/ index built eagerly on startup in background thread (no more ~1 s block on first import completion)
- [x] Code actions: "Add Javadoc comment" — generates stub with `@param`, `@return`, `@throws` tags
- [x] Same-file type completions: all `TypeDeclaration`/`EnumDeclaration`/`RecordDeclaration` names offered as completions with sort priority "1-" (before JDK types at "3-"), so `extends Foo|` always surfaces same-file classes first
- [x] Javadoc hover: paragraph breaks from `<p>` tags preserved through `normalizeWhitespace` (was collapsing to wall of text); `@see` class/member references rendered as inline code
- [x] Code lens: 0 refs = clickable no-op (empty showReferences), 1 ref = direct showReferences to that location, 2+ = showReferences popup
- [x] Code actions: "Add all missing imports" (bulk import for all unresolved types in file)
- [x] Code actions: "Remove unused import" / "Remove all unused imports"
- [x] Code actions: "Change to 'X' (pkg)" — similarity-based type rename suggestions (public packages only, max 5)
- [x] Code actions: "Generate Getter/Setter/both" — field detection via line-based AST visitor (NodeFinder fails on leading whitespace)
- [x] Hover suppressed when ECJ reports an error overlapping the hovered symbol

### Bug fixes
- [x] **UTF-16 panic** — `detect_import_prefix` sliced `&str` using a UTF-16 column as a byte index; fixed with `utf16_col_to_byte()` helper (was: `panicked at … byte index 9 is not a char boundary; it is inside 'ž'`)
- [x] **Inlay hints timeout** — `BridgeResponse::InlayHintsResult` variant in Rust expected tag `"inlayHintsResult"` but Java sends `"inlayHints"`; renamed variant to `InlayHints`
- [x] **Rename returning empty edits** — `new_name` field serialised as `"new_name"` (snake_case) but Java Gson expects `"newName"` (camelCase); added `#[serde(rename = "newName")]` on that field
- [x] **Rename local-var scope bug** — `declarationFromName` used the `VariableDeclarationStatement` / `SingleVariableDeclaration` node itself as the scope, so `referenceLocations` only walked inside the declaration and missed all usages; fixed to use `localScope(owner)` / `localScope(parent)` (enclosing block/method)
- [x] **Inlay hints char/null literals skipped** — `CharacterLiteral` and `NullLiteral` were in the skip-list alongside `SimpleName`; removed them so parameter-name hints are shown for those argument types
- [x] **Inlay hints hang on Java 25** — `setEnvironment(cp, null, null, true)` (includeRunningVMBootclasspath) in the inlay-hints binding parser and in `parseWithBindings` was causing hangs on Java 25.0.2; changed to `false`

---

## Known Bugs

- `List;⁎impo` artifact appearing in completions at `int a |` cursor position (source unknown)
- Class completions in `new` expression context insert only `ClassName`, not `ClassName<>()`
- Hover class name not styled reddish (matches jdtls behaviour)

---

## Next Steps (priority order)

No pending items known.
