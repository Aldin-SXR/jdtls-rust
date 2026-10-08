//! Port of the `JavaClientConnection` calls jdt.ls makes through its
//! `ExecuteCommandProposedClient` extension of the LSP client:
//! `workspace/executeClientCommand` (a request the client answers) and
//! `workspace/notify` (a fire-and-forget notification).

// The Gradle checksum prompt (`gradle/checksum/prompt`) is the upstream
// caller of `workspace/notify`; it arrives with the Gradle importer port.
#![allow(dead_code)]

use std::time::Duration;

use serde_json::Value;
use tower_lsp::lsp_types::notification::Notification;
use tower_lsp::lsp_types::ExecuteCommandParams;
use tower_lsp::Client;

use crate::features::formatting::ExecuteClientCommand;

/// `workspace/notify` (`ExecuteCommandProposedClient.sendNotification`).
pub enum Notify {}

impl Notification for Notify {
    type Params = ExecuteCommandParams;
    const METHOD: &'static str = "workspace/notify";
}

/// Why `executeClientCommand` failed.
#[derive(Debug, PartialEq, Eq)]
pub enum ClientCommandError {
    /// `TimeoutException`: the client didn't answer in time.
    Timeout,
    /// The client answered with an error (the deepest cause's message).
    Failed(String),
}

fn params(id: &str, arguments: Vec<Value>) -> ExecuteCommandParams {
    ExecuteCommandParams { command: id.to_owned(), arguments, work_done_progress_params: Default::default() }
}

/// `JavaClientConnection.executeClientCommand([timeout,] id, params...)`:
/// without a timeout the call waits for the answer (`join()`).
pub async fn execute_client_command(
    client: &Client,
    timeout: Option<Duration>,
    id: &str,
    arguments: Vec<Value>,
) -> Result<Value, ClientCommandError> {
    let request = client.send_request::<ExecuteClientCommand>(params(id, arguments));
    let response = match timeout {
        Some(timeout) => tokio::time::timeout(timeout, request).await.map_err(|_| ClientCommandError::Timeout)?,
        None => request.await,
    };
    response.map(|v| v.unwrap_or(Value::Null)).map_err(|e| ClientCommandError::Failed(e.message.into_owned()))
}

/// `JavaClientConnection.sendNotification(id, params...)`.
pub async fn send_notification(client: &Client, id: &str, arguments: Vec<Value>) {
    client.send_notification::<Notify>(params(id, arguments)).await;
}

#[cfg(test)]
pub(crate) mod test_support {
    //! A server whose client side is played by the test: `connect` returns
    //! the server's `Client` and the socket on which the client's messages
    //! arrive (and answers go back).

    use futures::{SinkExt, StreamExt};
    use serde_json::json;
    use tower::{Service, ServiceExt};
    use tower_lsp::jsonrpc::{Request, Response, Result};
    use tower_lsp::lsp_types::{InitializeParams, InitializeResult};
    use tower_lsp::{ClientSocket, LanguageServer, LspService};

    use super::Client;

    struct Server;

    #[tower_lsp::async_trait]
    impl LanguageServer for Server {
        async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
            Ok(InitializeResult::default())
        }
        async fn shutdown(&self) -> Result<()> {
            Ok(())
        }
    }

    pub async fn connect() -> (Client, ClientSocket, LspService<impl LanguageServer>) {
        let slot = std::sync::Arc::new(std::sync::Mutex::new(None));
        let captured = slot.clone();
        let (mut service, socket) = LspService::new(move |client| {
            *captured.lock().unwrap() = Some(client);
            Server
        });
        let init = Request::build("initialize").params(json!({ "capabilities": {} })).id(1).finish();
        service.ready().await.unwrap().call(init).await.unwrap();
        let initialized = Request::build("initialized").params(json!({})).finish();
        service.ready().await.unwrap().call(initialized).await.unwrap();
        let client = slot.lock().unwrap().take().unwrap();
        (client, socket, service)
    }

    /// Answer every `workspace/executeClientCommand` request with `answer`.
    pub fn answer_requests(
        mut socket: ClientSocket,
        answer: impl Fn(&str, &[serde_json::Value]) -> std::result::Result<serde_json::Value, String> + Send + 'static,
    ) {
        tokio::spawn(async move {
            while let Some(request) = socket.next().await {
                let Some(id) = request.id().cloned() else { continue };
                let params = request.params().cloned().unwrap_or_default();
                let command = params["command"].as_str().unwrap_or_default().to_owned();
                let arguments = params["arguments"].as_array().cloned().unwrap_or_default();
                let response = match answer(&command, &arguments) {
                    Ok(v) => Response::from_ok(id, v),
                    Err(message) => Response::from_error(
                        id,
                        tower_lsp::jsonrpc::Error {
                            code: tower_lsp::jsonrpc::ErrorCode::InternalError,
                            message: message.into(),
                            data: None,
                        },
                    ),
                };
                if socket.send(response).await.is_err() {
                    break;
                }
            }
        });
    }
}

#[cfg(test)]
mod execute_client_command_test {
    //! Port of `org.eclipse.jdt.ls.core.internal.ExecuteClientCommandTest`.
    //! The mocked `JavaLanguageClient` is the client side of a real
    //! connection; a handler that throws answers with an error response.

    use super::test_support::{answer_requests, connect};
    use super::*;
    use serde_json::json;

    /// `handler("send.it.back", params -> params.getArguments())`.
    fn send_it_back(command: &str, arguments: &[Value]) -> Result<Value, String> {
        if command == "send.it.back" {
            return Ok(Value::Array(arguments.to_vec()));
        }
        Err(format!("Unknown command: {command}"))
    }

