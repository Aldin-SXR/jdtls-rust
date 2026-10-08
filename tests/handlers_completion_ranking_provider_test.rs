//! Port of `org.eclipse.jdt.ls.core.internal.handlers.CompletionRankingProviderTest`.
//!
//! Upstream registers a Mockito-spied `ICompletionRankingProvider` in the
//! server's `CompletionContributionService` before each test. The ranking
//! API is ported (`src/features/completion/ranking.rs`), but providers are
//! registered in-process only: nothing on the LSP side can register one, so
//! the tests stay ignored.

mod common;
use common::completion::*;
use serde_json::json;

const NO_PROVIDER: &str = "registers an in-process ICompletionRankingProvider (a Mockito spy); providers cannot be registered over LSP";

/// `AbstractCompilationUnitBasedTest.setup` + `CompletionRankingProviderTest.setUp`.
fn setup() -> T {
    let mut t = setup_with(settings());
    t.ws.use_upstream_test_jdk("hello");
    t.caps = Caps::default();
    t
}

#[test]
#[ignore = "registers an in-process ICompletionRankingProvider (a Mockito spy); providers cannot be registered over LSP"]
fn test_rank() {
    let _ = NO_PROVIDER;
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n \t\tInteger.\n\t}\n}\n");

    let list = t.request_completions(&unit, "Integer.");
    assert!(!list.is_null());
    assert!(!items(&list).is_empty(), "No proposals were found");

    let recommended = &items(&list)[0];
    assert!(s(&recommended["label"]).starts_with('★'), "{recommended:#}");
    assert_eq!(recommended["filterText"], recommended["insertText"]);
}

#[test]
#[ignore = "registers an in-process ICompletionRankingProvider (a Mockito spy); providers cannot be registered over LSP"]
fn test_on_did_completion_item_select() {
    let mut t = setup();
    let unit = t.get_working_copy("src/java/Foo.java", "public class Foo {\n\tvoid foo() {\n \t\tInteger.\n\t}\n}\n");

    let list = t.request_completions(&unit, "Integer.");
    // handler.onDidCompletionItemSelect(String.valueOf((new CompletionResponse()).getId() - 1), "0")
    let rid = items(&list)[0]["data"]["rid"].clone();
    t.ws.request("workspace/executeCommand", json!({ "command": "java.completion.onDidSelect", "arguments": [rid, "0"] }));

    // verify(provider, times(1)).onDidCompletionItemSelect(argument.capture()):
    // the item data must carry the provider's data ("foo" -> "bar") and
    // COMPLETION_EXECUTION_TIME. Only an in-process provider can observe it.
    panic!("{NO_PROVIDER}");
}
