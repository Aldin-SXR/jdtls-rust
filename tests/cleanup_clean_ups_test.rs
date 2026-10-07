//! Ported methods of CleanUpsTest. Registry calls are exercised through the
//! public java/cleanup request with the same cleanup IDs and source assertions.
mod common;
use common::jdtls::*;
use serde_json::{json, Value};

// Match the TestVMType classpath used by the upstream manager fixture, rather
// than mixing the stub API with the running VM's internal compiler types.
fn test_vm(ws: &mut Workspace) {
    let home = if is_oracle() {
        static PRODUCT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
        let product = PRODUCT
            .get_or_init(|| {
                let output = std::process::Command::new("python3")
                    .arg(concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/scripts/prepare-oracle-fixture.py"
                    ))
                    .arg("jvm-configuration")
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
                std::path::PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
            })
            .clone();
        ws.oracle_home = Some(product.clone());
        product.join("plugins/jdtls.rust.jvmconfiguration.tests_1.0.0/fakejdk/22")
    } else {
        let home = ws.external_dir().join("test-vm-22");
        std::fs::create_dir_all(home.join("lib")).unwrap();
        std::fs::copy(
            fixtures_dir().join("fakejdk/22/rtstubs.jar"),
            home.join("lib/rt.jar"),
        )
        .unwrap();
        std::fs::write(home.join("release"), "JAVA_VERSION=\"22\"\n").unwrap();
        home
    };
    ws.settings["java"]["home"] = json!(home);
}

fn setup(source: &str, ids: &[&str]) -> (Workspace, String) {
    let mut ws = Workspace::new();
    ws.import_projects(&["maven/quickstart"]);
    ws.use_upstream_maven_test_jdk("quickstart", "22");
    let root = ws.project_root("quickstart");
    let mut options = test_default_options();
    for key in ["source", "compliance", "codegen.targetPlatform"] {
        options.insert(format!("org.eclipse.jdt.core.compiler.{key}"), "22".into());
    }
    options.insert(
        "org.eclipse.jdt.core.formatter.number_of_empty_lines_to_preserve".into(),
        "99".into(),
    );
    options.insert(
        "org.eclipse.jdt.core.formatter.indent_switchstatements_compare_to_switch".into(),
        "true".into(),
    );
    ws.set_project_options(&root, &options);
    ws.settings["java"]["cleanup"] = json!({"actions": ids});
    ws.init_options["extendedClientCapabilities"]["canUseInternalSettings"] = json!(false);
    test_vm(&mut ws);
    let uri = ws.create_cu(&root, "src/main/java", "test1", "A.java", source);
    (ws, uri)
}

fn edits(ws: &mut Workspace, uri: &str) -> Value {
    ws.wait_for_background_jobs();
    ws.open(uri);
    ws.request("java/cleanup", json!({"uri": uri}))["changes"][uri].clone()
}

#[test]
fn test_no_clean_up() {
    let source = "package test1;\npublic class A implements Runnable {\n    public void run() {} \n    /**\n     * @deprecated\n     */\n    public void destroy() {} \n}\n";
    let (mut ws, uri) = setup(source, &[]);
    assert_eq!(0, edits(&mut ws, &uri).as_array().unwrap().len());
}

#[test]
fn test_invert_equals_clean_up() {
    // Preserve the original missing semicolon and suppressed text-block newline.
    let source = "package test1;\npublic class A {\n    String message;\n    boolean result1 = message.equals(\"text\");\n    boolean result2 = message.equalsIgnoreCase(\"text\")}\n";
    let (mut ws, uri) = setup(source, &["invertEquals"]);
    let actual = apply_edits(source, edits(&mut ws, &uri).as_array().unwrap());
    let expected = "package test1;\npublic class A {\n    String message;\n    boolean result1 = \"text\".equals(message);\n    boolean result2 = \"text\".equalsIgnoreCase(message)}\n";
    assert_eq!(expected, actual);
}

#[test]
fn test_organize_imports_cleanup() {
    let source = "package test1;\npublic class A {\n    public void test() {\n        List<String> a1;\n\t    Iterator<String> a2;\n\t    Map<String, String> a3;\n\t    Set<String> a4;\n\t    JarFile a5;\n\t    StringTokenizer a6;\n\t    Path a7;\n\t    URI a8;\n\t    HttpURLConnection a9;\n\t    InputStream a10;\n\t    Field a11;\n\t    Parser a12;\n    }\n}\n";
    let expected = "package test1;\n\nimport java.io.InputStream;\nimport java.net.HttpURLConnection;\nimport java.net.URI;\nimport java.nio.file.Path;\nimport java.util.Iterator;\nimport java.util.List;\nimport java.util.Map;\nimport java.util.Set;\nimport java.util.StringTokenizer;\nimport java.util.jar.JarFile;\n\npublic class A {\n    public void test() {\n        List<String> a1;\n\t    Iterator<String> a2;\n\t    Map<String, String> a3;\n\t    Set<String> a4;\n\t    JarFile a5;\n\t    StringTokenizer a6;\n\t    Path a7;\n\t    URI a8;\n\t    HttpURLConnection a9;\n\t    InputStream a10;\n\t    Field a11;\n\t    Parser a12;\n    }\n}\n";
    let (mut ws, uri) = setup(source, &["organizeImports"]);
    assert_eq!(
        expected,
        apply_edits(source, edits(&mut ws, &uri).as_array().unwrap())
    );
}
