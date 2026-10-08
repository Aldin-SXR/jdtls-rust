# jdt.ls parity status

How far jdtls-rust is from eclipse.jdt.ls parity, measured against the upstream test
suite. For how the port is done, see [PORTING.md](PORTING.md).

* **Branch:** `jdtls-parity`, including the verified lifecycle/init/file-event,
  binary-editor, initial correction, completion and project-manager integrations. `main` is unchanged.
* **Reference:** eclipse.jdt.ls 1.58.0. The upstream checkout is 1.58.0-SNAPSHOT
  (2026-04-10), and the oracle in `.oracle/` is the 1.58.0 release.
* **Last updated:** 2026-10-08.

## Summary

The upstream suite at the `v1.58.0` tag has 2,117 tests (`@Test` and
`@ParameterizedTest` methods) in 198 classes (`org.eclipse.jdt.ls.tests` and
`org.eclipse.jdt.ls.tests.syntaxserver`). All counts on this page come from
`scripts/parity-count.py`; an earlier hand count of 2,087 missed tests, among them 39
of `CompletionHandlerTest`'s 156.

| | Tests | Share of upstream |
|---|---:|---:|
| Ported | 1,923 | 90.8% |
| Passing | 1,852 | 87.5% |
| Ported but `#[ignore]`d | 71 | 3.4% |
| Not ported yet | 194 | 9.2% |

Our own regression suites (`tests/lsp.rs`, `tests/*_regressions.rs`) and unit tests that
are not ports are excluded from these counts.

## By upstream area

| Area (`core.internal.*`) | Upstream | Ported | Passing | Passing % |
|---|---:|---:|---:|---:|
| handlers | 868 | 827 | 802 | 92% |
| correction | 610 | 590 | 558 | 91% |
| managers | 211 | 204 | 195 | 92% |
| refactoring | 118 | 106 | 103 | 87% |
| (root) | 72 | 10 | 10 | 13% |
| commands | 60 | 60 | 60 | 100% |
| preferences | 53 | 53 | 53 | 100% |
| javadoc | 32 | 32 | 32 | 100% |
| codemanipulation | 30 | 10 | 10 | 33% |
| filesystem | 21 | 21 | 19 | 90% |
| cleanup | 18 | 3 | 3 | 16% |
| syntaxserver | 14 | 0 | 0 | 0% |
| contentassist | 6 | 6 | 6 | 100% |
| framework/protobuf | 2 | 0 | 0 | 0% |
| corext | 1 | 1 | 1 | 100% |
| javafx | 1 | 0 | 0 | 0% |

## Ported classes

"Oracle" means the ported test file was also run against the real jdt.ls 1.58.0
(`JDTLS_ORACLE=1`). Oracle failures are listed with their cause.

