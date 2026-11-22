#!/bin/bash
# Builds Vixen.app: the Rust library, its Swift bindings, the Swift app, and the bundle.
#
#   scripts/build_app.sh [debug|release]      (default: release) builds everything into build/Vixen.app
#
# Workers building one area at the same time as others set these, so they can't break each other:
#
#   VIXEN_BUILD_ID=setup VIXEN_AREAS=setup,admin scripts/build_app.sh debug
#
# VIXEN_BUILD_ID  gives the build its own folder, target/build-<id>, holding its Rust output, generated
#                 bindings, Swift output and bundle (target/build-<id>/Vixen.app).
# VIXEN_AREAS     the window areas to compile (Cargo features of vixen-ffi, and their Swift modules): admin, dialogs,
#                 setup, preview, timeline, effecteditor, marks, shell (the sequencer window alone), sequencer (the window
#                 and those three parts), scheduler, tools.
#                 Leave it out to build them all.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CONFIG="${1:-release}"
BUILD_ID="${VIXEN_BUILD_ID:-main}"
WORK="$ROOT/target/build-$BUILD_ID"
cd "$ROOT"
source ./env.sh
command -v cargo >/dev/null 2>&1 || exit 1
xcrun --find swift >/dev/null 2>&1 || { echo "Xcode (or its command line tools) is required: install Xcode from the App Store, then run: xcode-select --install" >&2; exit 1; }
export CARGO_TARGET_DIR="$WORK/cargo"
# STUDIO-BEGIN build-identity: studio-mcp stamps the commit for studio_status and About; edits to tracked files don't rerun its build script
if [ -n "$(GIT_OPTIONAL_LOCKS=0 git -C "$ROOT" status --porcelain --untracked-files=no --ignore-submodules=dirty 2>/dev/null)" ]; then
    export VIXEN_BUILD_DIRTY=1
else
    export VIXEN_BUILD_DIRTY=0
fi
# STUDIO-END build-identity
# Keep Rust's C/C++ dependencies aligned with SwiftPM and Info.plist. Without an
# explicit target, cc uses the build host's macOS version for native objects.
export MACOSX_DEPLOYMENT_TARGET=26.0
# Keep this machine's paths (home folder, checkout, Cargo registry) out of the shipped binaries. Rust and Swift otherwise
# embed absolute source paths for panic and assertion messages. CARGO_ENCODED_RUSTFLAGS is used because paths may have spaces.
_SEP=$'\x1f'
CARGO_ENCODED_RUSTFLAGS="--remap-path-prefix=$HOME=~${_SEP}--remap-path-prefix=$ROOT=/vixen${_SEP}--remap-path-prefix=${CARGO_HOME:-$HOME/.cargo}=/cargo${_SEP}--remap-path-prefix=${RUSTUP_HOME:-$HOME/.rustup}=/rustup${_SEP}--cfg${_SEP}vixen_app_bundle"
export CARGO_ENCODED_RUSTFLAGS

echo "==> Media fallback (pinned FFmpeg source; downloaded and checksum-verified on the first build)"
"$ROOT/scripts/build_media_tools.sh" --fetch

