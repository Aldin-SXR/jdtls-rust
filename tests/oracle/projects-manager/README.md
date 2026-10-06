# Project-manager oracle fixture

This test-only OSGI fragment attaches to the real Eclipse JDT LS 1.58.0 core.
`ProjectsManagerTestCommand` calls the actual `StandardProjectsManager`,
`Preferences` and Eclipse resource APIs. It contains no replacement manager,
resource-filter implementation or regex engine.

Empty-root initialization uses `initializeProjects(Collections.emptyList(),
monitor)`, then reads the actual workspace projects and their identity against
`ProjectsManager.getDefaultProject()`. The Rust side calls the production Rust
workspace model and creates its default project's metadata and directories.

Resource-filter tests use upstream's original `maven/salut` project. Unlike
upstream's direct imports, LSP initialization has already configured filters.
The adapter first clears those filters through the real manager, restoring the
upstream starting state before performing the original preference changes and
assertions. It restores the original filters afterward. Filtering is queried
through the actual internal `Resource.isFiltered()` API, including nonexistent
folder handles as upstream does. Jobs are awaited after configuration changes.

From the repository root:

```sh
CARGO_INCREMENTAL=0 cargo test --test managers_projects_manager_test
JDTLS_ORACLE=1 CARGO_INCREMENTAL=0 cargo test --test managers_projects_manager_test -- --test-threads=1 --skip project::
JDTLS_ORACLE=1 CARGO_INCREMENTAL=0 cargo test --test projects_manager_regressions -- --test-threads=1 --skip project::
```

Oracle mode automatically builds an isolated product using
`scripts/prepare-oracle-fixture.py projects-manager`. It compiles only this
fragment, symlinks the original plugins and copies configurations into
`target/projects-manager-oracle`; it does not change the original oracle.
Eleven reused Rust project unit tests are excluded from the oracle commands and
from upstream-port counts. The additional regression target compares regex
matching against Eclipse and exercises the public LSP configuration and
diagnostic flows. Missing-file, `untitled:` and `inmemory:` buffer coverage lives
in `tests/lifecycle_regressions.rs` and runs against Rust: the Eclipse product
does not publish diagnostics for the absent file used by that test, even with
full validation requested. These additional regressions aren't upstream ports.
