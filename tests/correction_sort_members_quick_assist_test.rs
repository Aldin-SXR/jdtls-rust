//! Port of `org.eclipse.jdt.ls.core.internal.correction.SortMembersQuickAssistTest`.

mod common;

use common::quickfix::{get_range, Expected, QuickFixTest};
use serde_json::json;
use std::path::PathBuf;

fn setup() -> (QuickFixTest, PathBuf) {
    let mut t = QuickFixTest::new();
    let root = t.ws.new_empty_project(&common::jdtls::test_default_options());
    (t, root)
}

#[test]
fn test_sort_members_for_type() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tprivate String privateStr = \"private\";\n");
    buf.push_str("\tprivate String getPrivateStr() { return \"private\"; }\n");
    buf.push_str("\tpublic String publicStr = \"public\";\n");
    buf.push_str("\tpublic String getPublicStr() { return \"public\"; }\n");
    buf.push_str("\tprivate String privateStr1 = \"private1\";\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tprivate String privateStr = \"private\";\n");
    buf.push_str("\tpublic String publicStr = \"public\";\n");
    buf.push_str("\tprivate String privateStr1 = \"private1\";\n");
    buf.push_str("\tpublic String getPublicStr() { return \"public\"; }\n");
    buf.push_str("\tprivate String getPrivateStr() { return \"private\"; }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Sort Members for 'A.java'", &buf);

    let selection = get_range(&t.ws.read(&cu), "A");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_sort_members_for_type_with_fields() {
    let (mut t, root) = setup();
    t.ws.settings["java"]["codeAction"]["sortMembers"]["avoidVolatileChanges"] = json!(false);
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tprivate String privateStr = \"private\";\n");
    buf.push_str("\tprivate String getPrivateStr() { return \"private\"; }\n");
    buf.push_str("\tpublic String publicStr = \"public\";\n");
    buf.push_str("\tpublic String getPublicStr() { return \"public\"; }\n");
    buf.push_str("\tprivate String privateStr1 = \"private1\";\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tpublic String publicStr = \"public\";\n");
    buf.push_str("\tprivate String privateStr = \"private\";\n");
    buf.push_str("\tprivate String privateStr1 = \"private1\";\n");
    buf.push_str("\tpublic String getPublicStr() { return \"public\"; }\n");
    buf.push_str("\tprivate String getPrivateStr() { return \"private\"; }\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Sort Members for 'A.java'", &buf);

    let selection = get_range(&t.ws.read(&cu), "A");
    t.assert_code_actions_range(&cu, selection, &[e1]);
}

#[test]
fn test_sort_members_for_selection() {
    let (mut t, root) = setup();
    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tprivate String privateStr = \"private\";\n");
    buf.push_str("\tprivate String getPrivateStr() { return \"private\"; }\n");
    buf.push_str("\tpublic String publicStr = \"public\";\n");
    buf.push_str("\tpublic String getPublicStr() { return \"public\"; }\n");
    buf.push_str("\tprivate String privateStr1 = \"private1\";\n");
    buf.push_str("}\n");
    let cu = t.ws.create_cu(&root, "src", "test", "A.java", &buf);

    let mut buf = String::new();
    buf.push_str("package test;\n");
    buf.push_str("public class A {\n");
    buf.push_str("\tprivate String privateStr = \"private\";\n");
    buf.push_str("\tpublic String publicStr = \"public\";\n");
    buf.push_str("\tpublic String getPublicStr() { return \"public\"; }\n");
    buf.push_str("\tprivate String getPrivateStr() { return \"private\"; }\n");
    buf.push_str("\tprivate String privateStr1 = \"private1\";\n");
    buf.push_str("}\n");
    let e1 = Expected::new("Sort Selected Members", &buf);

    let selection = get_range(
        &t.ws.read(&cu),
        "private String getPrivateStr() { return \"private\"; }\n\tpublic String publicStr = \"public\";\n\tpublic String getPublicStr() { return \"public\"; }\n",
    );
    t.assert_code_actions_range(&cu, selection, &[e1]);
}