CARGO_FEATURES=()
SWIFT_MODULES=""
if [ -n "${VIXEN_AREAS:-}" ]; then
    CARGO_FEATURES=(--no-default-features --features "$VIXEN_AREAS")
    for area in ${VIXEN_AREAS//,/ }; do
        case "$area" in
            effecteditor) module=VixenEffectEditor ;;
            sequencer) module=VixenSequencer,VixenTimeline,VixenEffectEditor,VixenMarks,VixenDialogs ;;
            marks) module=VixenMarks,VixenDialogs ;;
            setup) module=VixenSetup,VixenDialogs ;;
            shell) module=VixenSequencer ;;
            *) first="$(printf '%s' "${area:0:1}" | tr '[:lower:]' '[:upper:]')"; module="Vixen$first${area:1}" ;;
        esac
        SWIFT_MODULES="$SWIFT_MODULES,$module"
    done
    # Dependencies may also be explicitly requested (e.g. marks,dialogs).
    SWIFT_MODULES="$(printf '%s' "${SWIFT_MODULES#,}" | tr ',' '\n' | awk '!seen[$0]++' | paste -sd ',' -)"
fi

echo "==> Rust ($CONFIG)"
if [ "$CONFIG" = "release" ]; then
    cargo build -p vixen-ffi --release ${CARGO_FEATURES[@]+"${CARGO_FEATURES[@]}"}
    RUST_OUT="$CARGO_TARGET_DIR/release"
else
    cargo build -p vixen-ffi ${CARGO_FEATURES[@]+"${CARGO_FEATURES[@]}"}
    RUST_OUT="$CARGO_TARGET_DIR/debug"
fi

# STUDIO-BEGIN mcp-helper-build: compile the local stdio MCP bridge with the same bundle toolchain
if [ "$CONFIG" = "release" ]; then
    cargo build -p studio-mcp --bin vixen-studio-mcp --release
else
    cargo build -p studio-mcp --bin vixen-studio-mcp
fi
# STUDIO-END mcp-helper-build

echo "==> Swift bindings"
GEN="$WORK/uniffi-swift"
rm -rf "$GEN"
cargo run -q -p vixen-ffi --bin uniffi-bindgen ${CARGO_FEATURES[@]+"${CARGO_FEATURES[@]}"} -- \
    generate --library "$RUST_OUT/libvixen_ffi.dylib" --language swift --out-dir "$GEN"

# The package this build compiles: the real sources, linked in, plus this build's generated bindings.
PKG="$WORK/pkg"
mkdir -p "$PKG/Sources/VixenBridge" "$PKG/Sources/vixen_ffiFFI/include" "$PKG/lib"
find "$PKG/Sources" -maxdepth 1 -type l -delete
for source in app/Sources/*; do
    ln -s "$ROOT/$source" "$PKG/Sources/$(basename "$source")"
done
cp app/Package.swift "$PKG/Package.swift"
cmp -s "$GEN/vixen_ffi.swift" "$PKG/Sources/VixenBridge/vixen_ffi.swift" || cp "$GEN/vixen_ffi.swift" "$PKG/Sources/VixenBridge/vixen_ffi.swift"
cmp -s "$GEN/vixen_ffiFFI.h" "$PKG/Sources/vixen_ffiFFI/include/vixen_ffiFFI.h" || cp "$GEN/vixen_ffiFFI.h" "$PKG/Sources/vixen_ffiFFI/include/vixen_ffiFFI.h"
[ -e "$PKG/Sources/vixen_ffiFFI/shim.c" ] || echo "// Required so SwiftPM treats the header as a C target." > "$PKG/Sources/vixen_ffiFFI/shim.c"
cp "$RUST_OUT/libvixen_ffi.a" "$PKG/lib/libvixen_ffi.a"

echo "==> Swift ($CONFIG)"
( cd "$PKG" && VIXEN_SWIFT_MODULES="$SWIFT_MODULES" swift build -Xswiftc -file-prefix-map -Xswiftc "$ROOT=/vixen" -Xcc "-ffile-prefix-map=$ROOT=/vixen" -j "${VIXEN_SWIFT_JOBS:-$(sysctl -n hw.ncpu)}" -c "$CONFIG" --scratch-path "$WORK/swift" --product Vixen )
BIN="$WORK/swift/$CONFIG/Vixen"

echo "==> Bundle"
if [ "$BUILD_ID" = "main" ]; then
    APP="${APP_OUT:-$ROOT/build/Vixen.app}"
else
    APP="${APP_OUT:-$WORK/Vixen.app}"
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$APP/Contents/Helpers"
cp "$BIN" "$APP/Contents/MacOS/Vixen"
# STUDIO-BEGIN mcp-helper-bundle: ship and sign the local stdio bridge alongside media helpers
cp "$RUST_OUT/vixen-studio-mcp" "$APP/Contents/Helpers/vixen-studio-mcp"
codesign --force --sign - --timestamp=none "$APP/Contents/Helpers/vixen-studio-mcp" >/dev/null 2>&1
# STUDIO-END mcp-helper-bundle
if [ "$CONFIG" = "release" ]; then
    # Debug-map entries name the object files in this checkout; release builds don't need them.
    /usr/bin/strip -S "$APP/Contents/MacOS/Vixen"
fi
cp app/Info.plist "$APP/Contents/Info.plist"
cp -R app/Resources/. "$APP/Contents/Resources/"
cp "$ROOT/target/media-tools/bin/ffmpeg" "$ROOT/target/media-tools/bin/ffprobe" "$APP/Contents/Helpers/"
mkdir -p "$APP/Contents/Resources/MediaTools"
cp "$ROOT/target/media-tools/provenance/manifest.json" \
   "$ROOT/target/media-tools/provenance/configure-arguments.txt" \
   "$ROOT/target/media-tools/provenance/source.txt" "$APP/Contents/Resources/MediaTools/"
# Native scheduler launches own the real application/process, not the short-lived
# `open` command. This helper uses only macOS frameworks and the private socket.
/usr/bin/clang -fobjc-arc -fblocks -O2 -Wall -Wextra -arch arm64 \
    -mmacosx-version-min=26.0 -framework Foundation -framework AppKit \
    crates/vixen-session/native/launch_helper.m -o "$APP/Contents/Helpers/vixen-launch-helper"
codesign --force --sign - --timestamp=none "$APP/Contents/Helpers/vixen-launch-helper" >/dev/null 2>&1
# SwiftPM adds this unused absolute framework search path to debug executables.
# All linked dependencies are system libraries; do not ship a checkout path.
DEBUG_FRAMEWORKS="$WORK/swift/out/Products/Debug/PackageFrameworks"
if /usr/bin/otool -l "$APP/Contents/MacOS/Vixen" | /usr/bin/grep -F "path $DEBUG_FRAMEWORKS (offset" >/dev/null; then
    /usr/bin/install_name_tool -delete_rpath "$DEBUG_FRAMEWORKS" "$APP/Contents/MacOS/Vixen"
fi
codesign --force --sign - --timestamp=none "$APP" >/dev/null 2>&1
echo "Built $APP"
