The `1.8/rtstubs.jar` and `9/rtstubs.jar` through `26/rtstubs.jar` files are copied
verbatim from the corresponding directories in
`eclipse.jdt.ls/org.eclipse.jdt.ls.tests/fakejdk/`.

Upstream's TestVMType provides these libraries without source attachments.
`Workspace::use_upstream_test_jdk` chooses the version named by an Eclipse
fixture's JRE container and replaces that container with the exact stub library.
Code-lens, call-hierarchy, symbol, completion, hover, reference and implementation
tests use it. Maven class-file tests add the Java 8 library through a retained
raw classpath entry, preserving their other dependencies; the oracle's URI
resolver chooses the fixture root when it also reports the host JDK.

PasteEventHandlerTest uses the Java 21 stub library for type-search candidates.
These fixtures preserve upstream counts, locations and edits without filtering
feature results or changing expected assertions.

`JVMConfiguratorTest` registers the unchanged upstream `TestVMType` in an
isolated Eclipse product. Its fake VM installations cover every copied version;
the Rust fixture supplies the same installation IDs, locations, versions and
library facts to the production runtime registry. These archives are API stubs,
not executable JDKs.
