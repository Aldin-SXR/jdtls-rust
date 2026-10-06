# Content-provider oracle fixture

This test-only OSGI fragment attaches to the real Eclipse JDT LS 1.58.0 core.
It supplies the four content-provider registrations from the upstream tests'
`plugin.xml`, and an unchanged copy of `FakeContentProvider.java` (EPL-2.0).
`PlaceHolder` intentionally refers to a missing class, as it does upstream.

`ContentProviderTestCommand` calls the real `ContentProviderManager` APIs and
captures platform logs, monitor cancellation, preference identity and line
mappings. Each command constructs one manager and can make successive calls on
that instance. It contains no replacement implementation of provider policy or
decompilation. The oracle fixture sets `jdt.ls.debug=true`, matching upstream.

The Rust fixture calls the production Rust manager with equivalent fake
extensions. Its default providers obtain attached source, raw decompiled text
and line pairs from the embedded bridge. Rust owns selection and mapping
conversion. Debug line dumping is a primitive FernFlower input, and its cache
key includes the option so debug and normal output cannot share cache entries.

From the repository root:

```sh
CARGO_INCREMENTAL=0 cargo test --test managers_content_provider_manager_test
JDTLS_ORACLE=1 CARGO_INCREMENTAL=0 cargo test --test managers_content_provider_manager_test -- --test-threads=1 --skip fixture::classfile::tests
```

Oracle mode automatically runs `scripts/prepare-oracle-fixture.py content-provider`.
The script compiles only the test fragment, symlinks the original oracle
plugins into `target/content-provider-oracle`, and adds the fragment to copied
configurations. It sets the isolated installation area explicitly because the
launcher is a symlink. The harness selects that product for this workspace only;
the original oracle installation and other targets remain unchanged.

The three `fixture::classfile::tests` are reused Rust URI unit tests, excluded
from the Eclipse command above and from the upstream-port count. The 21 named
upstream tests all execute actual manager calls in both modes.

The oracle target shares one Eclipse runtime and workspace for the class, just
as JUnit does upstream. This retains Eclipse's actual source-discovery cache
between tests: otherwise copying the test JDK into a new workspace for each
method repeatedly sends its SHA to Maven Central, and network timeouts pollute
the original empty-error assertions. No logs are filtered out. Managers,
preferences, monitors and fake return values are reset for each test command.
An exit callback drops the shared workspace and kills the test server. The
mapping fixture waits for Eclipse's asynchronous folder import to resolve its
class before exercising `getSourceResult`.
