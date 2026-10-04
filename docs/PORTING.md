# Porting eclipse.jdt.ls to jdtls-rust

Goal: feature parity with eclipse.jdt.ls. Every LSP feature behaves the same way,
and the upstream test suite (`eclipse.jdt.ls/org.eclipse.jdt.ls.tests`) is replicated
exactly: same fixtures, same inputs, same expected outputs.

The upstream sources are checked out (gitignored) at `eclipse.jdt.ls/` in the main
checkout: `/Users/aldin-sxr/Documents/Code/jdtls-rust/eclipse.jdt.ls`. Read the upstream
handler and its test before you implement anything.

## Architecture rules

* **Rust first.** LSP handling, the project model, text and edit computation, AST
  navigation over tree-sitter, and result shaping all live in Rust (`src/`).
* **Java only for things that need JDT itself**: ECJ compilation and problems, binding
  resolution, the Eclipse code formatter, and JDT DOM rewrites that have no practical
  Rust equivalent. The bridge (`ecj-bridge/`) should return *data* (bindings, ranges,
  problems). Rust turns that data into LSP results. Don't put LSP-shaped logic in Java
  when Rust can do it. When you touch existing Java feature logic, prefer moving it to
  Rust.
* **Virtual documents must keep working.** A `didOpen` document with no file on disk
  (`untitled:`, `inmemory://`, or a nonexistent `file:` path) supports every feature.
  It belongs to the default project. Never require the disk.
* Projects: `src/project/` holds the Rust ports of the jdt.ls importers (Gradle → Maven
  → Eclipse → invisible). `Dispatcher::context_for(uri)` gives the files, classpath,
  compliance and compiler options for the project that owns a URI.
* Compiler options: Rust computes the effective JDT option map (jdt.ls defaults from
  `project::jdtls_default_options`, then config `compilerOptions`, then project prefs)
  and sends it with every bridge request. Java applies it in
  `BridgeOptions.map(sourceLevel)`, so always build parser and compiler options through
  `BridgeOptions`.
* Workspace files are loaded lazily from disk by `DocumentStore`. Open documents
  override them.

## The oracle: run real jdt.ls, never approximate

Get exact text, wording and behaviour from the real Java server and replicate it
byte for byte. Don't guess messages, labels, titles, sort order or edit shapes.

* The reference server is jdt.ls 1.58.0, the same version as the `eclipse.jdt.ls/`
  checkout. It's unpacked at
  `/Users/aldin-sxr/Documents/Code/jdtls-rust/.oracle/jdtls-1.58.0` (gitignored).
  `scripts/oracle-jdtls.sh <data-dir>` launches it over stdio.
* `JDTLS_ORACLE=1 cargo test --test <file>` runs any harness-based test against the
  real jdt.ls instead of ours. Use it to:
  * confirm that a ported test is faithful (it should pass against the oracle), and
  * capture the exact responses. Write a scratch test that prints the oracle's result,
    then replicate it.
  In a worktree, the script resolves `.oracle/` relative to the repo. Set
  `JDTLS_ORACLE_HOME=/Users/aldin-sxr/Documents/Code/jdtls-rust/.oracle/jdtls-1.58.0`
  if it isn't there.
* The message strings in JDT, LTK and jdt.ls (refactoring errors, quick-fix labels)
  live in `*.properties` files inside the oracle's plugin jars, for example
  `unzip -p .oracle/jdtls-1.58.0/plugins/org.eclipse.jdt.core.manipulation_*.jar '*RefactoringCoreMessages.properties'`.
  Copy them verbatim.

* Delegate commands (`workspace/executeCommand`): jdt.ls reads most arguments with
  `JSONUtility.toModel`, which accepts a JSON value *or* a JSON-encoded string, and
  returns null for a bare number, because lsp4j hands numbers over as `Double`.
  vscode-java therefore sends `JSON.stringify(...)` for many model arguments. Accept
  both forms on our side, and inspect the delegate command before writing tests:
  `java.project.refreshDiagnostics`, for example, casts directly to strings and
  booleans and requires raw arguments.

## Tests

* One Rust integration-test file per upstream test class:
  `tests/<package>_<class_snake>.rs`, for example
  `org.eclipse.jdt.ls.core.internal.handlers.FoldingRangeHandlerTest` becomes
  `tests/handlers_folding_range_handler_test.rs`. Test functions use the upstream
  method name in snake_case (`testFoldingRanges` becomes `test_folding_ranges`).
* Start every file with `mod common; use common::jdtls::...;`. The harness
  (`tests/common/jdtls.rs`) supports:
  * `Workspace::import_projects(&["maven/salut"])`: copies from
    `tests/fixtures/projects` (a verbatim copy of upstream `projects/`), like
    `importProjects`.
  * `new_empty_project(&test_default_options())` and `create_cu(root, "src", "test1", "E.java", src)`,
    which correspond to `newEmptyProject` and `createCompilationUnit`.
  * `class_uri(project, fqn)`, `request(method, params)`, `open`, `change`, and
    `diagnostics(uri)`, which waits for `publishDiagnostics` after a fresh build.
  * `apply_edits`, `dos2unix`, `pos`, and `range`.
  * `client().request_results`: canned results for server→client requests by
    method (e.g. `workspace/executeClientCommand`); others get `null`.
  * The server starts lazily on the first request, after fixture setup, and the harness
    waits for `language/status` ServiceReady.
* Keep the inputs and expected values exactly as upstream has them. If a test can't
  pass yet (it needs an unimplemented subsystem), keep it ported and mark it
  `#[ignore = "reason"]`. Never weaken an assertion.
* Upstream fixture text resources live in `tests/fixtures/testresources` and
  `tests/fixtures/formatter`.
* Run with `cargo test --test <file_stem>`. Set `JDTLS_TEST_STDERR=1 JDTLS_LOG=debug`
  to see server logs.

## Build notes

* `build.rs` runs `mvn -f ecj-bridge/pom.xml package`. Use `-o` (offline) when building
  the bridge by hand. JDT 3.44.0 is in `~/.m2`.
* The bridge daemon is shared over a Unix socket that is versioned by the JAR's hash,
  so a rebuilt JAR always gets a fresh daemon.
* `tests/lsp.rs` is the pre-existing regression suite. Keep it green.
