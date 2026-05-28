#!/usr/bin/env bash
# Fetch the doltlite native lib for this platform and prepare it
# for linking from build.rs. Run once before `cargo run`.
#
# After download, fixes the dylib's install_name from
# /usr/local/lib/libdoltlite.dylib to @rpath/libdoltlite.dylib so
# the binary doesn't need root-installed libdoltlite, and ad-hoc
# re-signs (macOS).

set -euo pipefail

cd "$(dirname "$0")"

VERSION="0.11.2"
ARCH="$(uname -sm)"

case "$ARCH" in
    "Darwin arm64")  PKG="doltlite-lib-osx-arm64-${VERSION}" ;;
    "Linux x86_64")  PKG="doltlite-lib-linux-x64-${VERSION}" ;;
    "Linux aarch64") PKG="doltlite-lib-linux-arm64-${VERSION}" ;;
    *) echo "Unsupported platform: $ARCH"; exit 1 ;;
esac

if [ -d "$PKG" ] && [ -f "$PKG/libdoltlite.dylib" -o -f "$PKG/libdoltlite.so" ]; then
    echo "$PKG already present"
    exit 0
fi

ZIP="${PKG}.zip"
if [ ! -f "$ZIP" ]; then
    gh release download "v${VERSION}" -R dolthub/doltlite --pattern "$ZIP"
fi
unzip -q -o "$ZIP"

# Fix install_name on macOS so RPATH works from any vendored location.
if [[ "$ARCH" == Darwin* ]]; then
    install_name_tool -id "@rpath/libdoltlite.dylib" "$PKG/libdoltlite.dylib"
    codesign --remove-signature "$PKG/libdoltlite.dylib" 2>/dev/null || true
    codesign -s - "$PKG/libdoltlite.dylib"
fi

echo "Ready: $PKG"
