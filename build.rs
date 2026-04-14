use std::path::Path;
use std::process::Command;

fn main() {
    let bridge_pom = "ecj-bridge/pom.xml";
    let bridge_src = "ecj-bridge/src";
    let shaded_jar = "ecj-bridge/target/ecj-bridge-shaded.jar";

    emit_rerun_if_changed(Path::new(bridge_src));
    println!("cargo:rerun-if-changed={}", bridge_pom);

    // Skip Maven build when SKIP_ECJ_BUILD is set.
    if std::env::var("SKIP_ECJ_BUILD").is_ok() {
        write_jar_stub();
        return;
    }

    // Check Maven is available
    let mvn_available = Command::new("mvn")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if !mvn_available {
        println!("cargo:warning=mvn not found — skipping ecj-bridge build. \
                  Semantic features will be unavailable until you run: \
                  mvn -f ecj-bridge/pom.xml package");
        write_jar_stub();
        return;
    }

    // Build the ecj-bridge fat JAR via Maven
    let status = Command::new("mvn")
        .args(["-f", bridge_pom, "package", "-q", "-DskipTests"])
        .status()
        .expect("failed to run mvn");

    if !status.success() {
        panic!("ecj-bridge Maven build failed");
    }

    // Copy JAR to OUT_DIR so include_bytes! can reference a stable path
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("ecj-bridge-shaded.jar");
    std::fs::copy(shaded_jar, &dest)
        .unwrap_or_else(|e| panic!("failed to copy {shaded_jar} → {dest:?}: {e}"));

    // Write a Rust source file that embeds the JAR bytes.
    // include_bytes! needs a string literal, so we generate the file
    // rather than using concat!(env!(...)) which doesn't work in all cases.
    let rs_path = Path::new(&out_dir).join("ecj_jar_bytes.rs");
    std::fs::write(
        &rs_path,
        format!(
            "pub static ECJ_JAR_BYTES: &[u8] = include_bytes!({:?});\n",
            dest
        ),
    )
    .unwrap();
}

/// When Maven is unavailable, write a zero-byte stub so the code still compiles.
/// The server will report an error at runtime if semantic features are requested.
fn write_jar_stub() {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let rs_path = Path::new(&out_dir).join("ecj_jar_bytes.rs");
    std::fs::write(&rs_path, "pub static ECJ_JAR_BYTES: &[u8] = &[];\n").unwrap();
}

fn emit_rerun_if_changed(path: &Path) {
    println!("cargo:rerun-if-changed={}", path.display());
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                emit_rerun_if_changed(&entry_path);
            } else {
                println!("cargo:rerun-if-changed={}", entry_path.display());
            }
        }
    }
}
