# jdt.ls parity status

How far jdtls-rust is from eclipse.jdt.ls parity, measured against the upstream test
suite. For how the port is done, see [PORTING.md](PORTING.md).

* **Branch:** `jdtls-parity`, including the verified lifecycle/init/file-event,
  binary-editor, initial correction, completion and project-manager integrations. `main` is unchanged.
* **Reference:** eclipse.jdt.ls 1.58.0. The upstream checkout is 1.58.0-SNAPSHOT
  (2026-04-10), and the oracle in `.oracle/` is the 1.58.0 release.
* **Last updated:** 2026-10-05.

## Summary

The upstream suite has 2,087 `@Test` methods in 206 classes
(`org.eclipse.jdt.ls.tests` and `org.eclipse.jdt.ls.tests.syntaxserver`).

| | Tests | Share of upstream |
|---|---:|---:|
| Ported | 740 | 35.5% |
| Passing | 693 | 33.2% |
| Ported but `#[ignore]`d | 47 | 2.3% |
| Not ported yet | 1,347 | 64.5% |

On `jdtls-parity`, `cargo test --no-fail-fast --bins --tests` gives 1,114 passed,
0 failed and 48 ignored across 72 test targets. That count also includes our own regression suite
(`tests/lsp.rs`, 95 tests; `tests/lifecycle_regressions.rs`, 1 test;
`tests/binary_editor_regressions.rs`, 5 tests;
`tests/correction_regressions.rs`, 3 tests;
`tests/completion_regressions.rs`, 6 tests;
`tests/project_download_regressions.rs`, 2 tests;
`tests/paste_regressions.rs`, 9 tests;
`tests/smart_detection_regressions.rs`, 13 tests;
`tests/accessor_regressions.rs`, 18 tests;
`tests/constructor_regressions.rs`, 24 tests;
`tests/tostring_regressions.rs`, 28 tests;
`tests/hashcode_regressions.rs`, 30 tests;
`tests/delegate_regressions.rs`, 40 tests) and unit
tests that aren't ports. Five project-manager targets also compile the project
module's 11 unit tests, and BasicFileDetector recompiles its detector unit test;
those duplicate runs are excluded from the upstream-port counts.

## By upstream area

