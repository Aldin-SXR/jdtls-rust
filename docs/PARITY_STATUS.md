# jdt.ls parity status

How far jdtls-rust is from eclipse.jdt.ls parity, measured against the upstream test
suite. For how the port is done, see [PORTING.md](PORTING.md).

* **Branch:** `jdtls-parity`, including the verified lifecycle/init/file-event,
  binary-editor and initial correction integrations. `main` is unchanged.
* **Reference:** eclipse.jdt.ls 1.58.0. The upstream checkout is 1.58.0-SNAPSHOT
  (2026-04-10), and the oracle in `.oracle/` is the 1.58.0 release.
* **Last updated:** 2026-10-04.

## Summary

The upstream suite has 2,087 `@Test` methods in 206 classes
(`org.eclipse.jdt.ls.tests` and `org.eclipse.jdt.ls.tests.syntaxserver`).

| | Tests | Share of upstream |
|---|---:|---:|
| Ported | 478 | 22.9% |
| Passing | 441 | 21.1% |
| Ported but `#[ignore]`d | 37 | 1.8% |
| Not ported yet | 1,609 | 77.1% |

On `jdtls-parity`, `cargo test --no-fail-fast --bins --tests` gives 626 passed,
0 failed and 38 ignored. That count also includes our own regression suite
(`tests/lsp.rs`, 95 tests; `tests/lifecycle_regressions.rs`, 1 test;
`tests/binary_editor_regressions.rs`, 5 tests;
`tests/correction_regressions.rs`, 3 tests) and unit
tests that aren't ports.

## By upstream area

| Area (`core.internal.*`) | Upstream | Ported | Passing | Passing % |
|---|---:|---:|---:|---:|
| handlers | 871 | 410 | 391 | 45% |
| javadoc | 32 | 32 | 32 | 100% |
| commands | 60 | 7 | 7 | 12% |
| managers | 211 | 21 | 3 | 1% |
| correction | 604 | 8 | 8 | 1% |
| refactoring | 119 | 0 | 0 | 0% |
| (root) | 71 | 0 | 0 | 0% |
| preferences | 53 | 0 | 0 | 0% |
| codemanipulation | 20 | 0 | 0 | 0% |
| cleanup | 18 | 0 | 0 | 0% |
| syntaxserver | 14 | 0 | 0 | 0% |
| contentassist | 6 | 0 | 0 | 0% |
| filesystem, protobuf, javafx, template | 8 | 0 | 0 | 0% |

## Ported classes

"Oracle" means the ported test file was also run against the real jdt.ls 1.58.0
(`JDTLS_ORACLE=1`). Oracle failures are listed with their cause.

