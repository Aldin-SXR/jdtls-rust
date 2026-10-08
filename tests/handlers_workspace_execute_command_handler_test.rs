//! Port of `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceExecuteCommandHandlerTest`.
//!
//! The cases that call the delegate command handlers the jdt.ls test plug-in
//! contributes (`testcommand1`, `dup`, ...) are unit tests in
//! `src/features/execute_command.rs`; `testRegistryEventListener` loads OSGi
//! extension bundles, which the Rust server cannot host.

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

// The other cases call the delegate command handlers of the jdt.ls test
// plug-in; they are unit tests of the registry in
// `src/features/execute_command.rs`.