| Area (`core.internal.*`) | Upstream | Ported | Passing | Passing % |
|---|---:|---:|---:|---:|
| handlers | 871 | 561 | 542 | 62% |
| javadoc | 32 | 32 | 32 | 100% |
| commands | 60 | 7 | 7 | 12% |
| managers | 211 | 132 | 104 | 49% |
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
| handlers/CompletionHandlerTest | `handlers_completion_handler_test` | 57 | 55 | 2 | 55/55 active; real-JDK TimeUnit and Method* proposal counts differ from rtstubs.jar |
| handlers/DocumentHighlightHandlerTest | `handlers_document_highlight_handler_test` | 5 | 5 | 0 | pass |
| handlers/DocumentLifeCycleHandlerTest | `handlers_document_life_cycle_handler_test` | 19 | 17 | 2 | 17/17 active cases |
| handlers/DocumentSymbolHandlerTest | `handlers_document_symbol_handler_test` | 14 | 13 | 1 | 13/13 active |
| handlers/FileEventHandlerTest | `handlers_file_event_handler_test` | 8 | 8 | 0 | 8/8 |
| handlers/FoldingRangeHandlerTest | `handlers_folding_range_handler_test` | 9 | 9 | 0 | 9/9 |
| handlers/FormatterHandlerTest | `handlers_formatter_handler_test` | 34 | 34 | 0 | 34/34 |
| handlers/GenerateAccessorsActionTest | `handlers_generate_accessors_action_test` | 11 | 11 | 0 | 11/11 |
| handlers/GenerateAccessorsHandlerTest | `handlers_generate_accessors_handler_test` | 7 | 7 | 0 | 7/7 |
| handlers/GenerateConstructorsActionTest | `handlers_generate_constructors_action_test` | 7 | 7 | 0 | 7/7 |
| handlers/GenerateConstructorsHandlerTest | `handlers_generate_constructors_handler_test` | 6 | 6 | 0 | 6/6 |
| handlers/GenerateDelegateMethodsActionTest | `handlers_generate_delegate_methods_action_test` | 3 | 3 | 0 | 3/3 |
| handlers/GenerateDelegateMethodsHandlerTest | `handlers_generate_delegate_methods_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/GenerateToStringActionTest | `handlers_generate_to_string_action_test` | 7 | 7 | 0 | 7/7 |
| handlers/GenerateToStringHandlerTest | `handlers_generate_to_string_handler_test` | 8 | 8 | 0 | 8/8 |
| handlers/HashCodeEqualsActionTest | `handlers_hash_code_equals_action_test` | 6 | 6 | 0 | 6/6 |
| handlers/HashCodeEqualsHandlerTest | `handlers_hash_code_equals_handler_test` | 10 | 10 | 0 | 10/10 |
| handlers/HoverHandlerTest | `handlers_hover_handler_test` | 36 | 34 | 2 | pass (the 2 ignored also fail on jdt.ls) |
| handlers/ImplementationsHandlerTest | `handlers_implementations_handler_test` | 13 | 12 | 1 | pass |
| handlers/InitHandlerTest | `handlers_init_handler_test`, plus unit tests in `server.rs` and `preferences.rs` | 14 | 14 | 0 | 12/12 LSP cases; 2 unit cases |
| handlers/InlayHintHandlerTest | `handlers_inlay_hint_handler_test` | 46 | 46 | 0 | 43/46; 3 differ because the real JDK has sources |
| handlers/InlayHintFilterManagerTest | unit tests in `src/features/inlay_hint_filter.rs` | 7 | 7 | 0 | n/a (unit tests) |
| handlers/NavigateToDeclarationHandlerTest | `handlers_navigate_to_declaration_handler_test` | 5 | 5 | 0 | pass |
| handlers/NavigateToDefinitionHandlerTest | `handlers_navigate_to_definition_handler_test` | 11 | 8 | 3 | pass, except rtstubs/Kotlin |
| handlers/NavigateToTypeDefinitionHandlerTest | `handlers_navigate_to_type_definition_handler_test` | 7 | 6 | 1 | pass, except rtstubs |
| handlers/PasteEventHandlerTest | `handlers_paste_event_handler_test` | 22 | 22 | 0 | 22/22 |
| handlers/PrepareRenameHandlerTest | `handlers_prepare_rename_handler_test` | 15 | 15 | 0 | 15/15 |
| handlers/ReferencesHandlerTest | `handlers_references_handler_test` | 7 | 6 | 1 | pass |
| handlers/RenameHandlerTest | `handlers_rename_handler_test` | 22 | 22 | 0 | 20/22; jdt.ls NPEs on JDK 25 (record field) and has no Lombok jar |
| handlers/SelectionRangeHandlerTest | `handlers_selection_range_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/SemanticTokensHandlerTest | `handlers_semantic_tokens_handler_test` | 11 | 11 | 0 | 11/11 |
| handlers/SignatureHelpHandlerTest | `handlers_signature_help_handler_test` | 56 | 55 | 1 | 54/55; `test_signature_help_erasure_type`, where jdt.ls returns no doc |
| handlers/SmartDetectionHandlerTest | `handlers_smart_detection_handler_test` | 2 | 2 | 0 | 2/2 |
| handlers/TypeHierarchyHandlerTest | `handlers_type_hierarchy_handler_test` | 4 | 4 | 0 | 4/4 |
| handlers/WorkspaceDiagnosticsHandlerTest | `handlers_workspace_diagnostics_handler_test` | 2 | 2 | 0 | 2/2 (package deletion and diagnostic filtering) |
| handlers/WorkspaceExecuteCommandHandlerTest | `handlers_workspace_execute_command_handler_test` | 1 | 1 | 0 | 1/1 (unknown-command error) |
| handlers/WorkspaceSymbolHandlerTest | `handlers_workspace_symbol_handler_test` | 19 | 16 | 3 | 16/16 |
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
| managers/BasicFileDetectorTest | `managers_basic_file_detector_test` | 12 | 12 | 0 | n/a (unit ports) |
| managers/EclipseBuildSupportTest | `managers_eclipse_build_support_test` | 1 | 1 | 0 | 1/1 |
| managers/EclipseProjectImporterTest | `managers_eclipse_project_importer_test` | 15 | 11 | 4 | 8/8 active LSP; 3 unit ports |
| managers/InvisibleProjectBuildSupportTest | `managers_invisible_project_build_support_test` | 4 | 4 | 0 | 2/2 active LSP; 2 preference unit ports |
| managers/InvisibleProjectImporterTest | `managers_invisible_project_importer_test` | 27 | 26 | 1 | active cases pass; helper assertions use the Rust port |
| managers/InvisibleProjectPreferenceChangeListenerTest | `managers_invisible_project_preference_change_listener_test` | 6 | 6 | 0 | 6/6 |
| managers/MavenProjectImporterTest | `managers_maven_project_importer_test` | 32 | 31 | 1 | 29/29 active LSP; 2 unit ports |
| managers/MultiRootTest | `managers_multi_root_test` | 2 | 2 | 0 | 2/2 |
| managers/ProjectsManagerTest | `managers_projects_manager_test` | 11 | 7 | 4 | 7/7 active; Gradle and internal initialization cases ignored |
| managers/StandardProjectManagerTest | `managers_standard_project_manager_test` | 1 | 1 | 0 | n/a (unit port) |