| Upstream class | Test file | Ported | Pass | Ignored | Oracle |
|---|---|---:|---:|---:|---|
| handlers/AdvancedOrganizeImportsHandlerTest | `handlers_advanced_organize_imports_handler_test` | 5 | 5 | 0 | 5/5; dynamic chooser replies and unchanged Maven fixtures |
| handlers/BuildWorkspaceHandlerTest | `handlers_build_workspace_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/CallHierarchyHandlerTest | `handlers_call_hierarchy_handler_test` | 10 | 10 | 0 | 10/10; restored stub-JDK source-location assertion verified |
| handlers/CodeActionHandlerTest | `handlers_code_action_handler_test` | 25 | 22 | 3 | 21/25 passing here; the 4 ignored pass on the oracle |
| handlers/CodeLensHandlerTest | `handlers_code_lens_handler_test` | 14 | 14 | 0 | 14/14; restored two-lens binary assertion verified |
| handlers/CompletionHandlerLazyResolveTest | `handlers_completion_handler_lazy_resolve_test` | 20 | 20 | 0 | 20/20 |
| handlers/CompletionHandlerChainTest | `handlers_completion_handler_chain_test` | 12 | 12 | 0 | 12/12 |
| handlers/CompletionHandlerTest | `handlers_completion_handler_test` | 156 | 150 | 6 | all active ports verified on the oracle in targeted runs; see the completion evidence |
| handlers/DocumentHighlightHandlerTest | `handlers_document_highlight_handler_test` | 5 | 5 | 0 | pass |
| handlers/DocumentLifeCycleHandlerTest | `handlers_document_life_cycle_handler_test` + `src/document_store.rs` | 21 | 21 | 0 | 19/19 LSP cases; the two `DocumentMonitor` cases are unit tests. `testNonJdtError` (needs a generic resource-marker API) is not ported |
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
| handlers/SaveActionHandlerTest | `handlers_save_action_handler_test` | 4 | 4 | 0 | 4/4; unchanged hello fixtures and recovered syntax |
| handlers/SelectionRangeHandlerTest | `handlers_selection_range_handler_test` | 5 | 5 | 0 | 5/5 |
| handlers/SemanticTokensHandlerTest | `handlers_semantic_tokens_handler_test` | 11 | 11 | 0 | 11/11 |
| handlers/SignatureHelpHandlerTest | `handlers_signature_help_handler_test` | 56 | 55 | 1 | 54/55; `test_signature_help_erasure_type`, where jdt.ls returns no doc |
| handlers/SmartDetectionHandlerTest | `handlers_smart_detection_handler_test` | 2 | 2 | 0 | 2/2 |
| handlers/TypeHierarchyHandlerTest | `handlers_type_hierarchy_handler_test` | 4 | 4 | 0 | 4/4 |
| handlers/WorkspaceDiagnosticsHandlerTest | `handlers_workspace_diagnostics_handler_test` + `src/features/markers.rs` | 17 | 10 | 7 | 7/9 LSP cases (the two oracle failures are ignored here); marker conversion cases are unit tests. The m2e pom-marker cases (5) and `testEncoding` are not ported |
| handlers/WorkspaceExecuteCommandHandlerTest | `handlers_workspace_execute_command_handler_test` + `src/features/execute_command.rs` | 5 | 5 | 0 | 1/1 LSP case; the delegate-handler cases are unit tests of the registry with the test plug-in's contributions. `testRegistryEventListener` (OSGi bundles) is not ported |
| handlers/WorkspaceSymbolHandlerTest | `handlers_workspace_symbol_handler_test` | 19 | 19 | 0 | 19/19; all three restored stub-JDK assertions verified |
| correction/AssignToVariableRefactorTest | `correction_assign_to_variable_refactor_test` | 2 | 2 | 0 | 2/2 (advanced assignment commands) |
| cleanup/CleanUpsTest | `cleanup_clean_ups_test` | 3 | 3 | 0 | 3/3; no cleanup, invert equals, organize imports; 15 methods remain unported |
| correction/AbstractMethodQuickFixTest | `correction_abstract_method_quick_fix_test` | 8 | 8 | 0 | 8/8 |
| correction/LocalCorrectionQuickFixTest | `correction_local_correction_quick_fix_test` | 87 | 86 | 1 | 75/75 with `--test-threads=1`; 12 upstream methods remain unported |
| correction/ModifierCorrectionsQuickFixTest | `correction_modifier_corrections_quick_fix_test` | 42 | 40 | 2 | 42/42 |
| correction/NonProjectFixTest | `correction_non_project_fix_test` | 2 | 2 | 0 | 2/2; original source, action order, titles and command arguments |
| correction/OrganizeImportsActionTest | `correction_organize_imports_action_test` | 6 | 6 | 0 | 6/6; original sources and edit assertions |
| correction/TypeMismatchQuickFixTest | `correction_type_mismatch_quick_fix_test` | 44 | 44 | 0 | 44/44 |
| correction/UnresolvedVariablesQuickFixTest | `correction_unresolved_variables_quick_fix_test` | 48 | 48 | 0 | 48/48 |
| correction/UnresolvedTypesQuickFixTest | `correction_unresolved_types_quick_fix_test` | 35 | 21 | 14 | 31/33 non-disabled; `test_add_all_missing_imports` and `test_type_in_sealed_type_declaration` fail on the oracle too |
| correction/GetterSetterQuickFixTest | `correction_getter_setter_quick_fix_test` | 6 | 1 | 5 | 6/6 |
| correction/SerialVersionQuickFixTest | `correction_serial_version_quick_fix_test` | 5 | 5 | 0 | 5/5 |
| correction/RedundantInterfaceQuickFixTest | `correction_redundant_interface_quick_fix_test` | 2 | 2 | 0 | 2/2 |
| correction/UnnecessaryCastQuickFixTest | `correction_unnecessary_cast_quick_fix_test` | 1 | 1 | 0 | 1/1 |
| correction/UnresolvedMethodsQuickFixTest | `correction_unresolved_methods_quick_fix_test` | 91 | 91 | 0 | 91/91; the favorites tests resend settings once the server runs (jdt.ls copies live favorites) |
| codemanipulation/OverrideMethodsTestCase | `codemanipulation_override_methods_test_case` | 10 | 10 | 0 | 10/10 |
| refactoring/ExtractVariableTest | `refactoring_extract_variable_test` | 4 of 5 (one is commented out upstream) | 4 | 0 | 4/4 |
| JVMConfiguratorTest | `jvm_configurator_test` | 7 | 7 | 0 | 7/7 direct calls to the actual JVM/runtime APIs and unchanged upstream VM extension |
| commands/BuildPathCommandTest | `commands_build_path_command_test` | 4 | 4 | 0 | 4/4; unchanged Gradle 8.5 fixture runs on Java 21 |
| commands/DiagnosticsCommandTest | `commands_diagnostics_command_test` | 2 | 2 | 0 | 2/2 |
| commands/OrganizeImportsCommandTest | `commands_organize_imports_command_test` | 12 | 12 | 0 | 12/12; direct package collector plus per-CU wire operation, see below |
| commands/TypeHierarchyCommandTest | `commands_type_hierarchy_command_test` | 5 | 5 | 0 | 5/5 |
| javadoc/JavaDoc2MarkdownConverterTest | unit tests in `src/javadoc/converter.rs` | 19 | 19 | 0 | n/a (unit tests) |
| javadoc/JavaDoc2PlainTextConverterTest | unit tests in `src/javadoc/converter.rs` | 2 | 2 | 0 | n/a (unit tests) |
| javadoc/JavaDocImageExtractionTest | `javadoc_java_doc_image_extraction_test`, plus a unit test in `src/javadoc/path_handler.rs` | 6 | 6 | 0 | pass |
| javadoc/JavadocContentTest | `javadoc_javadoc_content_test` | 5 | 5 | 0 | pass |
| managers/ContentProviderManagerTest | `managers_content_provider_manager_test` | 21 | 21 | 0 | 21/21 direct API calls against the actual manager in an isolated Eclipse test-extension product |
| managers/BasicFileDetectorTest | `managers_basic_file_detector_test` | 12 | 12 | 0 | n/a (unit ports) |
| managers/EclipseBuildSupportTest | `managers_eclipse_build_support_test` | 1 | 1 | 0 | 1/1 |
| managers/EclipseProjectImporterTest | `managers_eclipse_project_importer_test` | 15 | 12 | 3 | 8/8 active LSP; 3 unit ports |
| managers/InvisibleProjectBuildSupportTest | `managers_invisible_project_build_support_test` | 4 | 4 | 0 | 2/2 active LSP; 2 preference unit ports |
| managers/InvisibleProjectImporterTest | `managers_invisible_project_importer_test` | 27 | 26 | 1 | active cases pass; helper assertions use the Rust port |
| managers/InvisibleProjectPreferenceChangeListenerTest | `managers_invisible_project_preference_change_listener_test` | 6 | 6 | 0 | 6/6 |
| managers/MavenProjectImporterTest | `managers_maven_project_importer_test` | 34 | 32 | 2 | 29/29 active LSP; 2 unit ports |
| managers/MultiRootTest | `managers_multi_root_test` | 2 | 2 | 0 | 2/2 |
| managers/ProjectsManagerTest | `managers_projects_manager_test` | 13 | 12 | 1 | 12/12 active, including unchanged Gradle successful-update and reload-marker assertions on Java 21; invalid-build status remains ignored |
| managers/StandardProjectManagerTest | `managers_standard_project_manager_test` | 1 | 1 | 0 | n/a (unit port) |
| preferences/ClientPreferencesTest | unit tests in `src/features/client_caps.rs` (`client_preferences_test`) | 16 | 16 | 0 | n/a (unit tests) |
| preferences/PreferencesTest | unit tests in `src/features/preferences/model.rs` (`preferences_test`) | 16 | 16 | 0 | n/a (unit tests) |
| preferences/PreferenceManagerTest | unit tests in `src/features/preferences/manager.rs` (`preference_manager_test`) | 15 | 15 | 0 | n/a (unit tests) |
| preferences/NullAnalysisTest | `preferences_null_analysis_test` | 6 | 6 | 0 | 6/6 |
| MovingAverageTest | `src/features/lifecycle.rs` | 1 | 1 | 0 | unit test; drives the adaptive validation debounce |
| handlers/MapFlattenerTest | `src/features/preferences/map_flattener.rs` | 5 | 5 | 0 | unit tests; lenient Gson list parsing |
| handlers/JavaSettingsTest | `handlers_java_settings_test` | 8 | 7 | 1 | 7/7 |
| handlers/PostfixCompletionTest | `handlers_postfix_completion_test` | 29 | 29 | 0 | 29/29 (oracle with `-Djava.lsp.joinOnCompletion=true`) |
| handlers/CompletionResolveHandlerTest | `handlers_completion_resolve_handler_test` | 2 | 1 | 1 | 1/1 |
| handlers/CompletionInsertReplaceCapabilityTest | `handlers_completion_insert_replace_capability_test` | 1 | 1 | 0 | 1/1 |
| handlers/CompletionRankingProviderTest | `handlers_completion_ranking_provider_test` | 2 | 0 | 2 | not applicable (registers a Mockito provider in-process) |
| handlers/CompletionRankingAggregationTest | `src/features/completion/ranking.rs` | 4 | 4 | 0 | unit tests |
| contentassist/SnippetUtilsTest | `src/features/completion/snippets.rs` | 5 | 5 | 0 | unit tests |
| contentassist/SortTextHelperTest | `src/features/completion/sort_text.rs` | 1 | 1 | 0 | unit test |
| corext/template/java/JavaLanguageServerTemplateStoreTest | `src/features/completion/template_store.rs` | 1 | 1 | 0 | unit test |
| commands/ProjectCommandTest | `commands_project_command_test` | 28 | 28 | 0 | 27 of 28; all pass on the oracle; `testUpdateSourcePaths` has no LSP command |
| commands/SourceAttachmentCommandTest | `commands_source_attachment_command_test` | 8 | 8 | 0 | 8/8 |
| commands/VmCommandTest | `src/project/runtime.rs` | 1 | 1 | 0 | unit test over a registry with a TestVMType install |
| correction/ConstructorQuickFixTest | `correction_constructor_quick_fix_test` | 3 | 0 | 3 | 3/3; ignored until ConstructorFromSuperclassProposal is ported |
| correction/ConvertToRecordQuickAssistTest | `correction_convert_to_record_quick_assist_test` | 32 | 32 | 0 | 31/32 (`test_convert_to_record11` is an oracle-internal NPE) |
| correction/JavadocQuickFixTest | `correction_javadoc_quick_fix_test` | 30 | 30 | 0 | 30/30 |
| correction/ReturnTypeQuickFixTest | `correction_return_type_quick_fix_test` | 4 | 4 | 0 | 4/4 |
| correction/StaticAccessQuickFixTest | `correction_static_access_quick_fix_test` | 4 | 4 | 0 | 4/4 |
| correction/StaticReferenceQuickFixTest | `correction_static_reference_quick_fix_test` | 4 | 4 | 0 | 4/4 |
| handlers/DiagnosticHandlerTest | `handlers_diagnostic_handler_test` | 7 | 7 | 0 | 7/7 |
| handlers/JDTLanguageServerTest | `handlers_jdt_language_server_test` | 3 | 3 | 0 | 3/3 |
| refactoring/ExtractFieldTest | `refactoring_extract_field_test` | 16 | 16 | 0 | 16/16 through java/getRefactorEdit |
| refactoring/ExtractMethodTest | `refactoring_extract_method_test` | 21 | 19 | 2 | 21/21 |
| CancellableProgressMonitorTest | `cancellable_progress_monitor_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/AssignToFieldQuickAssistTest | `correction_assign_to_field_quick_assist_test` | 6 | 6 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/AssistQuickFixTest21 | `correction_assist_quick_fix_test21` | 6 | 0 | 6 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/ConvertMethodReferenceToLambdaTest | `correction_convert_method_reference_to_lambda_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/ConvertSwitchExpressionQuickAssistTest | `correction_convert_switch_expression_quick_assist_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/ConvertToTextBlockQuickFixTest | `correction_convert_to_text_block_quick_fix_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/ConvertVarQuickFixTest | `correction_convert_var_quick_fix_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/LambdaQuickFixTest | `correction_lambda_quick_fix_test` | 9 | 9 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/NullAnnotationsQuickFix1d8MixTest | `correction_null_annotations_quick_fix1d8_mix_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/NullAnnotationsQuickFix1d8Test | `correction_null_annotations_quick_fix1d8_test` | 21 | 21 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/NullAnnotationsQuickFix9Test | `correction_null_annotations_quick_fix9_test` | 2 | 1 | 1 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/NullAnnotationsQuickFixTest | `correction_null_annotations_quick_fix_test` | 47 | 47 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/ReorgQuickFixTest | `correction_reorg_quick_fix_test` | 18 | 18 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/SortMembersQuickAssistTest | `correction_sort_members_quick_assist_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/StaticImportQuickAssistTest | `correction_static_import_quick_assist_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/StringConcatenationQuickFixTest | `correction_string_concatenation_quick_fix_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| correction/VariableQuickFixTest | `correction_variable_quick_fix_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| filesystem/EclipseProjectMetadataFileTest | `filesystem_eclipse_project_metadata_file_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| filesystem/GradleProjectMetadataFileTest | `filesystem_gradle_project_metadata_file_test` | 4 | 3 | 1 | passes on this server; see the agent notes in the commit history for oracle runs |
| filesystem/InvisibleProjectMetadataFileTest | `filesystem_invisible_project_metadata_file_test` | 4 | 3 | 1 | passes on this server; see the agent notes in the commit history for oracle runs |
| filesystem/JLSFsUtilsTest | `filesystem_jls_fs_utils_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| filesystem/MavenProjectMetadataFileTest | `filesystem_maven_project_metadata_file_test` | 6 | 6 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/ClasspathUpdateHandlerTest | `handlers_classpath_update_handler_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/CreateModuleInfoHandlerTest | `handlers_create_module_info_handler_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/FindLinksHandlerTest | `handlers_find_links_handler_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/ImportNewProjectsTest | `handlers_import_new_projects_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/ProgressReporterManagerTest | `handlers_progress_reporter_manager_test` | 5 | 5 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/ProjectConfigurationUpdateHandlerTest | `handlers_project_configuration_update_handler_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/ResolveSourceMappingHandlerTest | `handlers_resolve_source_mapping_handler_test` | 5 | 5 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/WorkspaceEventHandlerTest | `handlers_workspace_event_handler_test` | 3 | 2 | 1 | passes on this server; see the agent notes in the commit history for oracle runs |
| handlers/WorkspaceFolderChangeHandlerTest | `handlers_workspace_folder_change_handler_test` | 1 | 1 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| managers/GradleBuildSupportTest | `managers_gradle_build_support_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| managers/GradleProjectImporterTest | `managers_gradle_project_importer_test` | 45 | 43 | 2 | passes on this server; see the agent notes in the commit history for oracle runs |
| managers/GradleUtilsTest | `managers_gradle_utils_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| managers/MavenBuildSupportTest | `managers_maven_build_support_test` | 12 | 12 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| managers/MavenClasspathTest | `managers_maven_classpath_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| managers/WrapperValidatorTest | `managers_wrapper_validator_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/AdvancedExtractTest | `refactoring_advanced_extract_test` | 6 | 6 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/AnonymousClassCreationToLambdaTest | `refactoring_anonymous_class_creation_to_lambda_test` | 11 | 10 | 1 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/ConvertForLoopTest | `refactoring_convert_for_loop_test` | 1 | 1 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/GetRefactorEditHandlerTest | `refactoring_get_refactor_edit_handler_test` | 7 | 7 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/InferSelectionHandlerTest | `refactoring_infer_selection_handler_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/InlineConstantTest | `refactoring_inline_constant_test` | 4 | 4 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/InlineVariableTest | `refactoring_inline_variable_test` | 2 | 2 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/InvertConditionTest | `refactoring_invert_condition_test` | 20 | 20 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/InvertVariableTest | `refactoring_invert_variable_test` | 3 | 3 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/LambdaToAnonymousClassCreationTest | `refactoring_lambda_to_anonymous_class_creation_test` | 1 | 1 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |
| refactoring/MoveTest | `refactoring_move_test` | 6 | 6 | 0 | passes on this server; see the agent notes in the commit history for oracle runs |

## Ignored tests

Every ignore names its reason in the test file (`#[ignore = "..."]`), and every
ignored test remains counted as unfinished. All provider-manager ports now run
with their original internal assertions. The separate optional
`javadoc::converter::corpus_diff::corpus` unit test needs an external `JAVADOC_CORPUS`; it is the 12th ignored test in the full
Rust run and is excluded from the upstream-port count.

