//! Completion behavior across preferences, working copies and request boundaries.
mod common;
use common::jdtls::*;
use serde_json::{json, Value};

fn workspace() -> Workspace {
    let mut ws = Workspace::new();
    ws.import_projects(&["eclipse/hello"]);
    ws.settings = json!({"java": {
        "completion":{"postfix":{"enabled":false}},
        "signatureHelp":{"enabled":true},
        "format":{"insertSpaces":false,"tabSize":4},
        "maven":{"defaultMojoExecutionAction":"ignore"}
    }});
    ws.capabilities["textDocument"]["completion"]["completionItem"]["snippetSupport"] = json!(true);
    ws.capabilities["textDocument"]["completion"]["completionItem"]["labelDetailsSupport"] =
        json!(false);
    ws
}

fn virtual_unit(ws: &mut Workspace, name: &str, source: &str) -> String {
    let path = ws.project_root("hello").join("src/org/sample").join(name);
    assert!(!path.exists());
    let uri = url::Url::from_file_path(path).unwrap().to_string();
    ws.open_with(&uri, source);
    uri
}

fn complete(ws: &mut Workspace, uri: &str, source: &str, behind: &str) -> Value {
    let offset = source.rfind(behind).unwrap() + behind.len();
    let line = source[..offset].bytes().filter(|b| *b == b'\n').count();
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    ws.request(
        "textDocument/completion",
        json!({
            "textDocument":{"uri":uri},
            "position":{"line":line,"character":source[start..offset].encode_utf16().count()}
        }),
    )
}

