#!/bin/sh
# Launch the reference Java eclipse.jdt.ls (the "oracle") over stdio.
# Usage: scripts/oracle-jdtls.sh <data-dir>
# The server lives in .oracle/jdtls-<version> (see docs/PORTING.md).
set -e
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
JDTLS="${JDTLS_ORACLE_HOME:-$ROOT/.oracle/jdtls-1.58.0}"
DATA="${1:?usage: oracle-jdtls.sh <data-dir>}"
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) CFG=config_mac_arm ;;
  Darwin-*) CFG=config_mac ;;
  Linux-aarch64) CFG=config_linux_arm ;;
  *) CFG=config_linux ;;
esac
mkdir -p "$DATA"
# The configuration area must be writable; give each data dir its own copy.
[ -d "$DATA/.config" ] || cp -R "$JDTLS/$CFG" "$DATA/.config"
JAVA="${JAVA_HOME:+$JAVA_HOME/bin/}java"
exec "$JAVA" \
  -Declipse.application=org.eclipse.jdt.ls.core.id1 \
  -Dosgi.bundles.defaultStartLevel=4 \
  -Declipse.product=org.eclipse.jdt.ls.core.product \
  -Dosgi.checkConfiguration=true \
  -Dosgi.sharedConfiguration.area="$JDTLS/$CFG" \
  -Dosgi.sharedConfiguration.area.readOnly=true \
  -Dosgi.configuration.cascaded=true \
  -Xms256m -Xmx2G \
  --add-modules=ALL-SYSTEM \
  --add-opens java.base/java.util=ALL-UNNAMED \
  --add-opens java.base/java.lang=ALL-UNNAMED \
  -jar "$(ls "$JDTLS"/plugins/org.eclipse.equinox.launcher_*.jar | head -1)" \
  -configuration "$DATA/.config" \
  -data "$DATA/workspace"
