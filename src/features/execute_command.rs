//! Port of jdt.ls `WorkspaceExecuteCommandHandler`: dispatches
//! `workspace/executeCommand` requests to the registered delegate command
//! handlers (`org.eclipse.jdt.ls.core.delegateCommandHandler` contributions).
//!
//! The server's own `java.*` commands are matched in `server.rs`; commands
//! it doesn't know are dispatched here.

// Contributions are registered by embedders and tests; the server itself
// registers none yet.
#![allow(dead_code)]

use std::sync::{Arc, OnceLock, RwLock};

use serde_json::Value;
use tower_lsp::jsonrpc::{Error, ErrorCode};
use tower_lsp::lsp_types::ExecuteCommandParams;

/// `IDelegateCommandHandler`.
pub trait DelegateCommandHandler: Send + Sync {
    /// `executeCommand(commandId, arguments, monitor)`; `Err` is a thrown
    /// exception.
    fn execute_command(&self, command_id: &str, arguments: &[Value]) -> Result<Value, CommandError>;
}

/// An exception thrown by a delegate command handler.
#[derive(Debug)]
pub enum CommandError {
    /// A `ResponseErrorException`, rethrown as is.
    Response(Error),
    /// Any other exception, with its `getMessage()`.
    Exception(Option<String>),
}

/// `DelegateCommandHandlerDescriptor`: the commands a contribution handles
/// and its handler, created once on first use.
pub struct DelegateCommandHandlerDescriptor {
    id: String,
    commands: Vec<String>,
    factory: Box<dyn Fn() -> Arc<dyn DelegateCommandHandler> + Send + Sync>,
    handler: OnceLock<Arc<dyn DelegateCommandHandler>>,
}

impl DelegateCommandHandlerDescriptor {
    pub fn new(
        id: impl Into<String>,
        commands: &[&str],
        factory: impl Fn() -> Arc<dyn DelegateCommandHandler> + Send + Sync + 'static,
    ) -> Self {
        DelegateCommandHandlerDescriptor {
            id: id.into(),
            commands: commands.iter().map(|c| (*c).to_owned()).collect(),
            factory: Box::new(factory),
            handler: OnceLock::new(),
        }
    }

    /// `getDelegateCommandHandler()`.
    fn delegate_command_handler(&self) -> Arc<dyn DelegateCommandHandler> {
        self.handler.get_or_init(|| (self.factory)()).clone()
    }
}

impl std::fmt::Debug for DelegateCommandHandlerDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.id)
    }
}

/// `WorkspaceExecuteCommandHandler`.
#[derive(Default)]
pub struct WorkspaceExecuteCommandHandler {
    descriptors: RwLock<Vec<Arc<DelegateCommandHandlerDescriptor>>>,
}

/// `ResponseErrorCode.UnknownErrorCode`.
const UNKNOWN_ERROR_CODE: i64 = -32001;

impl WorkspaceExecuteCommandHandler {
    /// `WorkspaceExecuteCommandHandler.getInstance()`.
    pub fn instance() -> &'static WorkspaceExecuteCommandHandler {
        static INSTANCE: OnceLock<WorkspaceExecuteCommandHandler> = OnceLock::new();
        INSTANCE.get_or_init(WorkspaceExecuteCommandHandler::default)
    }

    /// Adds a contribution (the extension registry's `added` event).
    pub fn register(&self, descriptor: DelegateCommandHandlerDescriptor) {
        self.descriptors.write().unwrap_or_else(|e| e.into_inner()).push(Arc::new(descriptor));
    }

    /// `getAllCommands()`.
    pub fn all_commands(&self) -> std::collections::BTreeSet<String> {
        self.descriptors
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .flat_map(|d| d.commands.iter().cloned())
            .collect()
    }

    /// `executeCommand(params, monitor)`; `None` params are a Java `null`.
    pub fn execute_command(&self, params: Option<&ExecuteCommandParams>) -> Result<Option<Value>, Error> {
        let Some(params) = params.filter(|p| !p.command.is_empty()) else {
            let error_message = "The workspace/executeCommand has empty params or command";
            tracing::error!("{error_message}");
            return Err(Error { code: ErrorCode::InvalidParams, message: error_message.into(), data: None });
        };
        let candidates: Vec<Arc<DelegateCommandHandlerDescriptor>> = self
            .descriptors
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .filter(|d| d.commands.contains(&params.command))
            .cloned()
            .collect();
        if candidates.len() > 1 {
            let message = format!(
                "Found multiple delegateCommandHandlers ({}) matching command {}",
                format_set(&candidates),
                params.command
            );
            return Err(Error { code: ErrorCode::InternalError, message: message.into(), data: None });
        }
        let Some(descriptor) = candidates.first() else {
            return Err(Error {
                code: ErrorCode::MethodNotFound,
                message: format!("No delegateCommandHandler for {}", params.command).into(),
                data: None,
            });
        };
        // `JSONUtility.toModel(element, Object.class)`: a JSON-encoded string
        // argument stays a string.
        let handler = descriptor.delegate_command_handler();
        match handler.execute_command(&params.command, &params.arguments) {
            Ok(Value::Null) => Ok(None),
            Ok(v) => Ok(Some(v)),
            Err(CommandError::Response(e)) => {
                tracing::error!("Error in calling delegate command handler: {}", e.message);
                Err(e)
            }
            Err(CommandError::Exception(message)) => {
                tracing::error!("Error in calling delegate command handler: {message:?}");
                Err(Error {
                    code: ErrorCode::ServerError(UNKNOWN_ERROR_CODE),
                    message: message.unwrap_or_default().into(),
                    data: None,
                })
            }
        }
    }
}