## Ignored tests

Every ignore names its reason in the test file (`#[ignore = "..."]`), and every
ignored test keeps its upstream assertions unchanged.

| Reason | Count | Tests |
|---|---:|---|
| Upstream's fake test JDK (`rtstubs.jar`, no sources); we run a real JDK with `lib/src.zip` | 22 | `test_get_code_lens_symbols_for_class`, `outgoing_calls_src`; 9 ContentProviderManagerTest tests; `test_disassembled_source` and `test_source_version` (definition and type definition); `test_implementation_from_binary_type_with_class_content_support`; `test_references_in_jre`; `test_workspace_search`, `test_camel_case_fuzzy_search` and `test_workspace_search_with_class_content_support`; `test_hover_javadoc_link_plain`; completion `test_completion_import_static` and `test_snippet_interface_method` |
| Upstream test-plugin internals with no LSP equivalent (FakeContentProvider, null URIs, decompiler line mappings) | 9 | ContentProviderManagerTest |
| Lombok not supported | 1 | `test_lombok_show_generated_code_symbols` |
| Kotlin not supported | 1 | `test_kotlin` |
| Direct completion-requestor state access | 1 | `test_signature_help_for_selected_completion_proposal` selects the first raw proposal directly, whose ordering differs from LSP items; the public selection flow is implemented and oracle verified separately |
| The upstream test assumes a Java 10 JDK | 1 | `test_hover_on_java10var` |
| Needs code-action/quick-fix parity | 2 | lifecycle `test_unimplemented_methods` and `test_remove_dead_code_after_if` |
| Requires an installed JavaSE-1.8 or Java 26 VM | 4 | Eclipse `test_forbidden_reference`, `test_preview_features_disabled_by_default`; invisible `test_preview_features_enabled_by_default`; Maven `test_java26_project` |
| Oracle product lacks the resource-filter matcher available in the upstream test plugin | 1 | Eclipse `ignore_missing_resource_filters` |
| Internal project markers differ from published diagnostics | 1 | Eclipse `test_null_analysis` retains the upstream count of 2 markers |
| Direct empty-root initialization has no equivalent public LSP call | 1 | ProjectsManager `test_create_default_project` |
| Gradle model/update parity and a compatible Gradle VM | 3 | ProjectsManager `test_sending_ok_project_status`, `test_sending_warning_project_status`, `test_reload_gradle_project_marker` |

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

## Completion integration evidence

The completion branch is integrated. JDT's `CompletionEngine` returns proposals,
contexts and binding data; Rust computes descriptions, replacements, argument
placeholders, imports, snippets, documentation and completion resolution. A raw
JSON service preserves LSP 3.17 item defaults and insert/replace edits, with the
existing syntax path available while the bridge starts. Override bodies are
computed in Rust from method/type bindings, including interface default methods.

* `handlers_completion_handler_test`: 55 passed, 2 ignored on both Rust and the
  1.58 oracle (`--test-threads=4`). Inputs and assertions come from upstream.
  The fixture explicitly installs upstream's default type-comment template and
  sets `java.lsp.joinOnCompletion` for the two tests that set that JVM property.
* `completion_regressions`: 6 passed on Rust and the oracle: empty/custom comments,
  CRLF snippets, expired completion cache errors, selected-overload signature help
  and client hints, and class/interface override bodies in nonexistent files.
* The saved branch's five empty cache/disabled-record placeholders were removed;
  they are not ports and remain in the unported count. Its scratch probe was removed.
* `java.completion.onDidSelect` records method/constructor selections and requests
  client hints when enabled. Signature help uses the selected signature before
  guessing and clears an unmatched selection. Contribution/ranking providers and
  completion timing/common-data parity still need work.

