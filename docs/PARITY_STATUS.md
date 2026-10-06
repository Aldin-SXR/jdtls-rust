# jdt.ls parity status

How far jdtls-rust is from eclipse.jdt.ls parity, measured against the upstream test
suite. For how the port is done, see [PORTING.md](PORTING.md).

* **Branch:** `jdtls-parity`, including the verified lifecycle/init/file-event,
  binary-editor, initial correction, completion and project-manager integrations. `main` is unchanged.
* **Reference:** eclipse.jdt.ls 1.58.0. The upstream checkout is 1.58.0-SNAPSHOT
  (2026-04-10), and the oracle in `.oracle/` is the 1.58.0 release.
* **Last updated:** 2026-10-06.

## Summary

The upstream suite has 2,087 `@Test` methods in 206 classes
(`org.eclipse.jdt.ls.tests` and `org.eclipse.jdt.ls.tests.syntaxserver`).

| | Tests | Share of upstream |
|---|---:|---:|
| Ported | 836 | 40.1% |
| Passing | 809 | 38.8% |
| Ported but `#[ignore]`d | 27 | 1.3% |
| Not ported yet | 1,251 | 59.9% |

On `jdtls-parity`, `cargo test --no-fail-fast --bins --tests` gives 1,531 passed,
0 failed and 28 ignored across 88 test targets. That count also includes our own regression suite
(`tests/lsp.rs`, 95 tests; `tests/lifecycle_regressions.rs`, 2 tests;
`tests/binary_editor_regressions.rs`, 6 tests;
`tests/content_provider_regressions.rs`, 4 tests;
`tests/correction_regressions.rs`, 3 tests;
`tests/completion_regressions.rs`, 6 tests;
`tests/project_download_regressions.rs`, 2 tests;
`tests/paste_regressions.rs`, 9 tests;
`tests/smart_detection_regressions.rs`, 13 tests;
`tests/accessor_regressions.rs`, 18 tests;
`tests/constructor_regressions.rs`, 24 tests;
`tests/tostring_regressions.rs`, 28 tests;
`tests/hashcode_regressions.rs`, 30 tests;
`tests/delegate_regressions.rs`, 40 tests;
`tests/override_regressions.rs`, 33 tests;
`tests/method_correction_regressions.rs`, 29 tests;
`tests/dead_code_regressions.rs`, 26 tests;
`tests/unused_code_regressions.rs`, 32 tests;
`tests/exception_correction_regressions.rs`, 22 tests;
`tests/expression_correction_regressions.rs`, 35 tests;
`tests/type_import_regressions.rs`, 37 tests;
`tests/nullness_generation_regressions.rs`, 39 tests;
`tests/uncaught_exception_regressions.rs`, 18 tests;
`tests/allocation_correction_regressions.rs`, 24 tests) and unit
tests that aren't ports. Five project-manager targets also compile the project
module's 11 unit tests, and BasicFileDetector recompiles its detector unit test;
those duplicate runs are excluded from the upstream-port counts.

## By upstream area

