//! Scratch: print the completion list (and resolved items) for a source.
//! SCRATCH_SRC=<file with source> SCRATCH_BEHIND=<marker> SCRATCH_PATH=src/java/Foo.java
//! SCRATCH_CAPS=<json caps override for textDocument.completion> SCRATCH_SETTINGS=<json>
mod common;
use common::jdtls::*;
use serde_json::{json, Value};

#[test]
#[ignore]
fn scratch() {
    let src = std::fs::read_to_string(std::env::var("SCRATCH_SRC").unwrap()).unwrap();
    let behind = std::env::var("SCRATCH_BEHIND").unwrap();
    let path = std::env::var("SCRATCH_PATH").unwrap_or_else(|_| "src/java/Foo.java".into());
    let project = std::env::var("SCRATCH_PROJECT").unwrap_or_else(|_| "eclipse/hello".into());
    let mut ws = Workspace::new();
    ws.import_projects(&[project.as_str()]);
    let mut settings = json!({ "java": { "completion": { "postfix": { "enabled": false }, "lazyResolveTextEdit": { "enabled": false } }, "codeGeneration": { "generateComments": true }, "format": { "insertSpaces": false, "tabSize": 4 } } });
    if let Ok(s) = std::env::var("SCRATCH_SETTINGS") {
        settings = serde_json::from_str(&s).unwrap();
    }
    ws.settings = settings;
    let mut caps = default_client_capabilities();
    caps["textDocument"]["completion"] = json!({ "completionItem": { "snippetSupport": true } });
    if let Ok(c) = std::env::var("SCRATCH_CAPS") {
        caps["textDocument"]["completion"] = serde_json::from_str(&c).unwrap();
    }
    ws.capabilities = caps;
    let root = ws.project_root(project.rsplit('/').next().unwrap());
    let uri = url::Url::from_file_path(root.join(&path)).unwrap().to_string();
    ws.open_with(&uri, &src);
    let idx = src.rfind(&behind).unwrap() + behind.len();
    let before = &src[..idx];
    let line = before.matches('\n').count();
    let ch = src[before.rfind('\n').map_or(0, |i| i + 1)..idx].encode_utf16().count();
    let mut list = Value::Null;
    for _ in 0..3 {
        list = ws.request("textDocument/completion", json!({ "textDocument": { "uri": uri }, "position": { "line": line, "character": ch } }));
        if list["items"].as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
    let mut list = list;
    if std::env::var("SCRATCH_RESOLVE").is_ok() {
        let items: Vec<Value> = list["items"].as_array().cloned().unwrap_or_default();
        let mut resolved = Vec::new();
        for i in items.iter().take(10) {
            resolved.push(ws.request("completionItem/resolve", i.clone()));
        }
        list["resolved"] = json!(resolved);
    }
    println!("SCRATCH_OUT {}", serde_json::to_string_pretty(&list).unwrap());
}