This adds 57 upstream ports (55 active passes); the 6 regression cases and 8 new
unit tests are excluded from the upstream count. Completion is still incomplete:
42 CompletionHandlerTest cases, plus the dedicated lazy-resolve, chain and postfix
classes, remain unported. The old postfix helper remains available during bridge
startup; the JDT completion path does not yet include that provider.

## Project-manager integration evidence

The saved project-import branch is integrated with 111 substantive upstream ports:
101 passing and 10 ignored. The active LSP cases run against both Rust and jdt.ls
1.58.0; detector, naming, duplicate-import and preference helpers are unit ports.
Maven downloads also activate two existing handler ports without changing their
assertions: JUnit signature help and Reactor's exact 119 workspace-symbol matches.

* The Rust model retains raw Eclipse classpaths, natures, container children,
  outputs, source exclusions, attachments and project/build-file markers. Explicit
  Eclipse library source attachments remain authoritative, including their absence.
* Maven imports discover modules from all profiles, resolve parent models and
  transitive dependencies, download missing artifacts and sources, and retain import
  timestamps across restarts. Import progress is sent after initialization, when
  tower-lsp permits custom notifications.
* Invisible projects infer source roots from trigger files and later opens, follow
  referenced-library and preference changes, and refresh diagnostics and watchers.
  Workspace-folder/configuration changes preserve the verified document lifecycle.
* ECJ returns generated annotation-processor source data; Rust writes it into the
  project's generated-source folder. The AutoValue fixture and existing annotation
  processing regression both pass.
* Project/build-file marker reporting shares the existing saved-file build pipeline.
  Project configuration notifications are ordered before later requests. Local HTTP
  regressions cover artifact fallback, cache reuse, complete concurrent writes and
  preservation of origin records.
* Six empty placeholders and five incomplete test bodies are excluded, rather than
  counted as ignored ports. Unported cases include classpath-job scheduling/merging,
  explicit invalid-project cleanup and unmanaged-jar SHA-1 source discovery/hover.
* Gradle still reads build scripts heuristically; Tooling API/Buildship model parity
  is unfinished. Passing project-selection tests do not establish Gradle parity.

## Known differences from jdt.ls

* **JDT build.** The bridge now uses Maven JDT/ECJ 3.46.0. Its Core build is
  `v20260520-1003`; the 1.58 oracle bundles `v20260409-1507`. Snippet-directive
  whitespace differs between them; Rust restores whitespace from the DOM source
  range, retaining the upstream hover assertion (both snippet cases oracle pass).
  Other build differences may surface as more cases are ported.
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
* **Downloads:** Maven artifact and source downloads are implemented. Custom repository
  and mirror handling, complete Maven model parity and unmanaged-jar source discovery
  remain unfinished.
* **Accessor template dependencies:** locale-sensitive date/time/user resolvers,
  global template preference fallback and occurrence-qualified local/anonymous
  `${enclosing_type}` names still need parity work. Project templates and ordinary
  named nested types are covered.
* **Constructor generation dependencies:** inherited and external-package scope
  conflicts, inherited nullness annotations and annotated array dimensions still
  need the complete ScopeAnalyzer/StubUtility2Core dependency ports. Constructor
  comments share the template resolver/global preference limitations above.
* **toString dependencies:** complete cross-unit source-range ordering, external
  scope/import conflicts and global/date/time/user template resolvers remain
  unfinished. Standalone ASTParser still requires the running VM system library;
  supplied boot-class jars use the split-package workaround, so exact custom-JDK
  binding contents are not guaranteed. These limits are not counted as parity.
* **hashCode/equals dependencies:** inherited and external-package scope/import
  conflicts, inherited nullness and type-use annotation rendering still require
  the full ScopeAnalyzer/import-rewrite dependency ports. Comments share the
  global/date/time/user template limitations above.
* **Delegate generation dependencies:** parameter, nullness and type-use annotation
  rendering, inherited/external import-scope conflicts and global/date/time/user
  template resolvers still require the shared StubUtility2Core and ScopeAnalyzer
  dependency ports. Available source ranges and parameter names now cover current
  and referenced source units and binary source attachments.
* **Lombok** is not supported in any feature.

## Saved work branches

The project-import WIP branch `worktree-agent-a1e31779b8b46b008` (`af6a7a8`)
is now integrated and verified. No saved WIP branch remains unmerged.

## Largest remaining work

