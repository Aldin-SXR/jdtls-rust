# JVM configuration oracle fixture

This test-only fragment calls the actual Eclipse JDT LS 1.58.0
`JVMConfigurator`, `RuntimeEnvironment`, `StandardVMType`, `JavaRuntime` and
execution-environment APIs. `TestVMType.java` is the unchanged upstream test VM
extension. The small plugin helper supplies upstream's bundle identifier so its
original `FileLocator` lookup resolves the copied fixture directories.

The adapter sets up the upstream default Java 21 VM, creates the default Java
project through the actual project manager, observes VM selection, library
attachments and environment defaults, and waits for Eclipse background jobs.
Preview options are read after the actual VM-install-change listener runs.
Validation notifications travel through the real language-server connection.
Projects are copied and imported before VM setup starts the server; the adapter
does not substitute VM policy or project-management code.

The builder installs an unpacked fragment in `target/jvm-configuration-oracle`
and copies `fakejdk` and `fakejdk2` verbatim. Other plugins are symlinks to the
original product. The original `.oracle/jdtls-1.58.0` installation is untouched.

```sh
CARGO_INCREMENTAL=0 cargo test --test jvm_configurator_test
JDTLS_ORACLE=1 CARGO_INCREMENTAL=0 cargo test --test jvm_configurator_test -- --test-threads=1 --skip project::
CARGO_INCREMENTAL=0 cargo test --test jvm_configuration_regressions
JDTLS_ORACLE=1 CARGO_INCREMENTAL=0 cargo test --test jvm_configuration_regressions -- --test-threads=1 --skip selected_native_runtime_compiles_and_offers_imports_in_virtual_documents
```

The configurator target also compiles eleven existing Rust project-module unit
tests; those are not upstream JVM ports. The public settings regressions use the
installed native JDK for compilation, source-attachment and runtime-selection
checks. The absent-file, `untitled:` and `inmemory:` case is a Rust requirement;
it is excluded from oracle runs because Eclipse requires resource-backed units.