| Upstream class | Test file | Ported | Pass | Ignored | Oracle |
|---|---|---:|---:|---:|---|
| handlers/BuildWorkspaceHandlerTest | `handlers_build_workspace_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/CallHierarchyHandlerTest | `handlers_call_hierarchy_handler_test` | 10 | 9 | 1 | 9/9 active; `outgoing_calls_src` resolves into the real JDK's `src.zip` (environment) |
| handlers/CodeActionHandlerTest | `handlers_code_action_handler_test` | 11 | 11 | 0 | 11/11 |
| handlers/CodeLensHandlerTest | `handlers_code_lens_handler_test` | 14 | 13 | 1 | 13/13 active; Runnable exposes 3 lenses with the real JDK's sources (environment) |
| handlers/DocumentHighlightHandlerTest | `handlers_document_highlight_handler_test` | 5 | 5 | 0 | pass |
| handlers/DocumentLifeCycleHandlerTest | `handlers_document_life_cycle_handler_test` | 19 | 17 | 2 | 17/17 active cases |
| handlers/DocumentSymbolHandlerTest | `handlers_document_symbol_handler_test` | 14 | 13 | 1 | 13/13 active |
| handlers/FileEventHandlerTest | `handlers_file_event_handler_test` | 8 | 8 | 0 | 8/8 |
| handlers/FoldingRangeHandlerTest | `handlers_folding_range_handler_test` | 9 | 9 | 0 | 9/9 |
| handlers/FormatterHandlerTest | `handlers_formatter_handler_test` | 34 | 34 | 0 | 34/34 |
| handlers/HoverHandlerTest | `handlers_hover_handler_test` | 36 | 34 | 2 | pass (the 2 ignored also fail on jdt.ls) |
| handlers/ImplementationsHandlerTest | `handlers_implementations_handler_test` | 13 | 12 | 1 | pass |
| handlers/InitHandlerTest | `handlers_init_handler_test`, plus unit tests in `server.rs` and `preferences.rs` | 14 | 14 | 0 | 12/12 LSP cases; 2 unit cases |
| handlers/InlayHintHandlerTest | `handlers_inlay_hint_handler_test` | 46 | 46 | 0 | 43/46; 3 differ because the real JDK has sources |
| handlers/InlayHintFilterManagerTest | unit tests in `src/features/inlay_hint_filter.rs` | 7 | 7 | 0 | n/a (unit tests) |
| handlers/NavigateToDeclarationHandlerTest | `handlers_navigate_to_declaration_handler_test` | 5 | 5 | 0 | pass |
| handlers/NavigateToDefinitionHandlerTest | `handlers_navigate_to_definition_handler_test` | 11 | 8 | 3 | pass, except rtstubs/Kotlin |
| handlers/NavigateToTypeDefinitionHandlerTest | `handlers_navigate_to_type_definition_handler_test` | 7 | 6 | 1 | pass, except rtstubs |
| handlers/PrepareRenameHandlerTest | `handlers_prepare_rename_handler_test` | 15 | 15 | 0 | 15/15 |
| handlers/ReferencesHandlerTest | `handlers_references_handler_test` | 7 | 6 | 1 | pass |
| handlers/RenameHandlerTest | `handlers_rename_handler_test` | 22 | 22 | 0 | 20/22; jdt.ls NPEs on JDK 25 (record field) and has no Lombok jar |
| handlers/SelectionRangeHandlerTest | `handlers_selection_range_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/SemanticTokensHandlerTest | `handlers_semantic_tokens_handler_test` | 11 | 11 | 0 | 11/11 |
| handlers/SignatureHelpHandlerTest | `handlers_signature_help_handler_test` | 56 | 54 | 2 | 53/54; `test_signature_help_erasure_type`, where jdt.ls returns no doc |
| handlers/TypeHierarchyHandlerTest | `handlers_type_hierarchy_handler_test` | 4 | 4 | 0 | 4/4 |
| handlers/WorkspaceDiagnosticsHandlerTest | `handlers_workspace_diagnostics_handler_test` | 2 | 2 | 0 | 2/2 (package deletion and diagnostic filtering) |
| handlers/WorkspaceExecuteCommandHandlerTest | `handlers_workspace_execute_command_handler_test` | 1 | 1 | 0 | 1/1 (unknown-command error) |
| handlers/WorkspaceSymbolHandlerTest | `handlers_workspace_symbol_handler_test` | 19 | 15 | 4 | 15/15 |
| correction/SerialVersionQuickFixTest | `correction_serial_version_quick_fix_test` | 5 | 5 | 0 | 5/5 |
| correction/RedundantInterfaceQuickFixTest | `correction_redundant_interface_quick_fix_test` | 2 | 2 | 0 | 2/2 |
| correction/UnnecessaryCastQuickFixTest | `correction_unnecessary_cast_quick_fix_test` | 1 | 1 | 0 | 1/1 |
| commands/DiagnosticsCommandTest | `commands_diagnostics_command_test` | 2 | 2 | 0 | 2/2 |
| commands/TypeHierarchyCommandTest | `commands_type_hierarchy_command_test` | 5 | 5 | 0 | 5/5 |
| javadoc/JavaDoc2MarkdownConverterTest | unit tests in `src/javadoc/converter.rs` | 19 | 19 | 0 | n/a (unit tests) |
| javadoc/JavaDoc2PlainTextConverterTest | unit tests in `src/javadoc/converter.rs` | 2 | 2 | 0 | n/a (unit tests) |
| javadoc/JavaDocImageExtractionTest | `javadoc_java_doc_image_extraction_test`, plus a unit test in `src/javadoc/path_handler.rs` | 6 | 6 | 0 | pass |
| javadoc/JavadocContentTest | `javadoc_javadoc_content_test` | 5 | 5 | 0 | pass |
| managers/ContentProviderManagerTest | `managers_content_provider_manager_test` | 21 | 3 | 18 | pass |