| Work | Upstream tests | Share of suite |
|---|---:|---:|
| Remaining quick fixes and assists (`correction`) | 596 | 29% |
| Remaining completion (CompletionHandlerTest 42, LazyResolve 20, Chain 12, Postfix 29) | 103 | 5% |
| Remaining project managers | 79 | 4% |
| Refactoring | 119 | 6% |
| Remaining handlers outside completion: code actions, generation, imports, save actions, markers and lifecycle/init | 207 | 10% |
| Core utilities, preferences, commands and the rest | 243 | 12% |

## Updating this file

Ported and ignored counts come from the test files:

```sh
for f in tests/*.rs; do b=$(basename "$f" .rs); case "$b" in lsp|lifecycle_regressions|binary_editor_regressions|correction_regressions|completion_regressions|project_download_regressions|paste_regressions|smart_detection_regressions|accessor_regressions|constructor_regressions|tostring_regressions|hashcode_regressions|delegate_regressions) continue ;; esac
  echo "$b $(grep -c '#\[test\]' "$f") $(grep -c '#\[ignore' "$f")"; done
```

Add the ports that live as unit tests in `src/` (InlayHintFilterManagerTest 7,
JavaDoc2Markdown 19, JavaDoc2PlainText 2, JavaDocImageExtraction 1, InitHandler 2).
Exclude `lifecycle_regressions.rs`, `binary_editor_regressions.rs` and
`correction_regressions.rs`, `completion_regressions.rs` and
`project_download_regressions.rs`, `paste_regressions.rs` and
`smart_detection_regressions.rs`, `accessor_regressions.rs`,
`constructor_regressions.rs`, `tostring_regressions.rs`, `hashcode_regressions.rs`, `delegate_regressions.rs`, which are our regression suites, and empty placeholders (these are not ports). Upstream counts
come from `grep -c '@Test'` over `eclipse.jdt.ls/org.eclipse.jdt.ls.tests*/src`.
Update this file whenever a branch is merged into `jdtls-parity`.

## Paste integration evidence

All 22 `PasteEventHandlerTest` methods are ported with their original source,
selections, copied text and assertions, and pass against Rust and jdt.ls 1.58.0.
The fixture uses upstream's verbatim `fakejdk/21/rtstubs.jar`, preserving its
import-search candidates. The LSP helper JSON-encodes the paste model like the
editor, opens the working copy and finishes workspace jobs before the request.

* `java.edit.handlePasteEvent` escapes quotes, backslashes and control characters
  inside string literals, keeps Unicode text and literal escape sequences, and
  splits actual newlines with the source's EOL and requested indentation.
  Text blocks, comments and selections touching a literal's boundaries are excluded.
* Missing-import analysis uses an isolated pasted-source snapshot. Rust collects
  references from JDT's data-only DOM, searches project sources and libraries,
  checks visibility/type kinds and resolves ambiguity using existing or copied
  imports. The Rust ImportRewrite retains existing imports, applies configured
  groups/thresholds and returns the original copied text with its workspace edit.
* Static favorites use raw ECJ completion proposals from the upstream invoker's
  dummy compilation unit. The preference timing matches the oracle: organize
  imports sees the previous manager value when configuration replaces preferences.
* `java.project.resolveText` suggests a filename from the first class/interface,
  matches source package fragments, extends the longest package prefix and skips
  existing filenames. It does not create files. Enums, annotations and records
  preserve the upstream `Untitled` filename behavior.
* `paste_regressions`: 9 Rust passes; 8 oracle-compatible passes. These cover
  UTF-16 positions, selection boundaries, import preservation, static favorites,
  file destinations/collisions and unchanged buffers. The remaining Rust-only
  regression covers untitled, in-memory and nonexistent-file documents, including
  untitled buffers without a `.java` filename.

This adds 22 passing upstream ports; the nine regressions are excluded from that
count. Dedicated organize-import tests, module-import selection, complete static
favorite ordering and class-file copied-import coverage still require further
parity verification. The wider parity goal remains incomplete.

## Smart-semicolon integration evidence

Both `SmartDetectionHandlerTest` methods are ported with the original Java 21
fake-JDK setup, source, caret and assertions. They pass on Rust and JDT LS 1.58.0.

* `java.edit.smartSemicolonDetection` now returns the upstream caret destination
  when `java.edit.smartSemicolonDetection.enabled` is true (default false).
  Requests accept raw and JSON-encoded models and leave the working copy untouched.
* Rust implements the full handler and its fresh JFace Java partition scan:
  comments, characters, strings, escapes, source-level text blocks, open partition
  boundaries, whitespace, existing semicolons and block/array-initializer rules.
  The first-`for` heuristic and unsupported Markdown-partition quirk are retained.