| Reason | Count | Tests |
|---|---:|---|
| Lombok not supported | 3 | `test_lombok_show_generated_code_symbols`; completion `test_completion_lombok`, `test_completion_lombok2` (both pass on the oracle with `-javaagent`) |
| `@Disabled` upstream | 3 | completion `test_snippet_inner_record`, `test_snippet_sibling_inner_record`, `test_snippet_nested_inner_record` |
| Needs `repository.aspose.com`, unreachable here (oracle fails the same way) | 1 | completion `test_completion_invalid_javadoc` |
| Kotlin not supported | 1 | `test_kotlin` |
| Direct completion-requestor state access | 1 | `test_signature_help_for_selected_completion_proposal` selects the first raw proposal directly, whose ordering differs from LSP items; the public selection flow is implemented and oracle verified separately |
| The upstream test assumes a Java 10 JDK | 1 | `test_hover_on_java10var` |
| Requires an installed Java 26 VM (no JDK 26 package or download is reachable in the container) | 3 | Eclipse `test_preview_features_disabled_by_default`; invisible `test_preview_features_enabled_by_default`; Maven `test_java26_project` |
| Oracle product lacks the resource-filter matcher available in the upstream test plugin | 1 | Eclipse `ignore_missing_resource_filters` |
| Internal project markers differ from published diagnostics | 1 | Eclipse `test_null_analysis` retains the upstream count of 2 markers |
| Needs the getter/setter ("Create getter and setter for") proposal | 2 | ModifierCorrections `test_invisible_field_requested_in_same_package1`, `test_invisible_field_requested_in_same_package2` |
| Unported type proposals: ambiguous-type "Explicitly import" (7), type parameters / import-only type change (8), `NewCUProposal` create type (8), add-all-missing-imports (2); `@Disabled` upstream (2) | 27 | `correction_unresolved_types_quick_fix_test` |
| Unported `GetterSetterCorrectionSubProcessor` / `SelfEncapsulateFieldRefactoring` (`src/correction/getter_setter.rs` is a stub) | 6 | `correction_getter_setter_quick_fix_test` |
| Gradle model/update parity for an invalid build | 1 | ProjectsManager `test_sending_warning_project_status`; successful-update and reload-marker assertions now run with the compatible Gradle VM |
| No LSP equivalent: upstream makes a working copy without a lifecycle handler (the oracle also fails over LSP) | 1 | WorkspaceDiagnostics `test_working_copy` (`test_working_copy2` is the LSP form) |
| Oracle publishes no "Unknown referenced nature" report (`isIgnored` drops CheckMissingNatures markers) | 1 | WorkspaceDiagnostics `test_missing_natures` |
| Needs JDT incremental-builder semantics (duplicate class-file locator problems) | 1 | WorkspaceDiagnostics `test_bad_location_exception` |
| The Rust build writes no class files to output folders | 1 | JavaSettings `test_configure_settings` |
| Registers a Mockito ranking provider inside the Java server | 2 | `handlers_completion_ranking_provider_test` both cases (the ranking mechanism is ported in `ranking.rs`) |
| `@Disabled` upstream (needs a real JDK) | 1 | `test_module_completion_resolve_shows_documentation` |

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