## Ignored tests

Every ignore names its reason in the test file (`#[ignore = "..."]`), and every
ignored test keeps its upstream assertions unchanged.

| Reason | Count | Tests |
|---|---:|---|
| Upstream's fake test JDK (`rtstubs.jar`, no sources); we run a real JDK with `lib/src.zip` | 20 | `test_get_code_lens_symbols_for_class`, `outgoing_calls_src`; 9 ContentProviderManagerTest tests; `test_disassembled_source` and `test_source_version` (definition and type definition); `test_implementation_from_binary_type_with_class_content_support`; `test_references_in_jre`; `test_workspace_search`, `test_camel_case_fuzzy_search` and `test_workspace_search_with_class_content_support`; `test_hover_javadoc_link_plain` |
| Upstream test-plugin internals with no LSP equivalent (FakeContentProvider, null URIs, decompiler line mappings) | 9 | ContentProviderManagerTest |
| Missing local artifacts (no download yet) | 2 | `test_signature_help_assert_equals` (junit 4.13.1); `test_empty_names` (reactor-core 3.3.0) |
| Lombok not supported | 1 | `test_lombok_show_generated_code_symbols` |
| Kotlin not supported | 1 | `test_kotlin` |
| Needs a completion-area feature | 1 | `test_signature_help_for_selected_completion_proposal` (`CompletionHandler.selectedProposal`) |
| The upstream test assumes a Java 10 JDK | 1 | `test_hover_on_java10var` |
| Needs code-action/quick-fix parity | 2 | lifecycle `test_unimplemented_methods` and `test_remove_dead_code_after_if` |

## Lifecycle/init integration evidence

The lifecycle/init/file-event branch is integrated and verified. This adds 51
substantive upstream ports (49 passing, 2 ignored), including the two
`DiagnosticsCommandTest` cases and package deletion/diagnostic filtering from
`WorkspaceDiagnosticsHandlerTest`. All 47 new active LSP cases pass against
jdt.ls 1.58.0; the remaining 2 passing cases are preference unit tests.

* Saved-file builds and working-copy reconciliation use separate source snapshots;
  expected package checks and non-project syntax-only diagnostics follow jdt.ls.
* `java/buildWorkspace`, `java/buildProjects`, diagnostics refresh, dynamic
  capability registration, file watchers and `workspace/willRenameFiles` are wired.
* File, package and compilation-unit move refactoring preserve upstream assertions.
  Folder deletion clears child diagnostics and cached disk sources; open working
  copies survive disk deletion.
* The validation queue retains jdt.ls's behavior: other open buffers are added
  **after** the current queue is snapshotted and validated on the next trigger.
  `lifecycle_regressions.rs` checks this against the oracle.
* Empty placeholders from the WIP branch are not counted as ports. Unported cases
  remain recorded beside their test classes: build cancellation, lifecycle monitor
  and foreign-builder internals, JVM configuration, symbolic-link Gradle import,
  delegate command extensions and bundle events. The two InitHandler preference
  ports are counted once, as unit tests.
* Initialization advertises the upstream command list, but many listed commands
  still need implementations. Capability-shape tests do not prove those commands
  work. Save actions/cleanups and the rest of workspace marker reporting remain
  unfinished.

## Binary-editor integration evidence