* Lifecycle events and diagnostic validation track CoreASTProvider's active Java
  element. The Rust NodeFinder applies the upstream comment/literal and unfinished
  method-call guards to data-only JDT ASTs with `WAIT_ACTIVE_ONLY` behavior.
* `smart_detection_regressions`: 13 Rust passes; 12 oracle-compatible passes.
  These cover active-document changes, source versus compliance, UTF-16,
  CR/LF/CRLF, EOF, Unicode whitespace/identifier parts, invalid and out-of-line
  positions, preference updates, partition boundaries and unchanged buffers.
  The additional Rust-only case covers opened untitled, in-memory and nonexistent
  file buffers, plus raw model arguments.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 57 targets (904 passing, 48 ignored). The two upstream ports and twelve
file-backed regressions also pass with `JDTLS_ORACLE=1 --test-threads=2`; the
virtual-buffer/raw-model case is excluded from that oracle run.

## Accessor integration evidence

All seven `GenerateAccessorsHandlerTest` methods and eleven
`GenerateAccessorsActionTest` methods retain their upstream sources, selections,
capabilities, insertion preferences and assertions. All eighteen pass on Rust
and JDT LS 1.58.0. The handler fixture installs AbstractSourceTestCase's exact
comment/body templates through the project template store and uses the original
Java 21 fake JDK; the three generation tests compare the complete class source.

* Rust owns `java/resolveUnimplementedAccessors`, `java/generateAccessors`, source
  actions and selection-based quick assists. The approximate Java accessor action
  generator is removed. ASTRewrite and edit conversion run in Rust; JDT supplies
  DOM data and formatter edits.
* Discovery preserves static/final fields, primitive-boolean names, record
  components, declared accessor detection, generic parameter distinctions and
  signature-qualified generic arguments. Generation applies field/argument
  affixes, `this`/`is` preferences, project comment/body templates and cursor
  insertion rules. Prompt commands retain ordinal kinds and client capability
  fallback; eager and deferred actions return `WorkspaceEdit.changes`.
* `accessor_regressions`: eighteen Rust passes and seventeen oracle-compatible
  passes. Coverage includes nested/local/anonymous selection, enum/annotation and
  record fields, UTF-16/CRLF, opened buffer overrides, Unicode case rules, kind
  filtering, default line versus problem quick-fix settings, deferred resolution,
  custom templates, Markdown compliance, empty
  bodies, dollar escapes, variable aliases and malformed/empty requests. The
  additional Rust-only case exercises an untitled document.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 60 targets (940 passing, 48 ignored). The eighteen upstream ports and
seventeen file-backed regressions pass with `JDTLS_ORACLE=1 --test-threads=2`;
the virtual-document case is excluded. The existing getter/setter LSP regression
retains both original assertions and adds an exact named quick-assist assertion.
Full feature parity remains incomplete.

## Constructor integration evidence

All six `GenerateConstructorsHandlerTest` methods and seven
`GenerateConstructorsActionTest` methods preserve the upstream sources,
selections, capabilities, signatures, field assertions and complete expected
compilation units. All thirteen pass on Rust and JDT LS 1.58.0. Handler fixtures
use the original Java 21 fake JDK and AbstractSourceTestCase's formatter settings.
The handler's null-cursor generation overload is exercised through the public
request with a zero-length selection at EOF, preserving its append behavior.

* Rust owns `java/checkConstructorsStatus`, `java/generateConstructors`, visible
  superclass filtering, field discovery, constructor AST generation, import and
  text edits, source actions and quick assists. The approximate Java all-fields
  constructor action generator is removed. The bridge exports constructor
  bindings and declaration parameter names; Rust builds each operation.
* Discovery preserves source order, initialized-final/static exclusions, selected
  fragments and package/protected/private visibility. Generation supports
  superclass type substitution, constructor type parameters and bounds, throws,
  varargs, field order, argument affixes and collision suffixes, nested/local
  types, enum visibility and cursor insertion. Client capabilities determine
  prompt, command fallback, eager edit or deferred resolution behavior; edits
  retain `WorkspaceEdit.changes`.
* `constructor_regressions`: twenty-four Rust passes and twenty-three
  oracle-compatible passes, including precise binding DTOs, import conflicts,
  UTF-16/CRLF and buffer overrides, templates and repeated comment tags,
  deprecation tags, Markdown compliance and empty/unknown signature requests.
  The additional Rust-only case covers untitled, in-memory and nonexistent
  file documents.
