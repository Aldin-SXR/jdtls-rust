# jdt.ls parity status

How far jdtls-rust is from eclipse.jdt.ls parity, measured against the upstream test
suite. For how the port is done, see [PORTING.md](PORTING.md).

* **Branch:** `jdtls-parity`, including the verified lifecycle/init/file-event
  integration. `main` is unchanged.
* **Reference:** eclipse.jdt.ls 1.58.0. The upstream checkout is 1.58.0-SNAPSHOT
  (2026-04-10), and the oracle in `.oracle/` is the 1.58.0 release.
* **Last updated:** 2026-10-04.

## Summary

The upstream suite has 2,087 `@Test` methods in 206 classes
(`org.eclipse.jdt.ls.tests` and `org.eclipse.jdt.ls.tests.syntaxserver`).

| | Tests | Share of upstream |
|---|---:|---:|
| Ported | 459 | 22.0% |
| Passing | 413 | 19.8% |
| Ported but `#[ignore]`d | 46 | 2.2% |
| Not ported yet | 1,628 | 78.0% |

On `jdtls-parity`, `cargo test --no-fail-fast --bins --tests` gives 588 passed,
0 failed and 47 ignored. That count also includes our own regression suite
(`tests/lsp.rs`, 95 tests; `tests/lifecycle_regressions.rs`, 1 test) and unit
tests that aren't ports.

## By upstream area

