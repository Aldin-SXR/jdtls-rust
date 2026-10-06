#!/usr/bin/env python3
"""Build a test fragment in an isolated copy of the real Eclipse oracle.

The original product is never changed. No manager/decompiler implementation is
compiled here: the fragment supplies a call adapter and, where required, the
upstream tests' fake extensions.
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess

repo = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("fixture", choices=["content-provider", "projects-manager", "jvm-configuration"])
fixture = parser.parse_args().fixture
source = repo / "tests/oracle" / fixture
oracle = Path(os.environ.get("JDTLS_ORACLE_HOME", repo / ".oracle/jdtls-1.58.0")).resolve()
product = repo / "target" / f"{fixture}-oracle"
classes = product / "classes"
classes.mkdir(parents=True, exist_ok=True)
plugins = product / "plugins"
plugins.mkdir(exist_ok=True)
for plugin in (oracle / "plugins").iterdir():
    link = plugins / plugin.name
    if not link.exists():
        link.symlink_to(plugin, target_is_directory=plugin.is_dir())
def java_tool(name):
    if "JAVA_HOME" in os.environ:
        return str(Path(os.environ["JAVA_HOME"]) / "bin" / name)
    tool = shutil.which(name)
    if tool is None:
        raise RuntimeError(f"{name} not found; set JAVA_HOME to a JDK 21 or newer")
    return tool

classpath = os.pathsep.join(str(p) for p in (oracle / "plugins").glob("*.jar"))
subprocess.run([java_tool("javac"), "--release", "21", "-cp", classpath, "-d", str(classes),
                *(str(p) for p in (source / "src").rglob("*.java"))], check=True)
jar_name = f"jdtls.rust.{fixture.replace('-', '')}.tests_1.0.0.jar"
shutil.copyfile(source / "plugin.xml", classes / "fragment.xml")
if fixture == "jvm-configuration":
    # TestVMType's unchanged resource lookup requires actual directories.
    jar_name = "jdtls.rust.jvmconfiguration.tests_1.0.0"
    bundle = plugins / jar_name
    shutil.copytree(classes, bundle, dirs_exist_ok=True)
    (bundle / "META-INF").mkdir(exist_ok=True)
    shutil.copyfile(source / "MANIFEST.MF", bundle / "META-INF/MANIFEST.MF")
    for directory in ("fakejdk", "fakejdk2"):
        shutil.copytree(repo / "tests/fixtures" / directory, bundle / directory, dirs_exist_ok=True)
    for directory in ("doc", "modules"):
        (bundle / "fakejdk2/21a" / directory).mkdir(exist_ok=True)
else:
    subprocess.run([java_tool("jar"), "--create", "--file", str(plugins / jar_name),
                "--manifest", str(source / "MANIFEST.MF"), "-C", str(classes), "."], check=True)
for config in oracle.glob("config_*"):
    destination = product / config.name
    shutil.copytree(config, destination, dirs_exist_ok=True)
    ini = destination / "config.ini"
    lines = ini.read_text().splitlines()
    lines = [line + r",reference\:file\:" + jar_name + "@4" if line.startswith("osgi.bundles=") else line for line in lines]
    # The launcher is symlinked; explicitly resolve bundles from this product,
    # not the launcher's original installation (which lacks our test fragment).
    lines.append("osgi.install.area=" + product.as_uri())
    ini.write_text("\n".join(lines) + "\n")
print(product)