* Reference quirks are retained: existing constructors do not suppress discovery;
  private-only superclasses fall back to Object; no-argument delegation omits
  superclass generic/throws metadata; record components are omitted from the
  field list and the 1.58.0 manipulation library emits an ordinary constructor
  declaration for records. Later JDT's compact-constructor behavior is not the
  reference behavior (confirmed against the oracle's bytecode).

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 63 targets (977 passing, 48 ignored). The thirteen upstream ports and
twenty-three file-backed regressions pass with `JDTLS_ORACLE=1 --test-threads=2`;
the virtual-document case is excluded. Logs are in
`target/parity-evidence/constructors-full-suite-final.log` and
`target/parity-evidence/constructors-oracle-final.log` (gitignored).
Full feature parity remains incomplete; constructor dependency gaps are listed
above instead of being counted as upstream tests passed.

## toString integration evidence

All eight `GenerateToStringHandlerTest` and seven `GenerateToStringActionTest`
methods retain the upstream source fixtures, settings, selections, discovery
assertions and expected edits. All fifteen pass on Rust and JDT LS 1.58.0. The
handler fixtures retain the Java 21 fake JDK and upstream formatter options; the
null-cursor overload uses the public request with a zero-length EOF selection.

* Rust owns `java/checkToStringStatus`, `java/generateToString`, member discovery
  and binding-key conversion, templates, all four exposed output styles, import
  and text edits, prompt actions, quick assists and eager/deferred direct actions.
  The approximate Java concatenation action is removed. The bridge supplies JDT
  hierarchy/member bindings, source name ranges and formatting data.
* Generation covers concatenation, ordinary/chained StringBuilder, String.format,
  null skipping, array contents and limits, list slicing, the collection/map
  iterator helper, naming collisions, method replacement and cursor insertion.
  Project override-comment templates and the Java 23 template choice are covered.
  Edits retain `WorkspaceEdit.changes` and leave disk contents untouched.
* Twenty-eight additional Rust regressions cover styles, templates, binding-key
  selection/order/deduplication, transient and shadowed fields, method DTOs,
  records, overload/helper preservation, local names, comments and client
  capabilities. Twenty-seven also pass on the oracle; the remaining Rust-only
  case verifies untitled, in-memory and nonexistent file buffers.
* Reference quirks are retained: discovery checks inherited field shadowing only
  against unselected declared fields; generation deduplicates inherited names;
  request order and descriptive DTO values do not override binding keys;
  String.format ignores null skipping; multidimensional arrays use toString
  rather than deepToString; direct actions use only declared ordinary fields
  when deciding whether prompting is needed; the 1.58.0 override-comment path
  always uses the ordinary template, even when Java 23 Markdown is enabled
  (confirmed against the bundled StubUtility bytecode).

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 66 targets (1,020 passing, 48 ignored). The fifteen upstream ports and
twenty-seven file-backed regressions pass with `JDTLS_ORACLE=1 --test-threads=2`.
Logs are in `target/parity-evidence/tostring-full-suite-final.log` and
`target/parity-evidence/tostring-oracle-final.log` (gitignored). Full feature
parity remains incomplete; dependency gaps remain listed above.

## hashCode/equals integration evidence

All ten `HashCodeEqualsHandlerTest` and six `HashCodeEqualsActionTest` methods
retain their original source fixtures, preference choices, selections, discovery
assertions and full edited compilation-unit comparisons. All sixteen pass against
Rust and JDT LS 1.58.0. Handler tests preserve the Java 21 fake JDK and upstream
formatter settings, including the explicit last-member insertion default.

* Rust owns `java/checkHashCodeEqualsStatus`, `java/generateHashCodeEquals`,
  declared-field discovery, existing-method signatures, binding-key selection,
  generation, imports, text edits and prompt source/quick-assist actions. The
  approximate Java generator and its unused signature helper are removed.
* Generation supports all eight primitives, canonical float/double bit conversions,
  one reused double temporary, enum identity, null-safe reference comparisons,
  typed/deep array hashing and comparison, Objects hashing/equality, instanceof
  and block settings, concrete/abstract and binary superclass behavior, nested
  enclosing-instance helpers, regeneration, overload preservation and cursor
  insertion. Shared import scope now includes type parameters.
* Thirty additional Rust regressions cover exact DTOs and binding keys, generic
  types, field/local-name collisions, imported-name conflicts, records, member
  and local types, partial regeneration, custom override comments, Java 23 template
  choice, UTF-16/CRLF and open-buffer edits, capabilities and command fallback.
  Twenty-nine pass against the oracle; the remaining Rust-only test covers
  untitled, in-memory and nonexistent file buffers. Disk text is unchanged and
  edits retain `WorkspaceEdit.changes`.
* Reference quirks remain: discovery does not exclude transient fields; generation
  ignores descriptive DTO values and request order; standalone Objects.hash
  explicitly boxes primitive arguments; array terms precede the remaining Objects
  hash arguments; an empty selection returns super.hashCode(); regeneration=false
  inserts duplicate methods; implicit record methods appear in DOM status. The
  quick-assist presence check accepts ordinary one-argument equals overloads,
  while status checks specifically for equals(Object). Override comments keep
  the ordinary template with Java 23 Markdown enabled.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 69 targets (1,066 passing, 48 ignored). The sixteen upstream ports and
twenty-nine file-backed regressions pass with `JDTLS_ORACLE=1 --test-threads=2`.
Logs are in `target/parity-evidence/hashcode-full-suite-final.log` and
`target/parity-evidence/hashcode-oracle-final.log` (gitignored). Full feature
parity remains incomplete; dependency gaps remain listed above.


## Delegate-method integration evidence

All five `GenerateDelegateMethodsHandlerTest` and three
`GenerateDelegateMethodsActionTest` methods retain their upstream fixtures,
settings, selections, discovery assertions and full edited-source expectations.
All eight pass on Rust and JDT LS 1.58.0. Handler fixtures include the Java 21
fake JDK, upstream formatter settings and the malformed `int B[] array` field.
A zero-width EOF range exercises the public endpoint's append path for the
helper's null-cursor test.

* Rust owns `java/checkDelegateMethodsStatus`, `java/generateDelegateMethods`,
  field/method discovery, visibility and final-method filtering, erased signature
  and exception checks, source/attachment ordering, binding-key selection,
  imports, stubs, comments, cursor insertion and the source prompt action.
  The approximate Java generator is removed. The bridge supplies targeted member
  graphs, source ranges, parameter names and synthetic-record flags as data;
  its source metadata cache is bounded.
* Forty extra regressions cover exact DTOs, primitives/arrays/static fields,
  enums, records, type-variable bounds, interfaces and duplicate signatures,
  covariant-return/exception rules, generic substitution, method type bounds,
  wildcards, varargs, modifiers, imports, field/parameter collisions, source and
  JDK attachment ordering, duplicate selections, stale keys, comment templates,
  argument preferences, header insertion, capability/kind filtering and command
  fallback. Thirty-nine file-backed cases pass against the oracle; one Rust-only
  test checks untitled, in-memory and nonexistent-file buffers. Open-buffer edits
  retain UTF-16/CRLF, preserve disk text and use `WorkspaceEdit.changes`.
* Reference quirks remain: enum final methods suppress equals/hashCode targets,
  while synthetic record methods do not; record components are discovery targets
  but do not enable the prompt; a source prompt can exist with no eligible methods;
  interface candidates include non-public instance methods; methods retain final
  but lose synchronized/abstract/native/default modifiers; invocation keeps a bare
  field even when a parameter shadows it; duplicates generate duplicates; wholly
  stale keys return an internal error. Empty comment tags follow JDT's line-removal
  rules and preserve other blank comment lines.
* The recovered upstream field exposed sentinel extended source ranges that
  overflowed rewrite arithmetic. Rust now falls back to the ordinary node range
  when an extended range is invalid; the unchanged upstream edits pass.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 72 targets (1,114 passing, 48 ignored). The eight upstream ports and
thirty-nine file-backed regressions pass with `JDTLS_ORACLE=1 --test-threads=3`.
The oracle log reports 48 harness passes, including the virtual-buffer test that
returns early; 47 cases actually exercise the oracle. Logs are in
`target/parity-evidence/delegate-full-suite-final-3.log` and
`target/parity-evidence/delegate-oracle-final-4.log` (gitignored). Full feature
parity remains incomplete; the ledger still has 1,347 unported upstream tests.

One earlier full-suite run missed the dependency-scenario lifecycle test's save
marker notification. The unchanged test passed in isolation
(`target/parity-evidence/delegate-lifecycle-recheck.log`); the final full-suite
run above used no overlapping full-suite process. The earlier 39-regression
full run also passed all 72 targets (1,113 passing, 48 ignored).