Ten previously ignored upstream ports now pass against both Rust and jdt.ls
1.58.0: seven document-symbol cases, binary folding, semantic tokens of an
attached source and outgoing call hierarchy into and within a jar. The complete
five affected handler suites give 55 passing active cases against the oracle.

* Editor handlers obtain binary text through the existing class-file content
  provider. Binary sources stay outside the workspace document store and builds.
* The bridge supplies source-attachment metadata and binding-resolved AST/index
  data using the binary's owning project and source filename. Rust shapes symbols,
  tokens, lenses, selection ranges and hierarchy items.
* Explicit Eclipse/Maven raw library entries retain their sourcepath. Nearby
  source jars are not silently attached to Eclipse `lib` entries without one.
* Attached outlines include implicit default constructors and the Java model's
  package and flat-container conventions. Flat outlines without sources use zero
  ranges; their hierarchical outline uses decompiled text. Semantic tokens and
  selection ranges require an attached source buffer.
* Binary lenses resolve references back into workspace sources, and binary call
  hierarchy works even in a project with no workspace compilation units.
* Five additional oracle-verified regressions assert complete outline output,
  lens ranges and commands, source selection chains, no-source behavior and
  binary-only project contexts (`binary_editor_regressions.rs`).
* Two upstream assertions depend on the source-less fake test runtime: Runnable
  expects 2 lenses rather than the real JDK's 3, and `outgoing_calls_src` expects
  `currentThread` at line 0 rather than its attached source location. Both fail
  identically on Rust and the oracle. They retain their assertions and explicit
  environment ignores; the latter was previously a misleading Rust pass.

This brings passing upstream ports from 413 to 422 (ten newly passing cases,
one corrected environment classification). Binary Java model parity still needs
broader coverage of generated enum/record members and unusual or mismatched
source attachments; this integration does not claim exhaustive class-file parity.

## Correction integration evidence

The saved quick-fix infrastructure is integrated. Nineteen substantive upstream
ports pass against both Rust and jdt.ls 1.58.0: eight correction cases and eleven
`CodeActionHandlerTest` cases. The correction assertions retain exact titles,
generated serial IDs and resulting source; the handler ports retain upstream
fixtures, diagnostic inputs, kind restrictions and edit assertions.

* The bridge exports resolved JDT nodes, bindings, compiler problems and class
  bytes. Rust navigates the AST, computes rewrites and imports, constructs edits,
  orders ported proposals and handles action resolution.
* Ported processors cover serial IDs, redundant interfaces, unnecessary casts,
  unterminated strings and superfluous semicolons. Compiler-ignore edits into an
  external settings file are exercised by the redundant-interface fixture.
* Diagnostic conversion preserves raw problem IDs, arguments, UTF-16 ranges and
  client-supported tags. Lifecycle reconciliation converts against the current
  buffer; builds convert against the saved snapshot.
* Java diagnostic filtering and requested action-kind filtering are applied to
  the compatibility bridge. Import actions use the upstream label and selection
  rules; Rust combines unused-import removals and missing-import candidates in
  one edit. Unused imports can be removed without a diagnostic in the request.
* Non-project fixes consult the current lifecycle diagnostics mode. Deferred
  actions resolve into edits, and a change to the active correction document
  invalidates stored proposals. Three own regression tests verify these flows
  against the oracle, including mixed import additions and removals.
* The older editor harness now advertises the literal-action capability its
  code-action assertions require, and expects the upstream `Organize imports`
  label. Its 95 tests pass. The minimal lms-monaco capability fixture is retained.
* **Remaining:** most correction processors are still unported, including several
  empty scaffolds. The compatibility bridge still supplies unported fixes,
  refactors and generation actions, with incomplete ordering/metadata/resolve
  parity. Import candidate search still uses the old bridge implementation;
  ambiguous imports, sorting, wildcards and static imports need dedicated ports.
  Active AST tracking across other editor handlers, invalid proposal errors,
  snippet edits and full project-setting refresh on compiler-ignore actions also
  remain. The two lifecycle quick-fix tests stay ignored with their assertions;
  unimplemented-method generation is missing even though kind filtering is fixed.