/// `Collection.toString()` of the candidate descriptors.
fn format_set(candidates: &[Arc<DelegateCommandHandlerDescriptor>]) -> String {
    let parts: Vec<String> = candidates.iter().map(|d| format!("{d:?}")).collect();
    format!("[{}]", parts.join(", "))
}

#[cfg(test)]
mod workspace_execute_command_handler_test {
    //! Port of `org.eclipse.jdt.ls.core.internal.handlers.WorkspaceExecuteCommandHandlerTest`
    //! (`testExecuteCommandNonexistingCommand` runs over LSP in
    //! `tests/handlers_workspace_execute_command_handler_test.rs`;
    //! `testRegistryEventListener` loads OSGi bundles, which the Rust server
    //! cannot host).
    //!
    //! The jdt.ls test plug-in's `plugin.xml` contributions are registered
    //! on a handler instance: `TestDelegateCommandHandlerFactory` for
    //! `testcommand1`, `testcommand2`, `testcommand.throwexception` and
    //! `dup`, and `PlaceHolder` for `dup`.

    use super::*;
    use serde_json::json;
    use std::sync::atomic::{AtomicI32, Ordering};

    /// `TestDelegateCommandHandlerFactory.TestCommandHandler`.
    #[derive(Default)]
    struct TestCommandHandler {
        state: AtomicI32,
    }

    impl DelegateCommandHandler for TestCommandHandler {
        fn execute_command(&self, command_id: &str, arguments: &[Value]) -> Result<Value, CommandError> {
            if command_id == "testcommand.throwexception" {
                return Err(CommandError::Exception(Some("Unsupported".into())));
            }
            let args: String = arguments
                .iter()
                .map(|arg| match arg {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            Ok(json!(format!("{command_id}: {args}{}", self.state.fetch_add(1, Ordering::SeqCst))))
        }
    }

    /// `PlaceHolder`.
    struct PlaceHolder;

    impl DelegateCommandHandler for PlaceHolder {
        fn execute_command(&self, _command_id: &str, _arguments: &[Value]) -> Result<Value, CommandError> {
            Ok(Value::Null)
        }
    }

    fn handler() -> WorkspaceExecuteCommandHandler {
        let handler = WorkspaceExecuteCommandHandler::default();
        handler.register(DelegateCommandHandlerDescriptor::new(
            "org.eclipse.jdt.ls.core.internal.TestDelegateCommandHandlerFactory",
            &["testcommand1", "testcommand2", "testcommand.throwexception", "dup"],
            || Arc::new(TestCommandHandler::default()),
        ));
        handler.register(DelegateCommandHandlerDescriptor::new(
            "org.eclipse.jdt.ls.core.internal.PlaceHolder",
            &["dup"],
            || Arc::new(PlaceHolder),
        ));
        handler
    }

    fn params(command: &str, arguments: Vec<Value>) -> ExecuteCommandParams {
        ExecuteCommandParams { command: command.into(), arguments, work_done_progress_params: Default::default() }
    }

    #[test]
    fn test_execute_command() {
        let handler = handler();
        let mut params = params("testcommand1", vec![json!("hello"), json!("world")]);
        let result = handler.execute_command(Some(&params)).unwrap();
        assert_eq!(Some(json!("testcommand1: helloworld0")), result);

        params.command = "testcommand2".into();
        let result = handler.execute_command(Some(&params)).unwrap();
        assert_eq!(Some(json!("testcommand2: helloworld1")), result);
    }

    #[test]
    fn test_execute_command_morethan_one_command() {
        let handler = handler();
        let params = params("dup", vec![]);
        let ex = handler.execute_command(Some(&params)).unwrap_err();
        assert!(ex.message.starts_with("Found multiple delegateCommandHandlers"));
    }

    #[test]
    fn test_execute_command_throws_exception_command() {
        let handler = handler();
        let params = params("testcommand.throwexception", vec![]);
        let ex = handler.execute_command(Some(&params)).unwrap_err();
        assert_eq!("Unsupported", ex.message);
    }

    #[test]
    fn test_execute_command_invalid_parameters() {
        let handler = handler();
        let ex = handler.execute_command(None).unwrap_err();
        assert_eq!("The workspace/executeCommand has empty params or command", ex.message);
    }
}
