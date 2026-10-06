# Testing the local Java web editor

The local `web/` demo connects Monaco to the Rust server through a WebSocket.
It needs Rust, Maven, Java 21 or newer, and Node.js. The demo files in `web/`
and `ui/` are currently local, gitignored files; these instructions refer to
the existing workspace.

From the repository root:

```sh
CARGO_INCREMENTAL=0 cargo build
npm --prefix web install
npm --prefix web start
```

Leave that terminal running and open <http://localhost:3010/>. The adapter-based
editor is at <http://localhost:3010/index2.html>. After changing the Rust server,
build it again and reload the page; each connection starts a new server process.
After changing the browser scripts, use a hard reload to load the new scripts.

Check the following in either editor:

1. Enter `int x = "this is not an int";` inside a method. The string should have a
   red squiggle with `Type mismatch: cannot convert from String to int`.
2. Replace the string with `42`. The type error should disappear without saving.
3. Remove the `java.util.List` and `java.util.ArrayList` import lines while keeping
   their uses. Both type names should have red squiggles saying they cannot be
   resolved to a type. Restore the imports to clear those errors.
4. Hover over `items` after restoring the imports. Its Java signature should be
   shown as code, with source information below it.
5. Remove only the `java.util.ArrayList` import, place the cursor on `ArrayList`,
   and click **Quick Fix…**. Choose **Import 'ArrayList' (java.util)**. The import
   should be inserted and the error should disappear. Mouse clicks and keyboard
   selection both work in the action menu.

These demos use an unsaved non-project file. Eclipse JDT LS defaults to reporting
only syntax errors for such files. The clients explicitly call
`java.project.refreshDiagnostics` with `syntaxOnly: false` before opening their
buffers, and repeat that setup when connecting to a new server process. The
primary page can still show the informational non-project warning.

On 2026-10-06, Playwright exercised both routes against the real Rust server:
type errors and missing imports appeared, corrections cleared the errors,
hover signatures rendered correctly, and validation persisted after reload.
There were no console errors or failed requests. Local evidence is in
`target/parity-evidence/web-java-browser-2.log`, with screenshots named
`web-java-main.png`, `web-java-adapter.png`, and `web-java-imports-*.png`.

The import quick-fix flow was also verified on both routes. The primary client
now advertises code-action support and sends the original diagnostic (including
its Java source, problem code and data) with a valid selection range. The adapter
page enables its code-action providers and filters diagnostics to the selected
markers. Both pages position the action widget so Monaco's stacking order keeps
the clickable menu above its pointer guard. Selecting the import applies the
exact edit and clears the diagnostic. Evidence is in
`target/parity-evidence/import-browser-final-3.log` (keyboard),
`import-browser-final-4.log` (mouse), and `import-quickfix-*.png`.

The latest label and standalone-file flow were verified on both routes in
`target/parity-evidence/import-choice-browser-final-2.log`: the type mismatch
appeared, clicking **Import 'ArrayList' (java.util)** inserted the import and
cleared the missing-type error, and correcting the assignment cleared all errors.
There were no console errors or failed requests. Screenshots are
`import-choice-web-{main,adapter}-{menu,fixed}.png` in the same evidence directory.
The standalone warning remains expected when full validation is enabled.
