//! Upstream VM facts and direct production JVM configurator policy.
use crate::common::jdtls::{self, Workspace};
use crate::project::{self, runtime::*};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub struct Fixture {
    pub ws: Workspace,
    pub registry: RuntimeRegistry,
    pub fake: PathBuf,
    pub native: PathBuf,
    pub actionable: bool,
    oracle_setup: bool,
}
impl Fixture {
    pub fn new(actionable: bool) -> Self {
        let mut ws = Workspace::new();
        ws.settings = json!({"java":{"maven":{"downloadSources":true}}});
        ws.init_options["extendedClientCapabilities"]["actionableRuntimeNotificationSupport"] =
            json!(actionable);
        let mut fake = jdtls::fixtures_dir().join("fakejdk");
        let mut native = jdtls::fixtures_dir().join("fakejdk2/21a");
        for name in ["doc", "modules"] {
            std::fs::create_dir_all(native.join(name)).unwrap();
        }
        if jdtls::is_oracle() {
            static PRODUCT: OnceLock<PathBuf> = OnceLock::new();
            ws.oracle_home = Some(
                PRODUCT
                    .get_or_init(|| {
                        let output = Command::new("python3")
                            .arg(
                                Path::new(env!("CARGO_MANIFEST_DIR"))
                                    .join("scripts/prepare-oracle-fixture.py"),
                            )
                            .arg("jvm-configuration")
                            .output()
                            .unwrap();
                        assert!(
                            output.status.success(),
                            "{}",
                            String::from_utf8_lossy(&output.stderr)
                        );
                        PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
                    })
                    .clone(),
            );
            let bundle = ws
                .oracle_home
                .as_ref()
                .unwrap()
                .join("plugins/jdtls.rust.jvmconfiguration.tests_1.0.0");
            fake = bundle.join("fakejdk");
            native = bundle.join("fakejdk2/21a");
        }
        let mut registry = RuntimeRegistry::default();
        for entry in std::fs::read_dir(&fake)
            .unwrap()
            .flatten()
            .filter(|e| e.path().is_dir())
        {
            let id = entry.file_name().to_string_lossy().to_string();
            registry.installs.push(VmInstall {
                id: id.clone(),
                kind: "org.eclipse.jdt.ls.core.internal.TestVMType".into(),
                name: String::new(),
                home: entry.path(),
                version: Some(id.clone()),
                libraries: vec![VmLibrary {
                    path: entry.path().join("rtstubs.jar"),
                    source: None,
                    javadoc: None,
                }],
            });
        }
        registry.default_vm = Some(
            registry
                .find_vm(Some(&fake.join("21")), None)
                .unwrap()
                .key(),
        );
        Self {
            ws,
            registry,
            fake,
            native,
            actionable,
            oracle_setup: false,
        }
    }
    pub fn setup(&mut self) {
        if jdtls::is_oracle() && !self.oracle_setup {
            self.ws.request(
                "workspace/executeCommand",
                json!({"command":"jdtls.test.jvmConfiguration","arguments":[{"api":"setup"}]}),
            );
            self.oracle_setup = true;
        }
    }
    pub fn command(&mut self, input: Value) -> Value {
        self.setup();
        if jdtls::is_oracle() {
            return self.ws.request(
                "workspace/executeCommand",
                json!({"command":"jdtls.test.jvmConfiguration","arguments":[input]}),
            );
        }
        let original = self.registry.default_vm.clone();
        if input["api"] == "default" {
            let changed = self.registry.configure_default_vm(input["home"].as_str());
            return json!({"changed":changed,"different":original != self.registry.default_vm,"id":self.registry.default_install().unwrap().id});
        }
        let runtime: RuntimeEnvironment = serde_json::from_value(input.clone()).unwrap();
        let mut result = json!({"valid":runtime.is_valid(),"javadoc":runtime.javadoc_url()});
        if input["api"] == "javadoc" {
            return result;
        }
        let file = runtime.installation_file();
        result["directory"] = json!(file.as_ref().is_some_and(|p| p.is_dir()));
        result["validated"] = json!(file.as_ref().is_some_and(|p| valid_installation(p)));
        let configured = self.registry.configure(&[runtime.clone()], None);
        result["changed"] = json!(configured.changed);
        // The production server uses this same notification formatter.
        result["notices"] = json!(configured
            .notices
            .iter()
            .map(|message| notice(message, self.actionable))
            .collect::<Vec<_>>());
        let vm = self
            .registry
            .find_vm(file.as_deref(), runtime.name.as_deref())
            .cloned();
        result["present"] = json!(vm.is_some());
        if let Some(vm) = vm {
            result["vm2"] = json!(true);
            result["version"] = json!(vm.version);
            result["librariesNotNull"] = json!(true);
            result["libraries"] = json!(vm
                .libraries
                .iter()
                .map(|l| json!({"path":l.path,"javadoc":l.javadoc}))
                .collect::<Vec<_>>());
            result["different"] = json!(original != self.registry.default_vm);
            result["defaultSame"] = json!(self.registry.default_vm.as_ref() == Some(&vm.key()));
            result["environmentPresent"] =
                json!(runtime.name.as_deref().is_some_and(environment_supported));
            result["environmentSame"] = json!(
                runtime
                    .name
                    .as_ref()
                    .and_then(|n| self.registry.environments.get(n))
                    == Some(&vm.key())
            );
            if input["dispose"] == true {
                self.registry.installs.retain(|v| v.key() != vm.key());
                result["disposedAbsent"] = json!(self
                    .registry
                    .find_vm(None, runtime.name.as_deref())
                    .is_none());
            }
        }
        result
    }
    pub fn notices(&mut self, result: &Value, method: &str) -> Vec<Value> {
        if jdtls::is_oracle() {
            self.ws.wait_idle();
            return self
                .ws
                .client()
                .notifications
                .iter()
                .filter(|n| n["method"] == method)
                .map(|n| n["params"].clone())
                .collect();
        }
        result["notices"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["method"] == method)
            .map(|n| n["params"].clone())
            .collect()
    }
    pub fn preview(&mut self, root: &Path, versions: &[&str]) -> Value {
        let name = project::invisible::project_name(root);
        if jdtls::is_oracle() {
            return self.command(json!({"api":"preview","project":name,"versions":versions}));
        }
        let mut settings = project::ImportSettings::jdtls_defaults();
        settings.vm_version = Some("21".into());
        settings.trigger_files = vec![root.join("foo/bar/Foo.java")];
        settings.data_dir = Some(self.ws.server_workspace_dir());
        let mut model = project::Workspace::import(&[root.into()], &settings);
        let mut values = Vec::new();
        for version in versions {
            self.registry
                .configure_default_vm(self.fake.join(version).to_str());
            self.registry.apply_to_workspace(&mut model);
            let mut default = project::default_java_project(
                &self
                    .ws
                    .workspace_project_location(project::DEFAULT_PROJECT_NAME),
            );
            configure_project_preview(&mut default, self.registry.default_install().unwrap());
            values.push(json!([default,model.project(&name).unwrap().clone()].iter().map(|p| json!({
                "compliance":project::effective_option(p,project::COMPLIANCE,model.vm_version.as_deref()),
                "preview":project::effective_option(p,project::ENABLE_PREVIEW,model.vm_version.as_deref())
            })).collect::<Vec<_>>()));
        }
        json!(values)
    }
}
