`21/rtstubs.jar` is copied verbatim from
`eclipse.jdt.ls/org.eclipse.jdt.ls.tests/fakejdk/21/rtstubs.jar`.
The upstream test plugin selects this VM by default. PasteEventHandlerTest uses
its type-search candidates through a library classpath entry, replacing the
real JRE container. This preserves upstream's unambiguous java.util.List search
without adding a filter or changing the expected import edits.