## CompletionHandlerTest completion (2026-10-07)

All 156 `CompletionHandlerTest` methods are ported: 150 pass, 6 are ignored (above).
Every newly ported active test passed on the oracle in targeted runs. A full oracle run
under heavy machine load showed timing failures in a few constructor/Javadoc cases that
pass when run alone; recheck on an idle machine.

* Inputs and assertions are upstream's. 25 tests use upstream's rtstubs test JDK,
  preferences are set before the server starts, and `makeConsistent`/`getAST(WAIT_YES)`
  become a wait for diagnostics. `dataFieldURI`, `dataFieldExecutionTime` and
  `selectSnippetItem` assert jdt.ls's internal response cache; their ports check the same
  behaviour through `completionItem/resolve` and `java.completion.onDidSelect`, so they
  are not exact ports.
* Getter/setter proposals keep relevance 1 (upstream computes but never sets one);
  anonymous-type bodies get upstream's `;` handling.
* The bridge installs a job-less JDT index manager so subtype search after `new ` no
  longer fails with an NPE (it finds nothing: constructors of implementing types are
  still not proposed), and reads `CompletionEngine.lookupEnvironment` directly, fixing
  diamond and constructor type arguments.
* `src/features/completion/import_context.rs` ports `ContextSensitiveImportRewriteContext`
  and `ScopeAnalyzer.getDeclarationsInScope` for completion resolve. The `java.lang`
  same-package conflict check only sees package types already fetched.

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

## Preferences integration evidence

All four `preferences` test classes are ported: 53 upstream methods, all passing.

* `ClientPreferencesTest`, `PreferencesTest` and `PreferenceManagerTest` call Java APIs
  directly, so they are unit ports with the upstream inputs and expected values. The
  Mockito Maven-configuration mock is replaced by a recording mock.
* Rust now has a typed `ClientPreferences` (the global `client_caps` helpers delegate to
  it), a typed `Preferences` with `create_from`/`update_from` and a full `MapFlattener`
  port, and a `StandardPreferenceManager` port (listeners, code templates, JavaCore tab
  options, Maven/Eclipse preferences, `javals.profile`, multi-module directory). The
  server keeps one global manager updated on `initialize` and `didChangeConfiguration`.
* `java.project.encoding: warning` publishes the "Project 'X' has no explicit encoding
  set" marker; explicit encodings come from the POM or `org.eclipse.core.resources.prefs`.
  The static Gradle importer falls back to downloading declared dependencies.