| Area (`core.internal.*`) | Upstream | Ported | Passing | Passing % |
|---|---:|---:|---:|---:|
| handlers | 871 | 562 | 558 | 64% |
| javadoc | 32 | 32 | 32 | 100% |
| commands | 60 | 7 | 7 | 12% |
| managers | 211 | 132 | 109 | 52% |
| correction | 604 | 93 | 93 | 15% |
| refactoring | 119 | 0 | 0 | 0% |
| (root) | 71 | 0 | 0 | 0% |
| preferences | 53 | 0 | 0 | 0% |
| codemanipulation | 20 | 10 | 10 | 50% |
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
| handlers/CallHierarchyHandlerTest | `handlers_call_hierarchy_handler_test` | 10 | 10 | 0 | 10/10; restored stub-JDK source-location assertion verified |
| handlers/CodeActionHandlerTest | `handlers_code_action_handler_test` | 11 | 11 | 0 | 11/11 |
| handlers/CodeLensHandlerTest | `handlers_code_lens_handler_test` | 14 | 14 | 0 | 14/14; restored two-lens binary assertion verified |
| handlers/CompletionHandlerTest | `handlers_completion_handler_test` | 57 | 57 | 0 | 56/57 verified, including both restored stub-JDK tests; existing `test_snippet_ctor` product template mismatch remains |
| handlers/DocumentHighlightHandlerTest | `handlers_document_highlight_handler_test` | 5 | 5 | 0 | pass |
| handlers/DocumentLifeCycleHandlerTest | `handlers_document_life_cycle_handler_test` | 19 | 19 | 0 | 19/19 |
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
| handlers/HoverHandlerTest | `handlers_hover_handler_test` | 36 | 35 | 1 | 35 active ports; restored sourceless link assertion verified; Java 10 VM assumption remains |
| handlers/ImplementationsHandlerTest | `handlers_implementations_handler_test` | 13 | 13 | 0 | 13/13; restored eight source/binary implementation assertion verified |
| handlers/InitHandlerTest | `handlers_init_handler_test`, plus unit tests in `server.rs` and `preferences.rs` | 14 | 14 | 0 | 12/12 LSP cases; 2 unit cases |
| handlers/InlayHintHandlerTest | `handlers_inlay_hint_handler_test` | 46 | 46 | 0 | 43/46; 3 differ because the real JDK has sources |
| handlers/InlayHintFilterManagerTest | unit tests in `src/features/inlay_hint_filter.rs` | 7 | 7 | 0 | n/a (unit tests) |
| handlers/NavigateToDeclarationHandlerTest | `handlers_navigate_to_declaration_handler_test` | 5 | 5 | 0 | pass |
| handlers/NavigateToDefinitionHandlerTest | `handlers_navigate_to_definition_handler_test` | 11 | 10 | 1 | 10 active ports; both restored rtstubs assertions verified; Kotlin remains |
| handlers/NavigateToTypeDefinitionHandlerTest | `handlers_navigate_to_type_definition_handler_test` | 7 | 7 | 0 | 7/7; restored rtstubs assertion verified |
| handlers/OverrideMethodsActionTest | `handlers_override_methods_action_test` | 1 | 1 | 0 | 1/1 |
| handlers/PasteEventHandlerTest | `handlers_paste_event_handler_test` | 22 | 22 | 0 | 22/22 |
| handlers/PrepareRenameHandlerTest | `handlers_prepare_rename_handler_test` | 15 | 15 | 0 | 15/15 |
| handlers/ReferencesHandlerTest | `handlers_references_handler_test` | 7 | 7 | 0 | 7/7; restored System.out binary reference assertion verified |
| handlers/RenameHandlerTest | `handlers_rename_handler_test` | 22 | 22 | 0 | 20/22; jdt.ls NPEs on JDK 25 (record field) and has no Lombok jar |
| handlers/SelectionRangeHandlerTest | `handlers_selection_range_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/SemanticTokensHandlerTest | `handlers_semantic_tokens_handler_test` | 11 | 11 | 0 | 11/11 |
| handlers/SignatureHelpHandlerTest | `handlers_signature_help_handler_test` | 56 | 55 | 1 | 54/55; `test_signature_help_erasure_type`, where jdt.ls returns no doc |
| handlers/SmartDetectionHandlerTest | `handlers_smart_detection_handler_test` | 2 | 2 | 0 | 2/2 |
| handlers/TypeHierarchyHandlerTest | `handlers_type_hierarchy_handler_test` | 4 | 4 | 0 | 4/4 |
| handlers/WorkspaceDiagnosticsHandlerTest | `handlers_workspace_diagnostics_handler_test` | 2 | 2 | 0 | 2/2 (package deletion and diagnostic filtering) |
| handlers/WorkspaceExecuteCommandHandlerTest | `handlers_workspace_execute_command_handler_test` | 1 | 1 | 0 | 1/1 (unknown-command error) |
| handlers/WorkspaceSymbolHandlerTest | `handlers_workspace_symbol_handler_test` | 19 | 19 | 0 | 19/19; all three restored stub-JDK assertions verified |
| correction/AssignToVariableRefactorTest | `correction_assign_to_variable_refactor_test` | 2 | 2 | 0 | 2/2 (advanced assignment commands) |
| correction/AbstractMethodQuickFixTest | `correction_abstract_method_quick_fix_test` | 8 | 8 | 0 | 8/8 |
| correction/LocalCorrectionQuickFixTest | `correction_local_correction_quick_fix_test` | 75 | 75 | 0 | 75/75 with `--test-threads=1`; 12 upstream methods remain unported |
| correction/SerialVersionQuickFixTest | `correction_serial_version_quick_fix_test` | 5 | 5 | 0 | 5/5 |
| correction/RedundantInterfaceQuickFixTest | `correction_redundant_interface_quick_fix_test` | 2 | 2 | 0 | 2/2 |
| correction/UnnecessaryCastQuickFixTest | `correction_unnecessary_cast_quick_fix_test` | 1 | 1 | 0 | 1/1 |
| codemanipulation/OverrideMethodsTestCase | `codemanipulation_override_methods_test_case` | 10 | 10 | 0 | 10/10 |
| commands/DiagnosticsCommandTest | `commands_diagnostics_command_test` | 2 | 2 | 0 | 2/2 |
| commands/TypeHierarchyCommandTest | `commands_type_hierarchy_command_test` | 5 | 5 | 0 | 5/5 |
| javadoc/JavaDoc2MarkdownConverterTest | unit tests in `src/javadoc/converter.rs` | 19 | 19 | 0 | n/a (unit tests) |
| javadoc/JavaDoc2PlainTextConverterTest | unit tests in `src/javadoc/converter.rs` | 2 | 2 | 0 | n/a (unit tests) |
| javadoc/JavaDocImageExtractionTest | `javadoc_java_doc_image_extraction_test`, plus a unit test in `src/javadoc/path_handler.rs` | 6 | 6 | 0 | pass |
| javadoc/JavadocContentTest | `javadoc_javadoc_content_test` | 5 | 5 | 0 | pass |
| managers/ContentProviderManagerTest | `managers_content_provider_manager_test` | 21 | 8 | 13 | 6 active LSP ports and 2 direct null-input ports; Rust provider policy integrated |
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
ignored test remains counted as unfinished. The six provider-chain cases also
need the upstream plugin log assertions, which their current LSP adapters cannot
observe. The separate optional `javadoc::converter::corpus_diff::corpus` unit
test needs an external `JAVADOC_CORPUS`; it is the 28th ignored test in the full
Rust run and is excluded from the upstream-port count.

| Reason | Count | Tests |
|---|---:|---|
| Upstream test-plugin provider injection and log assertions (throwing providers, duplicate providers, placeholder provider) | 6 | ContentProviderManagerTest: `test_throws_exception`, `test_decompile_throws_exception`, `test_default_order`, `test_decompile_default_order`, `test_prefer_non_existing_provider_class`, `test_decompile_prefer_non_existing_provider_class` |
| Upstream test-plugin internals with no LSP equivalent (FakeContentProvider, decompiler line mappings) | 7 | ContentProviderManagerTest |
| Lombok not supported | 1 | `test_lombok_show_generated_code_symbols` |
| Kotlin not supported | 1 | `test_kotlin` |
| Direct completion-requestor state access | 1 | `test_signature_help_for_selected_completion_proposal` selects the first raw proposal directly, whose ordering differs from LSP items; the public selection flow is implemented and oracle verified separately |
| The upstream test assumes a Java 10 JDK | 1 | `test_hover_on_java10var` |
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
  remain. Lifecycle dead-code and unimplemented-method fixes are now enabled
  with their original assertions; see the correction evidence below.

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
  conflicts still need the complete ScopeAnalyzer dependency port. Inherited
  nullness annotations, redundant-nullness filtering, ordinary type-use
  annotations, annotated array dimensions and varargs now use the shared Rust
  import builder and stub policy. Constructor comments share the template
  resolver/global preference limitations above.
* **toString dependencies:** complete cross-unit source-range ordering, external
  scope/import conflicts and global/date/time/user template resolvers remain
  unfinished. Standalone ASTParser still requires the running VM system library;
  supplied boot-class jars use the split-package workaround, so exact custom-JDK
  binding contents are not guaranteed. These limits are not counted as parity.
* **hashCode/equals dependencies:** inherited and external-package scope/import
  conflicts, inherited nullness and type-use annotation rendering still require
  the full ScopeAnalyzer/import-rewrite dependency ports. Comments share the
  global/date/time/user template limitations above.
* **Delegate generation dependencies:** inherited/external import-scope conflicts
  and global/date/time/user template resolvers still require shared dependency
  ports. Inherited parameter nullness annotations and redundant-nullness filtering
  now follow the shared Rust stub policy. Available source ranges and parameter
  names now cover current and referenced source units and binary source attachments. Ordinary type-use
  annotations, annotated dimensions and varargs use the shared AST import builder.
* **Override/implementation dependencies:** complete inherited/external scope
  conflicts and global date/time/user body-template resolvers still need shared
  dependency ports. Inherited nullness annotations, source modifier order and
  redundant-nullness filtering now follow the Rust stub policy.
  Existing unqualified source-type references now prevent conflicting imports;
  ordinary type-use annotations, annotated dimensions and varargs use the shared
  AST import builder. Exact custom-JDK binding contents retain the
  standalone-parser limitation above.
* **Lombok** is not supported in any feature.

## Saved work branches

The project-import WIP branch `worktree-agent-a1e31779b8b46b008` (`af6a7a8`)
is now integrated and verified. No saved WIP branch remains unmerged.

## Largest remaining work

| Work | Upstream tests | Share of suite |
|---|---:|---:|
| Remaining quick fixes and assists (`correction`) | 536 | 26% |
| Remaining completion (CompletionHandlerTest 42, LazyResolve 20, Chain 12, Postfix 29) | 103 | 5% |
| Remaining project managers | 79 | 4% |
| Refactoring | 119 | 6% |
| Remaining handlers outside completion: code actions, generation, imports, save actions, markers and lifecycle/init | 206 | 10% |
| Core utilities, preferences, commands and the rest | 233 | 11% |

## Updating this file

Ported and ignored counts come from the test files:

```sh
for f in tests/*.rs; do b=$(basename "$f" .rs); case "$b" in lsp|*_regressions) continue ;; esac
  echo "$b $(grep -c '#\[test\]' "$f") $(grep -c '#\[ignore' "$f")"; done
```

Add the ports that live as unit tests in `src/` (InlayHintFilterManagerTest 7,
JavaDoc2Markdown 19, JavaDoc2PlainText 2, JavaDocImageExtraction 1, InitHandler 2).
Exclude `tests/lsp.rs`, all `tests/*_regressions.rs` files, and empty placeholders
(these are not ports). Upstream counts come from `@Test` methods in
`eclipse.jdt.ls/org.eclipse.jdt.ls.tests*/src`.
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

## Override/implementation integration evidence

All ten `OverrideMethodsTestCase` methods and the `OverrideMethodsActionTest`
method are ported through the public endpoints, preserving upstream discovery,
method/import counts, selections and complete cursor-insertion expectations.
Fixtures use the Java 21 fake JDK and original formatter settings. JavaModel's
`createType` snippets are realized inside the existing package `p` compilation
unit, including the snippet with `package P`; the package-mismatched
`Test480682.java` compilation-unit fixture remains verbatim. A name selection
exercises the operation's null-cursor append behavior on the requested type.

* Rust owns `java/listOverridableMethods`, `java/addOverridableMethods`, inherited
  method discovery, visibility, override/subsignature checks, final-method
  suppression, Cloneable flags, DTOs, selection, imports, implementation stubs,
  cursor insertion and the override/implement prompt actions. The approximate
  Java source-action generator is removed; the bridge exports the complete
  Object member graph for interface targets as semantic data.
* Thirty-three extra regressions cover binding DTOs, overloads, package visibility,
  static/private/own/final exclusions, covariance and generic substitution,
  primitive/reference/Optional defaults, synchronized modifiers, bounds,
  varargs, throws, argument affixes, task tags, direct and indirect interface
  super calls, interface default bodies and bodyless Object methods. They also
  cover record/enum behavior, named/local/anonymous type selection, body
  templates, binding-key-only selections, ordering/deduplication, empty/stale
  selections, capability/kind filtering, command fallback, resolve,
  module/package-info exclusions and open-buffer UTF-16/CRLF edits.
  Thirty-two file-backed cases pass against the oracle; one Rust-only case checks
  untitled, in-memory and nonexistent-file documents.
* Reference quirks are retained: requested methods follow inherited binding
  order; duplicates are deduplicated; wholly stale keys return an empty edit;
  comments remain disabled even when generateComments is enabled. Interface
  defaults use the ordinary throwing body template, while classes use the
  alternative template and super/default-return statements. Object declarations
  in interfaces have no body, with no Override annotation on clone/finalize.
  Synthetic record methods allow explicit Object overrides. Local type names
  use JavaModel names rather than binary names, and body-template blank lines
  preserve JDT's exact indentation.
* The shared import context now checks existing unqualified type references.
  The unchanged bug119171 fixture generates a fully qualified
  `java.util.Properties` parameter and no import, preserving `p.Properties`.
  Remaining ScopeAnalyzer, annotation, custom-JDK and template dependencies are
  listed above and are not counted as full feature parity.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 75 targets (1,158 passing, 48 ignored). The eleven upstream ports and
thirty-two file-backed regressions pass with `JDTLS_ORACLE=1 --test-threads=3`.
The oracle log reports 44 harness passes, including the virtual-buffer test that
returns early; 43 cases actually exercise the oracle. Logs are in
`target/parity-evidence/override-full-suite-final-2.log` and
`target/parity-evidence/override-oracle-final-9.log` (gitignored). The ledger now
has 751 upstream ports, with 1,336 upstream tests still unported.

## Abstract/native/unimplemented-method correction evidence

All eight `AbstractMethodQuickFixTest` methods and the two unimplemented-method
cases from `LocalCorrectionQuickFixTest` preserve the upstream fixtures, labels,
settings and complete edited-source expectations. At that stage, the other 85
methods in `LocalCorrectionQuickFixTest` were unported. The existing lifecycle
`test_unimplemented_methods` was enabled with its original action-count and kind
assertions; the dead-code batch below subsequently enables its remaining ignored case.

* Rust owns abstract/native modifier corrections, missing method bodies,
  make-type-abstract edits, inherited method selection, source-position ordering,
  enum-constant/anonymous-class targets, imports and implementation edits.
  The bridge exports compiler subsignature and override relations as semantic
  data; it does not select methods or generate these corrections.
* Unimplemented methods use the ordinary throwing method-body template and
  project Javadoc/Override preferences. The custom override command retains its
  alternative class template and existing comment behavior. Shared inherited
  comment expansion handles tags and `see_to_overridden` without changing
  delegate comments.
* Twenty-nine own regressions cover body preservation/removal, primitive,
  reference, Optional and old-style array defaults, constructors, interface
  static/default alternatives, annotations, modifier ordering, generic
  substitution/bounds, covariance, varargs, throws, concrete/default suppression,
  source order and reversed interface order, enum constants and anonymous
  classes. They also check templates, comment/annotation preferences, import
  conflicts, cross-package protected inheritance, deferred resolution with
  resource edits, diagnostics, unsaved Unicode/CRLF buffers and unchanged disk
  contents. Twenty-eight file-backed cases pass against the oracle; one Rust-only
  case uses untitled, in-memory and nonexistent-file documents with real
  diagnostics.
* Reference quirks are retained: make-type-abstract appends `abstract` even after
  `final`; removing `native` replaces an existing body with a default return;
  an imported Optional receives `null` while a fully qualified Optional receives
  `java.util.Optional.empty()` in missing-body corrections. Own abstract methods
  remain selected when another inherited method is missing. Inherited interface
  methods precede superclass methods, and directly implemented interfaces sort
  in reverse declaration order.
* The old-style array regression caught an incorrect AST property lookup and now
  receives `return null`. The cross-package regression caught compilation-order
  dependence in ECJ's source name environment: source package prefixes are now
  indexed before compiling units, allowing superclass resolution, missing-method
  diagnostics and the matching correction to work across packages.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 78 targets (1,198 passing, 47 ignored). The ten new upstream ports,
all eighteen active lifecycle cases and twenty-eight file-backed regressions
pass with `JDTLS_ORACLE=1 --test-threads=3`. The oracle log reports 57 harness
passes, including the virtual-buffer case that returns early; 56 cases actually
exercise the oracle. Logs are in
`target/parity-evidence/method-corrections-full-suite-final-1.log` and
`target/parity-evidence/method-corrections-oracle-final-2.log` (gitignored).
At the end of that batch the ledger had 761 upstream ports, 715 passing and
46 ignored; 1,326 upstream tests remained unported. Shared nullness/type-use
annotation, scope and global
preference/template dependencies above remain unfinished; this batch does not
claim full feature parity.

## Dead/unreachable-code correction evidence

Twenty-eight dead/unreachable-code methods from `LocalCorrectionQuickFixTest`
are now ported with their original fixtures, compiler options, proposal labels,
complete edited sources and assertion styles. At the end of that batch the class
had 30 of 87 methods ported; 57 remained unported. Lifecycle `test_remove_dead_code_after_if` is enabled
with its unchanged assertions, bringing all 19 ported lifecycle cases to passing.

* Rust owns unreachable-node selection, statement-tail removal through the next
  switch case, removal or promotion of control bodies, conditional-expression
  replacement, explicit casts/imports and the split `&&`/`||` proposals. The old
  approximate Java unreachable-code action and its dispatch are removed.
* The semantic bridge exports compiler assignment relations, functional-interface
  method bindings and invocation hierarchy members. Rust uses these facts to
  preserve primitive widening, boxing/unboxing, raw generic conversion and
  overloaded method/constructor selection when replacing a conditional expression.
  Return-target lookup stops at unresolved lambdas and declaration boundaries.
* Moving a surviving block uses its contiguous statement range, preserving
  comments, blank lines and separators. The range API is scoped to a removed or
  replaced block; it does not claim the full general ListRewrite range API.
  The ClassInstanceCreation placeholder uses JDT's SimpleType default, fixing
  cast formatting around moved constructor expressions.
* Twenty-six own regressions cover numeric/reference conversions, local/external/
  inherited/constructor overloads, lambdas and method references, imports,
  control-body preservation, switch statements/expressions, copied split branches,
  parentheses and an exact comment/blank-line expectation. They also exercise
  deferred resolution, resource edits, unsaved Unicode/CRLF buffers, unchanged
  disk sources and real diagnostics for untitled, in-memory and nonexistent-file
  documents. Twenty-five file-backed cases run against the oracle; the virtual
  document case runs only against Rust.
* Reference quirks are retained: a false `for` body's statements are promoted;
  removing an always-true condition also drops its preceding side-effecting
  operand; replacing a preceding `if` with its body preserves that body's braces.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 79 targets (1,253 passing, 46 ignored). With `JDTLS_ORACLE=1` and
`--test-threads=3`, the 30 local-correction ports, 19 lifecycle ports and 25
file-backed regressions pass against jdt.ls 1.58.0. The oracle log reports 75
harness passes including the virtual-document case that returns early; 74 cases
actually exercise the oracle. Logs are in
`target/parity-evidence/dead-code-full-suite-final-2.log` and
`target/parity-evidence/dead-code-oracle-final-3.log` (gitignored).
At the end of that batch the ledger had 789 upstream ports, 744 passing and
45 ignored; 1,298 upstream tests remained unported. Full TypeEnvironment conversion parity, inherited/import
scope and nullness/type-use annotation dependencies remain unfinished alongside
the earlier global preference/template work. This batch does not claim full
feature parity or the rest of LocalCorrectionsSubProcessor/quick assists.


## Unused-declaration correction evidence

All ten unused-declaration methods from `LocalCorrectionQuickFixTest` now
preserve the upstream fixtures, compiler options, labels and complete edited
sources, including resource-operation support and the existing getter/setter
alternatives. At the end of that batch the class had 40 of 87 methods ported;
47 remained unported.
The upstream method named `testUnusedTypeParameter` actually removes an unused
private nested type. Separate regressions verify real class/method type parameters.

* Rust ports the unused-member and unused-parameter fixes, binding-based reference
  selection, assignment/increment removal, side-effect extraction and statement
  or initializer-list replacement. Fragment splitting keeps initializer evaluation
  in source order, and conditional initializers use the upstream branch rules.
  The approximate Java declaration/member removal, assignment removal, parameter
  deletion and parameter-documentation generators and their dispatch are removed.
* Private-method parameter removal updates bound calls in the same unit, removes
  its Javadoc tag and checks own/inherited method names before choosing a conflict
  suffix. Existing method references suppress this fix. Unused type parameters
  can be removed or documented; documentation respects compiler options and tag
  order. The Java 22 unnamed-variable alternative uses the upstream declaration-
  kind restrictions and source-level gate.
* Thirty-two own regressions cover shadowed names and other classes, nested and
  ordered effects, constructors, chained assignments, control bodies, field-access
  receivers, loops and fragment splitting, conditional evaluation, overload/name
  collisions, method references, Javadoc removal/insertion, real type parameters,
  disabled documentation and Java 21/22 lambda/loop behavior. Deferred resolution,
  resource edits, unsaved Unicode/CRLF buffers, unchanged disk contents and real
  diagnostics for virtual documents are also covered. Thirty-one file-backed
  cases run against the oracle; one virtual-document case runs only against Rust.
* Reference quirks are retained: unused field initializers are discarded even when
  effectful; conditional constructor branches are discarded by the node-kind rule;
  chained assignments keep the surviving assignment even in remove-all mode.
  An effectful fragment in a multiple-variable `for` initializer has no keep edit,
  so that empty proposal is filtered out. Typed lambda parameters do not receive
  the unnamed-variable alternative supported for implicitly typed parameters.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 80 targets (1,295 passing, 46 ignored). With `JDTLS_ORACLE=1` and
`--test-threads=3`, all 40 local-correction ports and 31 file-backed regressions
pass against jdt.ls 1.58.0. The oracle log reports 72 harness passes including
one virtual-document case that returns early; 71 cases actually use the oracle.
Logs are in `target/parity-evidence/unused-full-suite-final-2.log` and
`target/parity-evidence/unused-oracle-final-4.log` (gitignored).
At the end of that batch the ledger had 799 upstream ports, 754 passing and
45 ignored; 1,288 upstream tests remained unported. This batch ports the individual unused-declaration fixes,
not the full unused-code cleanup, general linked-node finder, Javadoc processor,
or remaining LocalCorrectionsSubProcessor. Earlier conversion, scope, annotation
and global preference/template dependencies remain unfinished.

## Unreachable-catch and unused-throws correction evidence

All four unneeded-catch and four unnecessary-thrown-exception methods from
`LocalCorrectionQuickFixTest` preserve the upstream fixtures, compiler and
formatter options, proposal labels and complete edited-source expectations.
At the end of that batch the class had 48 of 87 methods ported; 39 remained unported.

* Rust ports catch removal, selected multi-catch alternative removal, catch-to-
  throws replacement, existing exception/supertype checks and method/initializer
  restrictions. Resources, other catches and `finally` preserve the try statement;
  otherwise the surviving body is promoted, with braces for multiple statements
  in a control body. Copying a statement range retains comments and blank lines.
* Rust removes unused thrown types, their matching `@throws`/`@exception` tags
  and imports when the reference type counter finds no other counted use. The
  shared Javadoc helper now renders thrown-type names without annotations or
  type arguments. The approximate Java thrown-exception action and dispatch are
  removed, including their obsolete proposal title.
* Twenty-two own regressions cover empty/single/multiple bodies, resources,
  `finally`, multi-catch selections, subtype alternatives, existing throws,
  annotations, imports, type arguments, Javadoc and exact comment preservation.
  Deferred resolution, diagnostic attachment, resource edits, unsaved Unicode/
  CRLF buffers and unchanged disk contents are also covered. Twenty-one cases
  exercise the oracle; one Rust-only case uses real diagnostics for untitled,
  in-memory and nonexistent-file documents.
* Reference quirks are retained: catch-to-throws is offered even when an overridden
  method does not declare the exception; type-literal references do not contribute
  to the import-removal counter. The copy retains type annotations while Javadoc
  uses the ordinary qualified type name.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 81 targets (1,325 passing, zero failures, 46 ignored). With
`JDTLS_ORACLE=1` and `--test-threads=3`, all 48 local-correction ports and the
file-backed dead-code, unused-code and exception regressions pass against jdt.ls
1.58.0. The oracle log reports 128 harness passes, including three virtual-document
cases that return early; 125 cases actually exercise the oracle. Logs are in
`target/parity-evidence/exception-corrections-full-suite-final-1.log` and
`target/parity-evidence/exception-corrections-oracle-final-1.log` (gitignored).
At the end of that batch the ledger had 807 upstream ports, 762 passing and
45 ignored; 1,280 upstream tests remained unported. This ports the individual catch/throws corrections, not
the full ChangeMethodSignatureProposal, generic type-reference counter, general
ListRewrite range API, Javadoc processor or uncaught-exception corrections.
Earlier conversion, scope, annotation and global preference/template gaps remain.

## Expression/operator and NLS-tag correction evidence

The expression-to-local-variable method, both invalid-operator parentheses
methods and the unnecessary-NLS-tag method from `LocalCorrectionQuickFixTest`
retain their upstream sources, compiler/formatter options, labels, assertion
styles and complete edited-source expectations. The class now has 52 of 87
methods ported; 35 remain unported.

* Rust ports invalid negation grouping around `instanceof` and infix operators,
  plus the comparison finder and exact insertion ranges for bitwise equality
  expressions. Comments, spacing and all intervening operands are preserved.
* Rust ports `NewLocalVariableCorrectionProposalCore` selection, unparenthesized
  initializer copying, recovered-cast fallback, type/import handling and reserved
  identifier discovery. The reserved-name walk retains the reference visitor's
  exclusions for nested types and variable initializers. Parameter affix ordering,
  numeric exclusions, Java character casing and identifier categories now share
  the naming helper with completion.
* Rust ports `StringFixCore.getReplace` for unnecessary NLS tags, including leading
  indentation, line delimiters, trailing comment text, one/two slash characters,
  adjacent tag markers and the EOF rule. The EOF fixture uses an incomplete type
  containing the tag. A tag after a completed top-level type triggers an oracle
  suppression-processor NPE; full suppression handling remains unported.
* Thirty-five own regressions cover operator comments and bitwise operand ranges,
  primitive/reference/array/generic/recovered types, imports, parameters, later
  declarations, field and nested-type exclusions, initializers, affix preferences
  and fallback, case-equivalent exclusions and Unicode case/identifier rules.
  They also check NLS comment retention, exact source edits, deferred resolution,
  diagnostic/resource edits, unsaved Unicode/CRLF buffers and unchanged disk
  sources. Thirty-four file-backed cases pass against the oracle; one Rust-only
  case exercises all three families with real diagnostics in virtual documents.
* Reference quirks remain: already-parenthesized invalid negation has no grouping
  proposal; new locals use parameter preferences and dimension zero for arrays;
  initializer corrections do not reserve initializer variables. The recovered
  qualified-generic fixture retains its qualified type and adds no import.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 82 targets (1,364 passing, zero failures, 46 ignored).
`expression-regressions-oracle-4.log` reports 35 passes, including the virtual
case that returns early; 34 cases actually exercise jdt.ls 1.58.0.
`expression-corrections-upstream-oracle-final-1.log` verifies all 52 local
correction ports and 54 of 55 active completion ports (two existing ignores).
The unchanged `test_snippet_ctor` retains the original upstream fixture and
assertion: the product returns literal `enclosing_simple_type` instead of
`AnotherClass`. It fails again in isolation in
`expression-completion-ctor-oracle-recheck-1.log`; Rust passes the assertion.
The completion row above now records this observed oracle difference rather than
claiming all active oracle cases pass. These logs and
`expression-corrections-full-suite-final-1.log` are under
`target/parity-evidence/` (gitignored).

The ledger now has 811 upstream ports, 766 passing and 45 ignored; 1,276 upstream
tests remain unported. Full annotation-aware import/type-node construction,
general ScopeAnalyzer/NamingConventions, string cleanup and suppression processors
remain unfinished, alongside the earlier conversion, scope and template gaps.
This batch does not claim full feature parity.

## Annotation-aware type imports and generation evidence

The AST overload of JDT `ImportRewrite.addImport`, its annotation/value builders,
owner-type construction and array dimensions are now ported to Rust. Constructors,
overrides, delegates, unimplemented-method corrections and expression-local
corrections use this builder. The string overload remains available for binding
DTOs and annotation-free names.

* Java exports annotation type bindings, explicitly declared member values,
  declaration/parameter/type annotations and capture wildcards as compiler facts.
  It exports distinct type variants when annotations differ on the type, arguments,
  owner or array dimensions, while preserving original compiler keys for semantic
  equality. Annotation fingerprints use qualified compiler identities and
  structured values, rather than JDT's simple-name display text.
* Rust creates marker, single-member and normal annotations; nested annotations;
  boolean, numeric, UTF-16 character and escaped string values; enum accesses;
  class literals; and singleton/empty/multiple-value annotation arrays. Defaults
  remain implicit and constant expressions use the compiler's resolved values.
  Annotation types, enum owners and class literal types participate in import
  conflict handling. An annotated qualified type retains its annotation after
  the final qualifier, as required by Java syntax.
* Rust retains annotated primitives, type variables, wildcard bounds, generic
  arguments, parameterized owners and every array dimension. The parameter helper
  ports `StubUtility2Core.createParameters`' special varargs handling: annotations
  on the innermost dimension move to `...`, with other dimensions kept in order.
  Capture normalization and the nested-capture check now follow JDT's wildcard
  rules instead of discarding every captured type argument.
* Thirty-seven own regressions exercise these paths through the public generation
  and correction endpoints, including differently annotated variants of the same
  type, equally named annotations in different packages, repeated annotations,
  import conflicts, negative/numeric/escaped values, unsaved buffers, inherited
  signatures and constructor/delegate/override varargs. Thirty-six file-backed
  cases pass on jdt.ls 1.58.0; one Rust-only case covers untitled, in-memory and
  nonexistent-file documents without writing sources to disk.
* The first full run exposed an existing harness defect: quick-fix problem
  positions were round-tripped through disk text, relocating selections when
  an unsaved buffer had different lines. The harness now retains the diagnostic's
  working-copy position. Fixtures and expected edits are unchanged. The isolated
  open-buffer dead-code case passes after this correction.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 83 targets (1,401 passing, zero failures, 46 ignored).
`type-import-oracle-final-1.log` verifies all 92 affected upstream ports across
constructor, delegate, override and abstract/local-correction classes, plus the
37-case harness run (36 actual oracle cases and one virtual-document early return).
`type-import-full-suite-final-2.log` and the oracle log are under
`target/parity-evidence/` (gitignored). The preceding full run is retained as
`type-import-full-suite-final-1.log`; its only failure was the harness selection
issue described above.

This shared dependency port adds no upstream test methods to the ledger: 811
ports, 766 passing and 45 ignored remain, with 1,276 upstream methods unported.
At this point `TypeLocation` and the annotation-filter context hook were available;
the redundant-nullness filter and inherited declaration/nullness annotation policy
were the next dependencies to port (see the following batch). Other generation
callers still need to adopt the AST overload;
full ScopeAnalyzer, NamingConventions and template dependencies remain unfinished.
Full feature parity is not yet achieved.


## Nullness filtering and inherited stub annotations (2026-10-05)

Rust now ports `RedundantNullnessTypeAnnotationsFilter` and
`StubUtility2Core.isCopyOnInherit` for constructors, delegates, overrides and
unimplemented-method quick fixes. Java exports resolved annotation values,
including defaults, source modifier tokens and package/module binding links;
Rust chooses what to retain and writes the edits.

* The nearest explicit nonnull default wins, including false or empty defaults
  that cancel an outer default. Boolean, enum-array, marker and
  `TypeQualifierDefault` annotations follow Eclipse's location rules. Configured
  secondary default names are recognized. Source package-info annotations are
  exported directly because standalone JDT package bindings require a workspace
  search environment to expose them.
* Redundant primary nonnull type annotations are removed only at locations
  covered by the default. Nullable and unrelated annotations remain. Exception,
  local, cast, new, receiver and instanceof locations strip primary nullness
  annotations; type variables and wildcards preserve their annotations. The
  upstream OTHER location removes all annotations. Varargs dimension annotations
  follow the existing JDT bypass and unknown-location rules.
* Only configured primary declaration nullness annotations are inherited.
  `inheritNullAnnotations=enabled` suppresses this copying. Override return and
  parameter declarations honor the target defaults; constructor and delegate
  declaration parameters use Eclipse's separate copying rule. Overridden source
  methods preserve the relative order of retained keywords and annotations.
* Interactive null-analysis configuration now preserves explicit project compiler
  options, matching `Preferences.updateAnnotationNullAnalysisOptions`. The client
  prompt/notification remains unported.
* Thirty-nine own regressions cover the public generation and correction paths,
  package/class/method defaults and cancellation, custom and secondary names,
  generic/wildcard/owner/array locations, declared annotations, external source
  modifier order, inheritance preferences, varargs and virtual documents. The
  virtual-document case runs on Rust only and checks untitled, in-memory and
  nonexistent-file documents without writing sources to disk.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 84 targets (1,440 passing, zero failures, 46 ignored).
`nullness-generation-oracle-final-1.log` passes all 168 harness cases: 92 affected
upstream ports, 37 earlier type-import regressions and 39 nullness regressions.
Two virtual-document cases return early on the oracle, leaving 166 actual oracle
cases. The same fixtures and assertions are used on both servers. The Rust log is
`nullness-generation-full-suite-final-1.log`; a final compile check after formatting
is `nullness-generation-format-check-final-1.log`. All logs are under
`target/parity-evidence/` (gitignored).

This dependency batch adds no upstream test methods to the ledger: 811 ports,
766 passing, 45 ignored and 1,276 unported remain. Source-module annotations and
binary package annotation discovery still need compiler-environment support;
other generation callers still need the AST import overload. Full ScopeAnalyzer,
NamingConventions and template dependencies remain unfinished. Full feature
parity is not yet achieved.


## Uncaught-exception and resource-closing correction evidence

Twenty additional `LocalCorrectionQuickFixTest` methods preserve the upstream
sources, selections, expected labels and complete resulting source. The class
now has 72 passing ports out of 87 upstream tests.

* Rust owns uncaught-exception collection, declared/caught-exception filtering,
  hierarchy ordering, subtype elimination, surround proposals, additional catches,
  existing multi-catch updates and throws-declaration changes. Binary overrides
  restrict new throws to their inherited contract; source overrides retain the
  reference behavior. More-specific existing throws and matching Javadoc tags
  are replaced, and imports are added or removed through the shared rewrite.
* Lexical scope analysis chooses catch names with the project exception-variable
  preference. Catch templates retain task tags and enclosing type/method variables,
  including the different exception-type variables used for ordinary and resource
  catches. Selection-aware source ranges keep leading comments outside a surround
  correction and include selected trailing comments.
* Expression lambdas become blocks containing the try/catch. Method references
  become lambdas with invocation/creation bodies, return versus expression
  statements, parameter names, name-conflict suffixes and receiver handling.
  Exceptions stay inside the lambda instead of adding throws to the outer method.
  Local declarations used after a selection stay in their original scope;
  initializers become assignments inside the try, `final` is removed, and escaping
  `var` declarations use their inferred type and the reference proposal label.
* Try-with-resources moves selected resource declarations and repeatedly extends
  the body through dependent local uses. It includes close-method exceptions,
  extends an existing try when appropriate, and rethrows narrower exceptions
  already declared by the enclosing method before handling a broader close type.
* Java exports only compiler facts for local-type methods and functional-interface
  methods. The legacy message-regex throws and try/catch action generators are
  removed. The shared rewrite flattener now uses JDT's valid `MISSING()` expression
  statement placeholder, which lets the formatter process copied expressions.
* `uncaught_exception_regressions` has 18 passing Rust cases: 17 complete-source
  snapshots captured from JDT LS, plus a Rust-only regression that exercises both
  uncaught and resource corrections on untitled, in-memory and missing-file buffers.
  The snapshots cover lambda boundaries, bound/static/unbound/creation references,
  parameter conflicts, escaping locals, resource lifetimes/rethrows, existing try
  statements, comments and custom templates.

Verification: `CARGO_INCREMENTAL=0 cargo test --no-fail-fast --bins --tests`
passes all 85 targets (1,478 passing, zero failures, 46 ignored), recorded in
`target/parity-evidence/uncaught-exceptions-full-suite-final-1.log`.
`uncaught-exceptions-oracle-final-1.log` passes all 90 harness cases with
`JDTLS_ORACLE=1 --test-threads=1`: 72 upstream ports and 18 additional cases,
including the virtual-buffer case that skips oracle requests (89 actual oracle
comparisons). An earlier concurrent oracle run returned a malformed surround edit
for `test_uncaught_exception_on_super4`; its isolated rerun and the final serial
run both pass, with the upstream assertion unchanged.

The ledger now has 831 upstream ports, 786 passing and 45 ignored, leaving 1,256
upstream methods unported. The 15 remaining LocalCorrectionQuickFixTest methods
cover unchecked conversions, variable hiding, duplicate methods and unused
allocations. Pattern-variable scope expansion and broader method-reference,
synthetic-SAM and surround-selection coverage still need porting and verification,
alongside the remaining quick assists and refactorings. Full feature parity remains
unfinished.

## Unused allocations and expression assignments

Three faithful `LocalCorrectionQuickFixTest` ports and both
`AssignToVariableRefactorTest` methods are now active. Rust generates unused
allocation corrections (throw, return, remove, local/field assignment and resource
assignment) and ordinary expression assignment refactors. The approximate Java
assignment generators are removed. Java supplies expression and method type facts;
Rust owns proposal eligibility, ordering, names, imports and source edits.

Assignment handles anonymous types, recovered semicolons, control-statement bodies,
field placement/static/final preferences, naming affixes and collisions, and
static nested-type field visibility. Resource assignment includes close exceptions,
existing catches, rethrows and lambda SAM exception boundaries. Advanced clients
receive `java.action.applyRefactoringCommand`; `java/getRefactorEdit` supports
`assignVariable` and `assignField`, returning the complete edit and exact UTF-16
rename position after formatting and imports. Other refactoring commands and
parameter assignments remain unfinished.

`allocation_correction_regressions` has 24 passing cases: 22 exact oracle
comparisons and two Rust-only virtual-document cases. Together with the existing
uncaught virtual-buffer case, the serial allocation/local/uncaught verification
has 119 harness passes and 116 actual oracle comparisons. Evidence:
`unused-allocation-oracle-final-1.log` (118 harness passes; 115 actual comparisons),
`ignored-restored-oracle-final-1.log` (the added nested-type scope assertion), and
`unused-allocation-scope-rust-green-2.log` (all 24 Rust cases).
The original three allocation fixtures retain their upstream tabs and assertions.
LocalCorrectionQuickFixTest now has 75 of 87 upstream methods ported; remaining
methods cover unchecked conversions, variable hiding and duplicate methods.

## Restored ignored tests

Sixteen previously ignored upstream tests now run with their original assertions.
Their setup uses the upstream sourceless `rtstubs.jar` matching each Eclipse
fixture's JRE container (Java 8 for hello, Java 18 for java18). Maven binary tests
retain their original dependencies and add the Java 8 stub library. Their oracle
URI resolver selects that explicit fixture root when the public symbol search
also reports the host JDK.

Restored coverage: code lenses (1), call hierarchy (1), workspace symbols (3),
completion (2), hover (1), definition/type definition (3), class-file providers (3),
references (1), and implementations (1). Fixes include using the imported
project's declared runtime for completion and symbol-search candidates, preserving
null computed completion parameter names, matching Eclipse's line-offset arithmetic,
and searching transitive binary type implementations in Rust over class-file facts.
Binary method implementation expansion and broader mixed source/binary hierarchy
coverage still require porting.

`ignored-restored-oracle-final-1.log` verifies 15 restored tests plus the added
allocation scope regression in serial, with zero failures.
`ignored-binary-implementations-oracle-final-1.log` verifies the sixteenth restored
test against Eclipse. `allocation-and-ignored-tests-full-suite-final-2.log` records
the full Rust suite: 87 targets, 1,523 passes, zero failures, 30 ignores.
The first full run exposed 63 regressions in seven existing targets because
ECJ's standalone DOM parser still requires VM bootstrapping for Java 21
stub projects. That bootstrap is retained; completion/type-search roots follow
the project classpath separately. The targeted rerun passes all 157 tests in those seven targets plus completion,
workspace symbols and implementations (`ignored-runtime-bootstrap-rust-green-1.log`).
The fresh final full run covers that correction.
At the end of that batch, the remaining 29 ignored upstream ports concerned unsupported features, test-plugin
internals, direct internal APIs, required VMs and Gradle model behavior; they are
still excluded from the passing ledger. Full parity remains unfinished.

## Web diagnostics and Rust provider policy (2026-10-06)

The local web clients now explicitly enable full diagnostics for their unsaved
non-project buffers before `didOpen`, and repeat that setup after reconnecting.
The server retains Eclipse's syntax-only default. Both editor pages render
language-tagged hover signatures as code rather than joining objects into text.
Playwright verified type mismatches, missing `List`/`ArrayList` imports, clearing
errors after corrections, hover signatures, and validation after reload on `/`
and `/index2.html`. No console errors or failed requests occurred. See
[WEB_TESTING.md](WEB_TESTING.md) for startup steps. `web/` and `ui/` remain local,
gitignored demo files.

The Rust lifecycle regression also verifies type-error ranges and clearing after
edits for a missing-file buffer, an untitled buffer and an in-memory buffer,
without creating a source file on disk. Evidence:
`web-java-browser-2.log`, `web-diagnostics-rust-1.log`, and `web-java-*.png` in
`target/parity-evidence/`.

`java/classFileContents` now selects its attached-source and FernFlower providers
through Rust policy. Java supplies attached source, raw decompiled text and raw
line pairs. Rust applies URI matching, provider priorities, preferred IDs,
fallback, cancellation checkpoints, preference injection and fresh provider
construction. An integration regression switches between decompiled and attached
source using preferences on successive requests; it also passes against Eclipse.
Four additional policy regressions cover preference identity, error fallback and
duplicate-provider logs, cancellation, and the absence of manager result caching.
These four tests are not counted as upstream ports.

Two ignored upstream tests (`test_open_nothing`, `test_decompile_nothing`) now call
the Rust manager directly, preserving the upstream null-input assertions. A thin
standalone Java probe checked both calls against the actual manager from the
oracle's `org.eclipse.jdt.ls.core_1.58.0.202604151538.jar`, rather than using an
invalid LSP URI as a substitute. `content-provider-null-oracle-1.log` records that
probe. `content-provider-oracle-1.log` separately verifies all six active LSP
provider ports and all six binary-editor regressions against Eclipse.

The other 13 provider ports remain ignored until their actual test-extension
registration, complete log assertions and decompiler mapping fixtures are
verified. Raw mapping transport and Rust mapping conversion are implemented;
the upstream mapping test remains unfinished and excluded from the passing count.
The current ledger is 836 ported, 809 passing, 27 ignored and 1,251 unported.
`web-and-provider-full-suite-1.log` records the fresh full Rust run: 88 targets,
1,531 passes, zero failures and 28 ignores (including the optional Javadoc corpus).

The follow-up local web check also verifies applying a missing-import quick fix.
The primary page previously omitted code-action capabilities and stripped the
Java diagnostic source, code and data from its request, which produced no import
action. It now preserves the diagnostic and uses a valid selection range. The
adapter page enables its advanced providers and supplies only selected-marker
diagnostics. A popup stacking fix lets mouse clicks reach the action menu. Both
pages show **Import 'java.util.ArrayList'**, apply the exact import edit, and clear
the unresolved-type error. Keyboard and mouse flows each pass two browser tests
without console errors or failed requests (`import-browser-final-3.log` and
`import-browser-final-4.log`). These changes are in the local, gitignored clients;
they do not change the upstream-test ledger or require a Rust behavior change.
