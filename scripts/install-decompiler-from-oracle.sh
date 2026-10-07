#!/bin/sh
# Install the FernFlower decompiler bundled with the jdt.ls oracle into ~/.m2, for
# environments that cannot reach the JetBrains Maven repository.
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JDTLS="${JDTLS_ORACLE_HOME:-$ROOT/.oracle/jdtls-1.58.0}"
JAR="$(ls "$JDTLS"/plugins/wrapped.com.jetbrains.intellij.java.java-decompiler-engine_*.jar | head -1)"
exec mvn -q install:install-file -Dfile="$JAR" -DgroupId=com.jetbrains.intellij.java \
  -DartifactId=java-decompiler-engine -Dversion=253.29346.240 -Dpackaging=jar