    fn boom(_: &str, _: &[Value]) -> Result<Value, String> {
        Err("BOOM!".into())
    }

    #[tokio::test]
    async fn test_execute_client_command_no_args() {
        let (client, socket, _service) = connect().await;
        answer_requests(socket, send_it_back);
        let response = execute_client_command(&client, None, "send.it.back", vec![]).await;
        assert_eq!(Ok(json!([])), response);
    }

    #[tokio::test]
    async fn test_execute_client_command_no_args_and_long_enough_timeout() {
        let (client, socket, _service) = connect().await;
        answer_requests(socket, send_it_back);
        let response = execute_client_command(&client, Some(Duration::from_secs(86400)), "send.it.back", vec![]).await;
        assert_eq!(Ok(json!([])), response);
    }

    #[tokio::test]
    async fn test_execute_client_command_throws() {
        let (client, socket, _service) = connect().await;
        answer_requests(socket, boom);
        let e = execute_client_command(&client, None, "whatever", vec![]).await.unwrap_err();
        assert_eq!(ClientCommandError::Failed("BOOM!".into()), e);
    }

    #[tokio::test]
    async fn test_execute_client_command_throws_and_long_enough_timeout() {
        let (client, socket, _service) = connect().await;
        answer_requests(socket, boom);
        let e = execute_client_command(&client, Some(Duration::from_secs(86400)), "whatever", vec![]).await.unwrap_err();
        assert_eq!(ClientCommandError::Failed("BOOM!".into()), e);
    }

    #[tokio::test]
    async fn test_execute_client_command_some_args() {
        let (client, socket, _service) = connect().await;
        answer_requests(socket, send_it_back);
        let params = vec![json!("one"), json!(2), json!([3])];
        let response = execute_client_command(&client, None, "send.it.back", params.clone()).await;
        assert_eq!(Ok(Value::Array(params)), response);
    }

    #[tokio::test]
    async fn test_execute_client_command_some_args_and_long_enough_timeout() {
        let (client, socket, _service) = connect().await;
        answer_requests(socket, send_it_back);
        let params = vec![json!("one"), json!(2), json!([3])];
        let response =
            execute_client_command(&client, Some(Duration::from_secs(86400)), "send.it.back", params.clone()).await;
        assert_eq!(Ok(Value::Array(params)), response);
    }

    #[tokio::test]
    async fn test_execute_client_command_times_out() {
        // The client never answers (the future never resolves).
        let (client, _socket, _service) = connect().await;
        let e = execute_client_command(&client, Some(Duration::from_millis(10)), "whatever", vec![]).await.unwrap_err();
        assert_eq!(ClientCommandError::Timeout, e);
    }
}

#[cfg(test)]
mod send_notification_test {
    //! Port of `org.eclipse.jdt.ls.core.internal.SendNotificationTest`: the
    //! mocked client is the test reading the server's outgoing messages.
    //! What the client does with a notification (throwing, blocking) never
    //! reaches the server: `sendNotification` returns without waiting.

    use super::test_support::connect;
    use super::*;
    use futures::StreamExt;
    use serde_json::json;

    async fn received(socket: &mut tower_lsp::ClientSocket) -> tower_lsp::jsonrpc::Request {
        tokio::time::timeout(Duration::from_millis(1000), socket.next()).await.expect("timeout(1000)").unwrap()
    }

    #[tokio::test]
    async fn test_notify_no_args() {
        let (client, mut socket, _service) = connect().await;
        send_notification(&client, "custom", vec![]).await;
        let n = received(&mut socket).await;
        assert_eq!("workspace/notify", n.method());
        assert_eq!(Some(&json!({ "command": "custom", "arguments": [] })), n.params());
    }

    #[tokio::test]
    async fn test_notify() {
        let (client, mut socket, _service) = connect().await;
        send_notification(&client, "custom", vec![json!("foo"), json!("bar")]).await;
        let n = received(&mut socket).await;
        assert_eq!("workspace/notify", n.method());
        assert_eq!(Some(&json!({ "command": "custom", "arguments": ["foo", "bar"] })), n.params());
    }

    #[tokio::test]
    async fn test_notify_with_exception() {
        let (client, mut socket, _service) = connect().await;
        send_notification(&client, "custom", vec![json!("foo"), json!("bar")]).await;
        // The client's handler throws a NullPointerException: it has received
        // the notification, and the server side is unaffected.
        let n = received(&mut socket).await;
        let was_thrown = n.method() == "workspace/notify";
        assert!(was_thrown);
        send_notification(&client, "custom", vec![]).await;
        assert_eq!("workspace/notify", received(&mut socket).await.method());
    }

    #[tokio::test]
    async fn test_notify_with_wait() {
        let (client, mut socket, _service) = connect().await;
        let (release, waiter) = tokio::sync::oneshot::channel::<()>();
        let was_called = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let called = was_called.clone();
        // The client's handler blocks until released.
        let handler = tokio::spawn(async move {
            let n = socket.next().await.unwrap();
            let _ = waiter.await;
            called.store(true, std::sync::atomic::Ordering::SeqCst);
            n
        });
        tokio::time::timeout(Duration::from_millis(1000), send_notification(&client, "custom", vec![json!("foo"), json!("bar")]))
            .await
            .expect("sendNotification doesn't wait for the client");
        assert!(!was_called.load(std::sync::atomic::Ordering::SeqCst));
        let _ = release.send(());
        assert_eq!("workspace/notify", handler.await.unwrap().method());
    }
}