fn item(list: &Value, label: &str) -> Value {
    list["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["label"] == label)
        .unwrap_or_else(|| panic!("{list:#}"))
        .clone()
}

fn replacement(item: &Value) -> &str {
    item["textEdit"]["newText"]
        .as_str()
        .or_else(|| item["insertText"].as_str())
        .unwrap()
}

#[test]
fn empty_template_preferences_produce_no_type_comment() {
    let mut ws = workspace();
    let source = "package org.sample;\n";
    let uri = virtual_unit(&mut ws, "EmptyTemplate.java", source);
    let list = complete(&mut ws, &uri, source, source);
    assert_eq!(
        "public class EmptyTemplate {\n\n\t${0}\n}",
        replacement(&item(&list, "class"))
    );
}

#[test]
fn custom_file_and_type_templates_use_working_copy_names_and_delimiters() {
    let mut ws = workspace();
    ws.settings["java"]["templates"] = json!({
        "fileHeader":["// ${file_name} in ${package_name}"],
        "typeComment":["/**"," * Custom ${type_name}"," * ${tags}"," */"]
    });
    let source = "package org.sample;\r\n";
    let uri = virtual_unit(&mut ws, "CustomTemplate.java", source);
    let list = complete(&mut ws, &uri, source, source);
    assert_eq!("// CustomTemplate.java in org.sample\n/**\n * Custom CustomTemplate\n */\npublic class CustomTemplate {\r\n\r\n\t${0}\r\n}", replacement(&item(&list, "class")));
}

#[test]
fn completion_resolve_rejects_a_proposal_from_an_expired_request() {
    let mut ws = workspace();
    let source = "package org.sample;\nclass Cache { void run(){ Str } }\n";
    let uri = virtual_unit(&mut ws, "Cache.java", source);
    let first = complete(&mut ws, &uri, source, "Str");
    let first = item(&first, "String - java.lang");
    assert!(first["data"]["rid"].is_string());
    let second = complete(&mut ws, &uri, source, "Str");
    assert_ne!(
        first["data"]["rid"],
        item(&second, "String - java.lang")["data"]["rid"]
    );
    let response = ws
        .client()
        .request_response("completionItem/resolve", first);
    assert_eq!(-32603, response["error"]["code"]);
    assert_eq!("Internal error.", response["error"]["message"]);
    assert!(
        response["error"]["data"]
            .as_str()
            .unwrap()
            .contains("Invalid completion proposal"),
        "{response:#}"
    );
}

#[test]
fn selecting_an_overload_triggers_client_hints_and_selects_its_signature() {
    let mut ws = workspace();
    ws.init_options["extendedClientCapabilities"]["executeClientCommandSupport"] = json!(true);
    ws.init_options["extendedClientCapabilities"]["onCompletionItemSelectedCommand"] =
        json!("editor.action.triggerParameterHints");
    let source = "package org.sample;\nclass Selection {\n void choose(int number){}\n void choose(String text){}\n void other(boolean flag){}\n void run(){ cho }\n}\n";
    let uri = virtual_unit(&mut ws, "Selection.java", source);
    let list = complete(&mut ws, &uri, source, "cho");
    let changed = source.replace("run(){ cho }", "run(){ choose() }");
    ws.change(&uri, &changed);
    let params = json!({"textDocument":{"uri":uri}, "position":{"line":5,"character":20}});
    let baseline = ws.request("textDocument/signatureHelp", params.clone());
    let baseline_index = baseline["activeSignature"].as_u64().unwrap() as usize;
    let baseline_label = baseline["signatures"][baseline_index]["label"]
        .as_str()
        .unwrap();
    let target = if baseline_label.contains("String") {
        "choose(int number) : void"
    } else {
        "choose(String text) : void"
    };
    let selected = item(&list, target);
    assert_ne!(baseline_label, target);
    ws.client()
        .take_server_requests("workspace/executeClientCommand");
    ws.request("workspace/executeCommand", selected["command"].clone());
    let commands = ws
        .client()
        .take_server_requests("workspace/executeClientCommand");
    assert_eq!(1, commands.len(), "{commands:#?}");
    assert_eq!(
        "editor.action.triggerParameterHints",
        commands[0]["params"]["command"]
    );
    let help = ws.request("textDocument/signatureHelp", params.clone());
    let active = help["activeSignature"].as_u64().unwrap() as usize;
    assert_eq!(target, help["signatures"][active]["label"], "{help:#}");
    assert_eq!(0, help["activeParameter"]);
    // A request whose signature cannot match the selection clears it.
    let unrelated = changed.replace("run(){ choose() }", "run(){ other() }");
    ws.change(&uri, &unrelated);
    let unrelated_help = ws.request(
        "textDocument/signatureHelp",
        json!({
            "textDocument":{"uri":uri}, "position":{"line":5,"character":19}
        }),
    );
    let unrelated_active = unrelated_help["activeSignature"].as_u64().unwrap() as usize;
    assert_eq!(
        "other(boolean flag) : void",
        unrelated_help["signatures"][unrelated_active]["label"]
    );
    ws.change(&uri, &changed);
    let reset = ws.request("textDocument/signatureHelp", params);
    let reset_active = reset["activeSignature"].as_u64().unwrap() as usize;
    assert_eq!(baseline_label, reset["signatures"][reset_active]["label"]);
}

#[test]
fn override_completion_generates_a_super_call_in_rust() {
    let mut ws = workspace();
    let source = "package org.sample;\nclass Override {\n hashC\n}\n";
    let uri = virtual_unit(&mut ws, "Override.java", source);
    let list = complete(&mut ws, &uri, source, "hashC");
    let proposal = item(&list, "hashCode() : int");
    assert_eq!("Override method in 'Object'", proposal["detail"]);
    assert_eq!("@Override\npublic int hashCode() {\n\t${0:// TODO Auto-generated method stub\n\treturn super.hashCode();}\n}", replacement(&proposal));
}

#[test]
fn override_completion_generates_an_interface_default_method() {
    let mut ws = workspace();
    let source = "package org.sample;\ninterface OverrideParent { int value(); }\ninterface OverrideChild extends OverrideParent {\n val\n}\n";
    let uri = virtual_unit(&mut ws, "OverrideChild.java", source);
    let list = complete(&mut ws, &uri, source, "val");
    let proposal = item(&list, "value() : int");
    assert_eq!("Override method in 'OverrideParent'", proposal["detail"]);
    assert_eq!("@Override\ndefault int value() {\n\t${0:// TODO Auto-generated method stub\n\tthrow new UnsupportedOperationException(\"Unimplemented method 'value'\");}\n}", replacement(&proposal));
}
