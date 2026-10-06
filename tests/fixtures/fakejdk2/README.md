`21a` is copied verbatim from upstream's `org.eclipse.jdt.ls.tests/fakejdk2/21a`.
It has the Java 21 release metadata and runtime-library jar used by the original
`JVMConfiguratorTest`. Its `bin/java` file is intentionally empty: this fixture
is for VM installation validation and metadata, not compiler execution.

The upstream `doc` and `modules` directories contain marker files, which are
also copied. The Rust fixture helper and isolated oracle builder ensure the
directories exist before validation and Javadoc lookup.