| Area (`core.internal.*`) | Upstream | Ported | Passing | Passing % |
|---|---:|---:|---:|---:|
| handlers | 871 | 399 | 371 | 43% |
| javadoc | 32 | 32 | 32 | 100% |
| commands | 60 | 7 | 7 | 12% |
| managers | 211 | 21 | 3 | 1% |
| correction | 604 | 0 | 0 | 0% |
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
| handlers/CallHierarchyHandlerTest | `handlers_call_hierarchy_handler_test` | 10 | 9 | 1 | 8 pass; `outgoing_calls_src` resolves into the real JDK's `src.zip` (environment) |
| handlers/CodeLensHandlerTest | `handlers_code_lens_handler_test` | 14 | 13 | 1 | 13/13 |
| handlers/DocumentHighlightHandlerTest | `handlers_document_highlight_handler_test` | 5 | 5 | 0 | pass |
| handlers/DocumentLifeCycleHandlerTest | `handlers_document_life_cycle_handler_test` | 19 | 17 | 2 | 17/17 active cases |
| handlers/DocumentSymbolHandlerTest | `handlers_document_symbol_handler_test` | 14 | 6 | 8 | pass |
| handlers/FileEventHandlerTest | `handlers_file_event_handler_test` | 8 | 8 | 0 | 8/8 |
| handlers/FoldingRangeHandlerTest | `handlers_folding_range_handler_test` | 9 | 8 | 1 | 8/9 (the harness had no jdt:// URIs at the time) |
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
| handlers/SemanticTokensHandlerTest | `handlers_semantic_tokens_handler_test` | 11 | 10 | 1 | 10/10 |
| handlers/SignatureHelpHandlerTest | `handlers_signature_help_handler_test` | 56 | 54 | 2 | 53/54; `test_signature_help_erasure_type`, where jdt.ls returns no doc |
| handlers/TypeHierarchyHandlerTest | `handlers_type_hierarchy_handler_test` | 4 | 4 | 0 | 4/4 |
| handlers/WorkspaceDiagnosticsHandlerTest | `handlers_workspace_diagnostics_handler_test` | 2 | 2 | 0 | 2/2 (package deletion and diagnostic filtering) |
| handlers/WorkspaceExecuteCommandHandlerTest | `handlers_workspace_execute_command_handler_test` | 1 | 1 | 0 | 1/1 (unknown-command error) |
| handlers/WorkspaceSymbolHandlerTest | `handlers_workspace_symbol_handler_test` | 19 | 15 | 4 | 15/15 |
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
| Upstream's fake test JDK (`rtstubs.jar`, no sources); we run a real JDK with `lib/src.zip` | 18 | 9 ContentProviderManagerTest tests; `test_disassembled_source` and `test_source_version` (definition and type definition); `test_implementation_from_binary_type_with_class_content_support`; `test_references_in_jre`; `test_workspace_search`, `test_camel_case_fuzzy_search` and `test_workspace_search_with_class_content_support`; `test_hover_javadoc_link_plain` |
| Class-file handlers and fixture setup remain incomplete; all 11 rechecked and still failing (see below) | 11 | `test_folding_ranges`; 7 document-symbol tests (WordUtils, StrTokenizer, `test_package_class`, the no-source jar, `test_decompiled_source`); `test_semantic_tokens_source_attachment`; `test_get_code_lens_symbols_for_class`; `outgoing_jar` |
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

## Class-file ignore audit

Re-ran the 11 tests previously labeled only "needs jdt:// classfile support" with
`--ignored` (excluding the separate Lombok case). None passes yet:

* Seven document-symbol tests and the folding-range test fail in the harness:
  `Workspace::class_uri` only searches source files. The binary-capable
  `class_file_uri` lookup exists but is not used by these ports.
* The semantic-token source-attachment fixture setup is currently an empty
  function, and the harness tries to read its `jdt:` URI as a disk path.
* The binary code-lens request returns no lenses. Binary outgoing call hierarchy
  returns a zero selection range instead of the attached source's line 61.
* `source_text` only handles the document store and `.java` files; document symbols,
  folding, code lenses and semantic-token handlers still need binary source/model
  integration. Navigation/hover class-file support alone does not cover them.

These ignores remain enabled, with their assertions unchanged. This is the next
concrete binary-document gap; removing the ignore annotations alone would not
increase feature parity.

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
was saved as a WIP commit and **hasn't been built or tested**.

| Branch | Area | Ahead of `jdtls-parity` | State |
|---|---|---|---|
| `worktree-agent-a1a367cb061d797a4` (`3a27dfc`) | Completion: JDT `CompletionEngine` running in the bridge without the Java model, plus the Rust conversion layer | 7 commits, about 11k lines | stopped while comparing JDT 3.46 with 3.44; no tests ported yet |
| `worktree-agent-afcd3c34fa679aeb8` (`52a2d64`) | Quick-fix infrastructure: semantic AST, Rust ASTRewrite and ImportRewrite, CodeActionHandler, quick-fix test harness | 3 commits, about 18k lines | stopped while writing its first test files |
| `worktree-agent-a1e31779b8b46b008` (`af6a7a8`) | Project import: Eclipse and Maven importers to match m2e and jdt.ls, Maven downloads, a Gradle decision | 4 commits, about 7k lines | stopped while starting MavenProjectImporterTest |

## Largest remaining work

| Work | Upstream tests | Share of suite |
|---|---:|---:|
| Quick fixes and assists (`correction`) | 604 | 29% |
| Completion (CompletionHandlerTest 156, LazyResolve 20, Chain 12, Postfix 29) | 217 | 10% |
| Project managers | 211 | 10% |
| Refactoring | 119 | 6% |
| Remaining handlers: code actions, code generation, organize imports, paste, save actions, workspace markers and other lifecycle/init cases | about 250 | 12% |
| Core utilities, preferences, commands and the rest | about 220 | 11% |

## Updating this file

Ported and ignored counts come from the test files:

```sh
for f in tests/*.rs; do b=$(basename "$f" .rs); case "$b" in lsp|lifecycle_regressions) continue ;; esac
  echo "$b $(grep -c '#\[test\]' "$f") $(grep -c '#\[ignore' "$f")"; done
```

Add the ports that live as unit tests in `src/` (InlayHintFilterManagerTest 7,
JavaDoc2Markdown 19, JavaDoc2PlainText 2, JavaDocImageExtraction 1, InitHandler 2).
Exclude `lifecycle_regressions.rs`, which is our regression suite, and empty
placeholders (these are not ports). Upstream counts
come from `grep -c '@Test'` over `eclipse.jdt.ls/org.eclipse.jdt.ls.tests*/src`.
Update this file whenever a branch is merged into `jdtls-parity`.