This raises passing upstream ports from 422 to 441. The two correction helper unit
tests and three own regressions are excluded from the upstream-port count.

## Known differences from jdt.ls

* **JDT version.** The bridge uses JDT/ECJ 3.44.0, while jdt.ls 1.58 uses 3.46. One
  visible effect: `{@code}` containing braces ends at the first `}` (for example in
  `Map.computeIfAbsent`). The completion branch evaluates 3.46.
* **`java.signatureHelp.enabled`** defaults to `true`, where jdt.ls defaults to
  `false`. This keeps signature help working for clients that send no settings
  (lms-monaco, `web/`).
* **Attached Javadoc HTML for methods** (anchor computation) isn't ported. Types,
  fields and packages are.
* **Hover edge cases:** `var` at compliance 1.8 returns `[]` instead of `[""]`; packages
  split across JDK modules pick a different module root; a type variable inside a
  varargs parameter.
* **References:** jdt.ls searches every workspace project, but we search only the
  requesting project's closure plus its libraries. Binary subtypes aren't searched
  for implementations.
* **Rename:** overriding methods are linked only through files that mention the name;
  a package rename covers only the selected source folder; the "type already exists"
  check and textual or comment matches aren't implemented.
* **Signature help fallback path** (no AST context) is approximate; no test covers it.
* **Downloads:** missing Maven artifacts and `-sources.jar` files aren't downloaded.
  The project-import branch adds this.
* **Lombok** is not supported in any feature.

## Not merged yet

These branches hold work in progress that was interrupted by API session limits. Each
was saved as a WIP commit and has not been verified in the integrated branch.

| Branch | Area | Ahead of `jdtls-parity` | State |
|---|---|---|---|
| `worktree-agent-a1a367cb061d797a4` (`3a27dfc`) | Completion: JDT `CompletionEngine` running in the bridge without the Java model, plus the Rust conversion layer | 7 commits, about 11k lines | stopped while comparing JDT 3.46 with 3.44; 62 substantive completion tests saved (7 ignored), integration and oracle verification pending |
| `worktree-agent-a1e31779b8b46b008` (`af6a7a8`) | Project import: Eclipse and Maven importers to match m2e and jdt.ls, Maven downloads, a Gradle decision | 4 commits, about 7k lines | stopped while starting MavenProjectImporterTest |

## Largest remaining work

| Work | Upstream tests | Share of suite |
|---|---:|---:|
| Quick fixes and assists (`correction`) | 604 | 29% |
| Completion (CompletionHandlerTest 156, LazyResolve 20, Chain 12, Postfix 29) | 217 | 10% |
| Project managers | 211 | 10% |
| Refactoring | 119 | 6% |
| Remaining handlers: code actions, code generation, organize imports, paste, save actions, workspace markers and other lifecycle/init cases | about 239 | 11% |
| Core utilities, preferences, commands and the rest | about 220 | 11% |

## Updating this file

Ported and ignored counts come from the test files:

```sh
for f in tests/*.rs; do b=$(basename "$f" .rs); case "$b" in lsp|lifecycle_regressions|binary_editor_regressions|correction_regressions) continue ;; esac
  echo "$b $(grep -c '#\[test\]' "$f") $(grep -c '#\[ignore' "$f")"; done
```

Add the ports that live as unit tests in `src/` (InlayHintFilterManagerTest 7,
JavaDoc2Markdown 19, JavaDoc2PlainText 2, JavaDocImageExtraction 1, InitHandler 2).
Exclude `lifecycle_regressions.rs`, `binary_editor_regressions.rs` and
`correction_regressions.rs`, which are our regression suites, and empty placeholders (these are not ports). Upstream counts
come from `grep -c '@Test'` over `eclipse.jdt.ls/org.eclipse.jdt.ls.tests*/src`.
Update this file whenever a branch is merged into `jdtls-parity`.