* `NullAnalysisTest` (6/6 on Rust and the oracle) observes upstream's marker counts over
  LSP, so two inputs differ from upstream: `testNullAnalysisDisabled` enables
  `java.project.encoding` (upstream's third marker is the encoding marker) and
  `testMissingNonNull` restarts the server after adding a folder, because jdt.ls only
  re-applies null-analysis options at startup.
* **Remaining:** the disable-test-classpath flag and multi-module directory update only
  the modelled m2e state, not the Maven classpath; completion snippets still read raw
  `java.templates.*` settings.

## Unresolved-method correction evidence

All 91 `UnresolvedMethodsQuickFixTest` methods are ported; 90 pass on Rust and all 91
pass on jdt.ls 1.58.0.

* `src/correction/unresolved_elements*` ports `UnresolvedElementsBaseSubProcessor` for
  `UndefinedMethod`, `UndefinedConstructor`, `UndefinedAnnotationMember`,
  `ParameterMismatch` and `NoMessageSendOnArrayType`: create method/constructor (other
  files, outer types, abstract variants), "Change to" renames, add/remove/swap/change
  arguments and parameters, argument casts, sender-type changes, qualification, missing
  cast parentheses, static-import favorites, the `new` keyword fix, array access and
  annotation members.
* The bridge exports receiver and created-type member graphs, well-known types and a
  cast-compatibility relation for units that need them.
* `validate_all_open_buffers_on_changes` now defaults to `true`, as upstream.
* `test_indirect_protected_method` passes since the modifier-correction port.
  `ConvertLoopOperation.modifyBaseName`, the "Let type
  implement interface"/"change constructor type" sender proposals and generated method
  comments are simplified or missing; no upstream test covers them.

## Type-mismatch correction evidence

All 44 `TypeMismatchQuickFixTest` methods are ported and pass on Rust and on jdt.ls 1.58.0.

* `src/correction/type_mismatch*` ports `TypeMismatchBaseSubProcessor`: add/change cast,
  Optional wrapping, method return type changes (with the `@return` tag), receiver and
  sender type changes, implement interface, constructor type, `!= null` checks,
  incompatible return types and throws clauses, and foreach variable types. JLS 5.5 cast
  compatibility is computed in Rust over the binding graph.
* The bridge exports primitive well-known types for every AST and bindings for
  annotation type members. The legacy bridge "Cast to" action is filtered out.
* Parameter mismatches reuse the sender-type proposals ("Let 'X' implement 'Y'",
  "Change 'X' to compatible type").
* The bridge resolves source types through their package fragment (folder), as JDT does,
  via `SourceLayout`, even when the declared package differs.
* The Java50Fix raw-type "Add type arguments" fix is ported with the InferTypeArguments
  solver (`src/correction/infer_type_arguments*`). Type sets are "any" or finite over seen
  types rather than JDT's symbolic subtype sets.
* **Remaining:** candidate subtypes for the constructor type proposal (upstream runs code
  completion); removing type annotations on type change; the "type arguments from
  context" and deprecated-field proposals sharing the raw-type dispatch entry.

## Modifier-correction evidence

All 42 `ModifierCorrectionsQuickFixTest` methods are ported; 40 pass on Rust and all 42
on jdt.ls 1.58.0. `setup()` sets `java.format.insertSpaces=false` (upstream's mocked
preference manager keeps JavaCore's tab default).

* `src/correction/modifier_corrections/` ports `ModifierChangeCorrectionProposalCore`
  (cross-unit lazy changes, `ModifierRewrite`, bodies for methods made static, abstract
  enclosing classes), `VariableDeclarationRewrite.rewriteModifiers`, invalid-modifier
  removal, every non-accessible-reference kind, overridden-method visibility/final/static,
  effectively-final locals, synchronized/static method modifiers, sealed types and
  permitted subtypes, "Add permitted type cases", `@Override` removal with "Create method
  in super type", and "Mark method as deprecated". Untested proposals were compared with
  the oracle.
* `modifier::keywords` now follows `AST.newModifiers` order.
* The semantic AST parses units and the sources they look up in their package fragment
  (folder), matching the Java model; this fixed a completion regression where the
  folder-based name environment and declared-package parse disagreed.

## Unresolved-variable correction evidence

All 48 `UnresolvedVariablesQuickFixTest` methods are ported and pass on Rust and on
jdt.ls 1.58.0.

* `src/correction/unresolved_elements/variables.rs` ports `collectVariableProposals`:
  new local/field/parameter/constant/enum constant, remove assignment, "Change to" similar
  variables and methods, array `length` and static-import favorites;
  `new_variable.rs` ports `NewVariableCorrectionProposalCore` (member-order placement).
  The variable proposals also run for non-accessible references.
* `UndefinedType` is dispatched to Rust for the similar-type "Change to 'X' (pkg)"
  proposals only. Creating types, import-only and qualify-type proposals, project setup
  fixes and the enhanced-for loop variable remain unported.
* The bridge sends qualifier-type members and compatibility data for unresolved field
  references; the old bridge "Create local variable/parameter/field/constant" actions are
  dropped. JDT's insert-after-previous-element placement is used by the new proposals
  only, not yet globally in the Rust `ASTRewrite`.

## Chain/lazy-resolve completion and extract refactoring groundwork (2026-10-07)

* `CompletionHandlerLazyResolveTest` (20/20) and `CompletionHandlerChainTest` (12/12)
  pass on Rust and the oracle. Chain completion: the bridge's `ChainCompletionService`
  (`chains` op) returns chain data; `src/features/completion/chain.rs` selects entry
  points, reads `recommenders.chain.*` project preferences and builds proposals. A
  completion NPE with on-demand imports was fixed in `CodeAssistEnvironment`.
* `src/features/completion/postfix.rs` holds an unwired port of the postfix template
  engine. To finish: call it from `handler.rs`, add a `StoredProposal::Postfix` resolve
  path (upstream re-adds import edits on resolve), and port `PostfixCompletionTest` (29).
* `src/refactoring/` holds unwired ports of `RefactoringStatus`, `SelectionAnalyzer`, AST
  fragments, `ScopeAnalyzer`, naming suggestions, `Checks`/side-effect checkers,
  `ExtractTempRefactoring` and `ExtractConstantRefactoring`. To finish: port the extract
  parts of `RefactorProposalUtility` into `correction::quick_assist::refactor_proposals`,
  retire the bridge's `makeExtract*Action`, un-ignore the three `ExtractVariableTest`
  cases, then port ExtractField/ExtractMethod (flow analysis) and `GetRefactorEditHandler`.
  Method side-effect lookup currently only sees the current file.

## Next steps

* Unresolved types: `NewCUProposal` and `AddTypeParameterProposalCore` are ported
  (`unresolved_elements/new_type.rs`). Still ignored: the 7 ambiguous-type tests (dispatch
  written, unproven), add-all-missing-imports, two annotation cases and the sealed case. The
  import-only proposal is written but disabled (`IMPORT_ONLY_ENABLED`) until the
  similar-type search skips test-source types for main code.
* Getter/setter: `src/correction/getter_setter.rs` has a self-encapsulate port; only
  `test_invisible_field_to_getter_setter_5` passes. The proposal is blocked because the
  bridge's field binding reports `is_from_source() == false`; check the declaring class
  instead, then verify the never-run `sef` path against the oracle.

Run `scripts/parity-count.py` for the per-class gap. The largest remaining items:

* Type quick fixes (`NewCUProposal`, add import / add-all-missing-imports, ambiguous
  types, type parameters) and getter/setter self-encapsulation: 33 ported tests wait on them.
* Refactoring (118 tests): extract variable/constant are wired (`quick_assist::refactor_proposals`); next ExtractField/ExtractMethod and the remaining classes.
* The remaining correction classes (null annotations, Javadoc, convert-to-record,
  reorg, lambda, ...), Gradle importer (45), `ProjectCommandTest` (28), m2e pom
  markers (WorkspaceDiagnostics, MavenBuildSupport), core utilities, syntax server and
  filesystem tests.
* Environment for running the suite in a Linux container: install JDK 8 (for
  `test_forbidden_reference`) and JDK 25, and set
  `JAVA_HOME` to it for tests (JDK 21 stays the default `java` for Gradle 8.5);
  `scripts/install-decompiler-from-oracle.sh` when the JetBrains repository is unreachable.
  Downloads honour `HTTPS_PROXY`/`NO_PROXY` and `SSL_CERT_FILE`. Rust does not yet fall
  back to `-javadoc.jar` downloads when no sources jar exists, as m2e does
  (`test_hover_with_attached_javadoc` passes only once that jar is cached).

## Known differences from jdt.ls

* **JDT build.** The bridge now uses Maven JDT/ECJ 3.46.0. Its Core build is
  `v20260520-1003`; the 1.58 oracle bundles `v20260409-1507`. Snippet-directive
  whitespace differs between them; Rust restores whitespace from the DOM source
  range, retaining the upstream hover assertion (both snippet cases oracle pass).
  Other build differences may surface as more cases are ported.
* **Resource filters:** the default filters, direct manager operations, saved-file
  builds and tested Java regex forms work. Java-specific Unicode inline flags/properties,
  string-encoded filter lists and arbitrary persisted `.project` matcher
  descriptors remain unported. The tested forms do not establish complete
  `java.util.regex.Pattern` compatibility.
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
* **`refactoring_extract_variable_test::test_extract_variable1`** fails identically on the oracle on this machine: attached JDK sources change the inferred parameter name (`index` instead of `i`).
* **Extension bundles.** jdt.ls loads OSGi bundles (`initializationOptions.bundles`,
  e.g. java-debug and the test runner) that contribute delegate commands. The Rust
  server can't host Eclipse plug-ins; its delegate-command registry
  (`features/execute_command.rs`) only takes Rust contributions.
* **Builder.** Saved-file problems come from a full rebuild of the affected projects,
  not JDT's incremental builder: duplicate types across files are reported by ECJ in
  build order rather than as the builder's duplicate class-file problems, and no class
  files are written to output folders.

## Saved work branches

The project-import WIP branch `worktree-agent-a1e31779b8b46b008` (`af6a7a8`)
is now integrated and verified. No saved WIP branch remains unmerged.

## Largest remaining work

| Work | Upstream tests | Share of suite |
|---|---:|---:|
| Remaining quick fixes and assists (`correction`) | 536 | 26% |
| Remaining completion (LazyResolve 20, Chain 12, Postfix 29, ranking/resolve/insert-replace 9) | 70 | 3% |
| Remaining project managers | 77 | 4% |
| Refactoring | 119 | 6% |
| Remaining handlers outside completion: code actions, generation, imports, save actions, markers and lifecycle/init | 206 | 10% |
| Core utilities, preferences, commands and the rest | 229 | 11% |

## Updating this file

Run `scripts/parity-count.py` (missing tests per class plus the totals and the area
table), `--all` to list every class, or `--markdown` for a table of ported classes.
It counts `#[test]`/`#[ignore` in `tests/<pkg>_<class_snake>.rs` and in `mod
<class_snake>` blocks under `src/` (unit ports), plus the few irregular unit ports
listed in its `EXTRA` table. Keep `tests/lsp.rs` and `tests/*_regressions.rs` for our own
regressions; they are never counted. Update this file whenever a branch is merged into
`jdtls-parity`.

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

At that point, the other 13 provider ports remained ignored until their actual
test-extension registration, complete log assertions and decompiler mapping
fixtures were verified. Raw mapping transport and Rust mapping conversion were
implemented, while the mapping test remained excluded from the passing count
until the follow-up batch below.
That batch reached 836 ported, 809 passing, 27 ignored and 1,251 unported.
`web-and-provider-full-suite-1.log` records the fresh full Rust run: 88 targets,
1,531 passes, zero failures and 28 ignores (including the optional Javadoc corpus).

The follow-up local web check also verifies applying a missing-import quick fix.
The primary page previously omitted code-action capabilities and stripped the
Java diagnostic source, code and data from its request, which produced no import
action. It now preserves the diagnostic and uses a valid selection range. The
adapter page enables its advanced providers and supplies only selected-marker
diagnostics. A popup stacking fix lets mouse clicks reach the action menu. Both
pages showed the then-current import label, applied the exact import edit, and cleared
the unresolved-type error. Keyboard and mouse flows each pass two browser tests
without console errors or failed requests (`import-browser-final-3.log` and
`import-browser-final-4.log`). These changes are in the local, gitignored clients;
they do not change the upstream-test ledger or require a Rust behavior change.

## Restored provider internals (2026-10-06)

All 21 `ContentProviderManagerTest` ports now call the actual manager APIs,
restoring 13 ignored tests and correcting the attached-source decompile port to
use `getSource` rather than substituting a `java/classFileContents` request.
The original fake extensions exercise exception fallback and its error log,
duplicate priorities and their error log, missing provider classes and their
info log, preferred providers with no errors, cancellation of the actual
monitor, identity of the injected preferences, and two requests on one manager
with different fake return values. The mapping test retains both non-null
arrays, both lengths of six, and the original first pair `[11, 12]`.

The Rust tests use the production manager and obtain real attached-source and
raw FernFlower facts from the embedded bridge. The bridge accepts the same
primitive debug line-dump option used by the upstream class, with separate
cache keys for normal and debug output. Provider policy and mapping conversion
remain in Rust.

A test-only fragment supplies the unchanged upstream `FakeContentProvider`,
the original four extension registrations, and a thin command adapter in an
isolated copy of the real Eclipse 1.58.0 product. The actual Eclipse manager,
registry, decompiler and platform logs run unchanged. The original oracle
installation is untouched, and the per-workspace product override does not
change other targets. The target shares one Eclipse runtime for the class,
preserving its actual source-discovery cache as the upstream JUnit class does.
It does not filter errors to satisfy the empty-error assertions. Mapping setup
waits for the reference project to be imported before resolving its class.
See [the fixture README](../tests/oracle/content-provider/README.md) for commands
and the exact adapter scope.

`content-provider-direct-oracle-4.log` verifies all 21 upstream cases with zero
failures. `content-provider-direct-rust-2.log` verifies all 21 Rust ports plus
three reused URI unit tests, and `content-provider-integration-rust-1.log`
verifies six public binary-editor integrations and four additional policy
regressions. The fresh full run, `content-provider-direct-full-suite-2.log`,
records 88 targets, 1,547 passes, zero failures and 15 ignores. Evidence lives in
`target/parity-evidence/`.

The ledger is now 836 ported, 822 passing, 14 ignored and 1,251 unported. Full
feature parity remains unfinished; the remaining ignored ports and unported
classes are still excluded from the passing count.

## Default projects and resource filters

`ProjectsManagerTest.testCreateDefaultProject` is restored with the original
empty-root initialization, project count, non-null handle, identity and existence
assertions. Two newly ported upstream methods, `testResourceFilters` and
`testInvalidResourceFilters`, retain the original `maven/salut` fixture, folder
handles, preference values and assertions. Rust materializes the default Java
project's metadata and source/output directories; source buffers remain virtual.

The Rust project model owns managed resource filters. They match complete names,
inherit through ancestors, include the upstream managed-filter sentinel, prune
filtered files from source discovery and compilation, and leave the default
project untouched. Invalid expressions are removed individually. The native
regex engine uses Java syntax with named captures/backreferences enabled and
Java's ASCII defaults for predefined classes. Actual Eclipse comparisons cover
quoted literals, character-class intersections, lookarounds, numeric/named
backreferences, possessive quantifiers, atomic groups, Unicode escapes, full-name
matching and inherited filters. Broader Java regex compatibility remains a gap.

Public settings updates retain the old list for null or missing values; an empty
array removes filters. This differs from the direct manager setter, where null
clears them. The saved-file regression verifies removal of old diagnostics,
preservation of unaffected errors, retention across null/unrelated updates, and
restoration after an empty list and full rebuild. A separate Rust lifecycle
regression verifies type errors and their correction in absent-file, `untitled:`
and `inmemory:` buffers with a filter that would exclude all project resources.
The oracle product doesn't publish diagnostics for that absent-file buffer;
virtual-buffer support remains an explicit Rust requirement, not an upstream port.

A thin test-only fragment calls the real `StandardProjectsManager`, `Preferences`
and `Resource.isFiltered` APIs. The shared fixture builder creates separate
`projects-manager` and `content-provider` products without changing the original
oracle. The Rust fixtures call the production model directly. No manager or
regex implementation is substituted in the Eclipse product.

Verification evidence in `target/parity-evidence/`:

* `project-filters-oracle-final-2.log`: all 10 active ProjectsManager upstream
  ports pass; the subsequent additional configuration regression exposed the
  need for a full rebuild after reintroducing filtered sources.
* `project-filter-regressions-oracle-final-3.log`: all 14 additional project
  regressions pass against Eclipse, excluding the 11 reused Rust unit tests.
* `oracle-fixture-builder-provider-final-1.log`: all 21 upstream provider-manager
  ports still pass against Eclipse after generalizing the fixture builder.
* `project-filters-full-suite-final-1.log`: 89 targets, 1,587 passes, zero failures
  and 14 ignores, including the optional external Javadoc corpus.

The ledger now has 838 upstream ports, 825 passing, 13 ignored and 1,249 unported.
Full feature parity remains unfinished, including the three ignored Gradle
manager ports and the unported cancellation/importer manager cases.


## JVM configuration and execution environments

All seven original `JVMConfiguratorTest` methods are ported with their original
assertions: default VM reuse, native runtime validation, library Javadoc,
absolute Javadoc-directory conversion, preview/compliance changes, both runtime
validation notification forms, and the single `java.lang.Object` symbol check.
The isolated oracle registers upstream's unchanged `TestVMType` and all its
Java 8 and Java 9–26 stub libraries. A thin adapter calls the actual JDT LS,
launching and project APIs; it does not replace configuration or VM policy.
The default/invisible-project listener test observes 21 → 26 → 12 → 21.

Rust owns runtime preferences, installation lookup, execution-environment and
named-VM selection, the default VM, and source/Javadoc attachment settings.
Public `java.configuration.runtimes` accepts nested and flat preferences;
missing keys retain the list and explicit null/empty lists clear it while
previously installed VMs remain registered. The first `default` key is the
only one considered, matching upstream even when that value is false. Runtime
validation is logged during initialization and sent to the client after a
configuration update, using actionable notifications when negotiated.

Project `java.home` and the selected runtimes are separate from the Java process
running the compiler bridge. VM changes update project libraries, default
compiler options and unmanaged-project preview settings. Referenced projects
contribute their dependencies and sources, not their JRE libraries. The Java
bridge reads the selected JDK image for compilation, type search and binary
navigation, with module descriptors and caches scoped to that image. Configured
JDK source attachments and Javadoc locations reach binary-editor content and
class-file URIs instead of being overwritten by the running JVM's defaults.
The filesystem factory and module-descriptor APIs were checked against the
[official Java filesystem documentation](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/java/nio/file/FileSystems.html)
and [module-descriptor documentation](https://docs.oracle.com/en/java/javase/25/docs/api/java.base/java/lang/module/ModuleDescriptor.html);
the pinned Eclipse implementation and oracle supply configuration semantics.

Five additional public LSP regressions verify separate project execution
environments, runtime changes and continued compiler availability, initialization
versus update notifications, native source/Javadoc attachments, and type errors
plus import fixes in absent-file, `untitled:` and `inmemory:` buffers. All four
resource-backed cases also pass against Eclipse. The virtual-buffer case is an
explicit Rust requirement and is excluded from oracle runs. The runtime-change
case includes upstream's empty fake Java executable: project-VM configuration
must not use it to launch the compiler bridge.

The existing fallback tests now isolate their compiler socket and use an
unavailable bridge instead of relying on startup timing. All original assertions
remain. The range-formatting case still uses the real compiler bridge. The
server releases its configuration lock before waiting for bridge startup, so
fallback requests and settings updates remain responsive. The LSP regression
target remains 95 tests.

Evidence in `target/parity-evidence/`:

* `jvm-configurator-oracle-final-3.log`: 7/7 original upstream methods pass;
  the 11 reused Rust project unit tests are excluded from this oracle command.
* `jvm-wire-oracle-final-2.log`: 4/4 additional resource-backed settings cases.
* `jvm-wire-rust-final-3.log`: all five additional cases pass, including virtual
  buffers and the two-project execution-environment selection.
* `jvm-lsp-regressions-final-3.log`: all 95 existing LSP regressions pass with
  deterministic fallback coverage.
* `jvm-builder-provider-oracle-final-1.log` and
  `jvm-builder-projects-oracle-final-1.log`: original isolated provider and
  project-manager products still pass after extending the fixture builder.
* `jvm-full-suite-final-2.log`: 91 targets, 1,610 passes, zero failures and
  14 ignores, including the optional external Javadoc corpus.

After the JVM batch, the ledger was 845 ports, 832 passing, 13 ignored and
1,242 unported.
This batch does not prove complete VM-platform parity: contributed/native VM
extension discovery, pre-release-file JDK metadata, all legacy installation
layouts and strict execution-environment access rules need broader comparisons.
Full feature parity remains unfinished, including the ignored build-tool cases
and the unported preference, refactoring, cleanup and syntax-server suites.

## Source-path commands

All four original `BuildPathCommandTest` methods are ported, preserving the
original Eclipse, Maven, Gradle and standalone-folder fixtures, source-path
counts and display paths. Both add/remove commands now execute the Rust port
of `BuildPathCommand` and `ProjectUtils` policy. They choose the deepest Java
project location, reject build-tool-owned classpaths with the original Maven
and Gradle messages, and create a linked invisible project when needed.

Adding an existing source path and removing an absent one are successful
no-ops. A source folder beneath an existing source folder is rejected; adding
a parent excludes its existing children and visible subprojects. Removing a
child clears only the corresponding inclusion/exclusion patterns, preserving
other patterns, per-source outputs, extra attributes and library access rules.
Changed invisible-project results contain workspace-relative `sourcePaths`;
visible-project and no-op results omit that field as Eclipse does.

Raw classpath changes are persisted atomically. Updates retain untouched XML,
including comments and metadata belonging to other tools. Invisible project
metadata stays in the server workspace, with the user folder linked as `_`;
manual source paths survive restart. A trigger-based importer reapplies its
source/output preferences on initialization, including linked output paths.
Standalone opens also materialize the default project's metadata, and commands
against that project retain one project handle. Source files and nonexistent
source directories are never created by these operations.

Nine additional LSP regressions cover the exact response objects, no-ops,
outside-workspace errors, nested exclusions, empty/root classpaths, classpath
metadata, visible subprojects, default projects and restart behavior. Eight
resource-backed regressions are verified against Eclipse; the ninth checks the
Rust requirement that unsaved, absent-file buffers retain type diagnostics
through source-path changes without creating source files.

The unchanged Gradle fixture uses Gradle 8.5, which supports running on Java 21
([official release notes](https://docs.gradle.org/8.5/release-notes.html)). Its
oracle run uses a checksum-verified Temurin 21 JDK in `.oracle/jdks/`; the wrapper,
repositories and dependency declarations are unchanged. Run the two targets
against the reference server with a compatible Java 21 installation:

```sh
JAVA_HOME=/path/to/jdk21 JDTLS_ORACLE=1 CARGO_INCREMENTAL=0 cargo test \
  --test commands_build_path_command_test --test build_path_regressions \
  -- --test-threads=1
```

Evidence in `target/parity-evidence/`:

* `build-path-focused-rust-6.log`: 4/4 original ports and all nine regressions.
* `build-path-focused-oracle-6.log`: 4/4 original ports and all eight
  resource-backed regressions; virtual-buffer case explicitly excluded.
* `build-path-full-suite-1.log`: 93 targets, 1,623 passes, zero failures and
  14 ignores, including the optional external Javadoc corpus.
* `build-path-existing-oracle-2.log`: all 29 existing diagnostics-command,
  lifecycle, multi-root and invisible source/output preference cases pass.

At the source-path commit, the ledger was 849 ports, 836 passing, 13 ignored
and 1,238 unported. Gradle model/update parity remains unfinished; the follow-up
restoration below covers the two runtime-dependent project-manager cases.

## Restored Gradle project-manager assertions

`ProjectsManagerTest.testSendingOKProjectStatus` and
`testReloadGradleProjectMarker` now run with their original fixtures, commands,
notification counts/status and marker assertions. Their earlier oracle failure
came from running Gradle 8.5 on Java 25. The unchanged fixtures and assertions
pass on Java 21, so both ignores are removed. Run the project-manager oracle
target with `JAVA_HOME` pointing to a compatible Java 21 installation, as for
the source-path targets above.

All 12 active upstream project-manager methods pass against both Rust and
Eclipse; the target also repeats 11 Rust project unit tests. Evidence:

* `gradle-ignored-java21-oracle-probe-1.log` and
  `gradle-ignored-rust-probe-1.log`: both restored cases pass unchanged.
* `gradle-restored-projects-rust-1.log` and
  `gradle-restored-projects-oracle-1.log`: 23 passes, zero failures and one
  remaining ignore per target (12 upstream methods plus 11 Rust unit tests).
* `build-path-full-suite-final-2.log`: 93 targets, 1,625 passes, zero failures
  and 12 ignores after restoring the two Gradle tests.

`testSendingWarningProjectStatus` remains ignored and unfinished. Its original
invalid Gradle build references a Java configuration without applying the Java
plugin. Rust currently reports `OK` instead of `WARNING`. The public Eclipse
probe observed only the initial update message, so verifying the original
direct-manager assertion also requires the proper Gradle/job setup. This is
recorded as missing model parity, not a passing test or a complete Gradle port.

At the Gradle restoration, the ledger was 849 ports, 838 passing, 11 ignored and 1,238 unported.
Full feature parity remains unfinished.


## Organize-imports command and shared Rust operation

All 12 original `OrganizeImportsCommandTest` methods are ported with unchanged
source strings, imported projects, expected text, edit-range assertions and
settings. The new `java.edit.organizeImports` handler returns
`WorkspaceEdit.changes` for clients without `workspace.applyEdit`; clients that
advertise it receive the same edit through `workspace/applyEdit`, and the
command returns `{}`. Empty edits follow that branch too. Client acknowledgement
does not mutate the server buffer or disk.

The command and source action now use the Rust reference collector, scoped type
search and `ImportRewrite` previously used by paste. Organize imports rebuilds
the import list; paste retains existing imports. This removes the source
assistant's dependence on the approximate Java organizer. Rust owns unused-import
removal, sorting, normal/static wildcard thresholds, filtered type selection,
noninteractive ambiguity handling, module descriptors, lexical scope, Javadoc
and static favorite selection. Compiler `ImportNotFound` facts preserve existing
unresolvable imports only for references that still need them, preferring exact
imports over wildcard matches.

The pinned public Eclipse command has a directory-routing quirk:
`getFileForLocation` returns an `IFile` handle for a descendant directory, so the
command tries to organize it as a file and produces no edits. The original
package test invokes `organizeImportsInPackageFragment` directly instead. Its
Rust port uses the production package collector followed by the same per-unit
wire operation, preserving both original output assertions. The collector keeps
Eclipse's substring match for package names. A separate wire regression verifies
the directory command's no-op behavior, including source roots and packages.
Project commands collect all source roots and tests, excluding non-source and
unchanged units.

Eighteen additional regressions cover negotiated/rejected client edits, raw and
invalid arguments, ambiguity without a chooser, filters, unresolved normal/static
imports, wildcard expansion, recovered syntax, deferred source-action edits,
project scopes, CRLF/Unicode comments and unsaved text. Sixteen resource-backed
cases pass against Eclipse; the direct Rust package-collector regression and the
virtual-buffer requirement are recorded separately. `untitled:`, `inmemory:` and
absent-file buffers organize without creating source files.

Type listings now read each selected VM's module image rather than treating
`jrt-fs.jar` as an ordinary archive or adding the running VM's classes. Module
names and the `SourceFile` attribute remain compiler facts. Binary symbol
locations reuse the existing Rust URI builder, retaining the module, original
source filename and classpath attributes while the handle names the class file.
The same indexed source-filename facts now reach binary implementation links.
Per-project symbol searches omit referenced projects' runtime libraries and
avoid adding a second running-VM copy. The runtime regression verifies Java
21/25 import-search isolation and symbol uniqueness when Java 21 is available
through `JAVA21_HOME` or the local `.oracle/jdks/` installation; the native-image
assertions always run. The JVM notification regression waits for its
asynchronous warning before checking the unchanged notification count and payload; a request round trip
does not guarantee notification completion.

Evidence in `target/parity-evidence/`:

* `organize-command-oracle-3.log`: all 12 original methods, including the direct
  package collector with per-CU Eclipse edits.
* `organize-regressions-oracle-4.log`: 16 resource-backed comparisons; direct
  collector and virtual-buffer cases are distinct from those comparisons.
* `organize-runtime-symbol-rust-final-5.log`: 19 existing symbol ports, all five
  JVM configuration regressions and all 18 import regressions.
* `organize-final-oracle-7.log`: all 12 command ports, the 16 resource-backed
  import comparisons and all 19 existing symbol methods pass with the final
  module/source-filename metadata. Collector and virtual cases remain separate.
* `organize-runtime-image-oracle-final-5.log`: selected Java 21/25 imports and
  symbols, original source filenames, module names and build-path attributes.
* `organize-runtime-notification-oracle-final-1.log`: unchanged JVM notification
  assertions with an explicit asynchronous-notification wait.
* `organize-existing-oracle-final-1.log`: existing source-action and paste targets
  pass (3, 22 and 9 tests; Rust-only virtual paste remains explicitly excluded).
* `organize-full-suite-final-3.log`: 95 targets, 1,655 passes, zero failures and
  12 ignores, including the optional external Javadoc corpus.

That batch brought the ledger to 861 ports, 850 passing, 11 ignored and 1,226
unported. Interactive import-choice callbacks and the advanced handler/source
assistant suites were still unfinished at that point; the following batch
addresses those areas. Save-action/cleanup integration remains unfinished.


## Interactive import selection and standalone quick fixes

`java/organizeImports` now uses the Rust import operation and negotiates
`workspace/executeClientCommand` with command
`java.action.organizeImports.chooseImports`. The client receives the document
URI, candidate groups with UTF-16 source ranges and the restore-existing-imports
flag. Candidate identities are opaque tokens scoped to the operation. Rust maps
returned IDs to its own candidates rather than trusting returned type names;
null entries and unknown IDs are ignored. A null reply or missing client-command
support cancels the entire ambiguous operation, including otherwise valid
removals and unique imports. An empty selection allows those unambiguous edits.
Operations without ambiguity do not prompt or require client-command support.

Source actions enable the chooser only when `advancedOrganizeImportsSupport` is
negotiated, including deferred `codeAction/resolve`. Eclipse's source assistant
passes a Java-rendered resource location URI (`file:/...`), while the direct
handler forwards the client's document URI; both forms are preserved, including
Unicode filenames and escaped spaces. Workspace-edit resource URIs are compared
as URLs in the Unicode regression because their equivalent serialized forms can
differ. Source actions now offer **Add all missing imports** with kind `source`
when the DOM reports an undefined type (including Javadoc). That action retains
existing imports and passes `restoreExistingImports: true`; organize imports
rebuilds the list. The edit command and paste retain their noninteractive
behavior.

All five methods of `AdvancedOrganizeImportsHandlerTest`, all six methods of
`OrganizeImportsActionTest` and both methods of `NonProjectFixTest` are ported
with their original sources and assertions. The advanced handler's direct Java
callback is exercised through the public handler and dynamic client replies;
its candidate names, order, ranges and resulting source assertions are retained.
Its Maven/static-import cases use unchanged `salut4` and `salut6` fixtures.
The `MavenBuildSupport.applies` assertions use the same Maven-project nature
predicate exposed through project settings, including project existence.

The standalone-file diagnostic is expected Eclipse behavior: code `16`, severity
`2`, warning that only JDK classes are available. Full-validation mode still
reports type mismatches and unresolved JDK types. The standalone regression
checks the warning alongside `String`-to-`int` and missing-`ArrayList` errors,
applies the import fix and corrects the assignment in the client buffer, then
verifies only the warning remains and disk content is unchanged. Import-fix
labels now use Eclipse's **Import 'ArrayList' (java.util)** wording, shaped in
Rust from the legacy bridge response. The broader unresolved-type quick-fix
processor remains unported; this batch does not claim its full proposal parity.

The first advanced Maven run exposed a cold-cache defect: JARs were downloaded
without fetching the missing POMs needed to discover transitive dependencies.
The Rust resolver now fetches dependency, parent and imported-BOM POMs before
loading their models, respecting offline settings. Two local HTTP/offline
regressions verify all four POM requests, inherited dependency management,
transitive versions, cache reuse after the repository server stops and direct
dependency retention when offline. They are Rust regressions rather than
upstream ports.

Fourteen additional import regressions cover cancellation, empty replies,
identity validation, capability negotiation, deferred resolution, restoration,
multiple ambiguities, CRLF/UTF-16 ranges, Java URI rendering, no-op behavior,
standalone diagnostics and virtual buffers. Thirteen resource-backed cases pass
against Eclipse. The `untitled:`, `inmemory:` and absent-file case is a separate
Rust requirement and is explicitly excluded from Oracle comparisons. The test
client's dynamic request handlers retain its ability to move shared workspaces
between test threads.

Evidence in `target/parity-evidence/`:

* `import-choice-oracle-final-10.log`: all 13 original methods and 13
  resource-backed comparisons pass; the virtual-buffer case is recorded
  separately from those comparisons.
* `import-choice-rust-9.log`: all 14 import regressions and five existing JVM
  configuration regressions pass with the corrected import-fix wording.
* `import-choice-rust-7.log`: all 13 original methods, the two Maven regressions
  and existing project-manager regressions pass.
* `import-choice-maven-natures-oracle-final-11.log`: all five advanced handler
  methods pass with the original Maven build-support predicate assertions.
* `import-choice-full-suite-final-3.log`: 99 targets, 1,684 passed, zero failures
  and 12 ignored, including the optional external Javadoc corpus.
* `import-choice-browser-final-2.log`: both `/` and `/index2.html` show the type
  error, display and apply **Import 'ArrayList' (java.util)**, clear the
  missing-type error and clear all errors after correcting the assignment.
  No console errors or failed requests. Screenshots are
  `import-choice-web-{main,adapter}-{menu,fixed}.png`. The first browser script's
  exact typed-whitespace assertion timed out because Monaco auto-indented the
  keyboard input; the final assertions verify the observed diagnostic and edit
  flow without assuming the editor's indentation policy.

The ledger is now 874 ports, 863 passing, 11 ignored and 1,213 unported.

Save actions, cleanup integration and the remaining unported tests still require
work before full Eclipse feature parity can be claimed.

## Save actions and initial cleanup integration

`textDocument/willSaveWaitUntil` now delegates to the Rust SaveActionHandler
port. Import organization uses the shared noninteractive operation, honors
`java.saveActions.organizeImports` and the project save-participant import
setting, and returns edits without requesting client application. The manual
`java/cleanup` request returns `WorkspaceEdit.changes`, including an empty edit
array when no cleanup changes the document.

Cleanup selection honors `java.saveActions.cleanup`, `java.cleanup.actions`
and its deprecated `actionsOnSave` fallback. The exact client capability is
`canUseInternalSettings`: when enabled, the project `org.eclipse.jdt.ui` save
participant preferences replace the LSP cleanup list. They are not merged.
Disabled participant settings and disabled `sp_` keys are excluded, duplicate
IDs are removed, and `renameFileToType` remains the separate lifecycle action.
List updates select modern/deprecated IDs from the current notification, as
`Preferences.updateFrom` does. Clearing the modern list cannot revive an old
deprecated setting. The lifecycle rename action reads that same effective list.

The initial registry implements `invertEquals` (and `cleanup.invert_equals`)
and `organizeImports`. Rust selects invocations from their resolved signatures,
recognizes safe non-null arguments, and computes DOM rewrites. The bridge adds
only the compiler fact that an expression has a constant value. The invert
finder preserves Eclipse's treatment of enum constants, String concatenation,
`this`, primitive arguments, overloaded methods and parenthesized receivers.
Selected invocations suppress recursive child visits, as in the original finder.
Sequential cleanups reparse an isolated working copy and return one replacement
of the original document, so their edits do not overlap. Editor state and disk
content remain unchanged until the client applies the result.

All four original `SaveActionHandlerTest` methods are ported: unused imports,
favorite static imports, a missing formatter profile and the LSP/project cleanup
conflict. The malformed source in the conflict case is preserved. Three original
`CleanUpsTest` methods preserve the sources, trailing spaces, recovered syntax
and exact edit-result assertions. The Maven setup uses the unchanged Java 22
stub API and the existing isolated oracle TestVMType extension to avoid searching
the machine's JDK alongside the test VM. No original assertion was weakened.

Fourteen regressions cover enable/disable and cleanup-list updates, manual cleanup independent of
save settings, project preference precedence, disabled internal preferences,
deprecated settings, cleanup composition and alias deduplication, unknown and
rename IDs, noninteractive save import resolution, CRLF/UTF-16 edit ranges,
resolved equality signatures, exact enum-expression constant recognition and
editor-only documents. Thirteen resource-backed
cases pass against Eclipse; the `untitled:`, `inmemory:` and absent-file case is
explicitly excluded from Oracle comparisons.

Evidence in `target/parity-evidence/`:

* `save-actions-rust-final-7.log`: seven original methods and all fourteen
  regressions pass.
* `save-actions-oracle-final-4.log`: all seven original methods and thirteen
  resource-backed regressions pass; virtual documents are checked only in Rust.
* `save-actions-browser-final-1.log`: both `/` and `/index2.html` show the
  String-to-int error, display and apply **Import 'ArrayList' (java.util)**,
  clear the missing-import error and clear all errors after correcting the
  assignment. Console errors and failed requests: zero. Screenshots are
  `save-actions-web-{main,adapter}-{menu,fixed}.png`.

Fifteen original cleanup methods and the other registered cleanup operations
remain unported. As upstream does, the save handler appends separate organize
and cleanup results against the original working copy; enabling both can return
overlapping edits if both operations change the same document. Cleanup registry
composition itself returns one combined edit. Full Eclipse feature parity is
still unfinished.
