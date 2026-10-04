//! Port of `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceExecuteCommandHandlerTest`.
//!
//! Most upstream cases call delegate command handlers that the jdt.ls test
//! plug-in contributes (`testcommand1`, `dup`, ...) or load OSGi extension
//! bundles; those aren't part of the server.

mod common;
use common::jdtls::*;
use serde_json::json;


#[test]
fn test_execute_command_nonexisting_command() {
    let mut ws = Workspace::new();
    let resp = ws.client().request_response(
        "workspace/executeCommand",
        json!({ "command": "testcommand.not.existing", "arguments": ["hello", "world"] }),
    );
    let ex = &resp["error"];
    assert_eq!("No delegateCommandHandler for testcommand.not.existing", ex["message"], "{resp}");
    // ResponseErrorCode.MethodNotFound
    assert_eq!(-32601, ex["code"]);
}





// Upstream cases not ported here (no empty tests counted as ports):
// test_execute_command: testcommand1/testcommand2 are delegate command handlers contributed by the jdt.ls test plug-in, not by the server.
// test_execute_command_morethan_one_command: the duplicate `dup` command is contributed twice by the jdt.ls test plug-in, not by the server.
// test_execute_command_throws_exception_command: testcommand.throwexception is a delegate command handler of the jdt.ls test plug-in, not of the server.
// test_execute_command_invalid_parameters: over LSP, null params make JDTLanguageServer.executeCommand throw a NullPointerException (Internal error) before the handler's own check runs.
// test_registry_event_listener: loads OSGi extension bundles (jdt.ls.extension-0.0.1.jar) into the Equinox runtime; the Rust server has no bundle support.
