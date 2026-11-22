#!/bin/bash
# Prints the folder holding the pinned Sparkle 2 release (B49): Sparkle.xcframework, which the app links for automatic
# updates, and bin/ with Sparkle's release tools (sign_update, generate_keys, generate_appcast). Fetches the release's
# Swift Package Manager archive once and checks its hash, as build_media_tools.sh does for FFmpeg.
#
#   SPARKLE="$(scripts/studio_sparkle.sh)"     # then "$SPARKLE/Sparkle.xcframework", "$SPARKLE/bin/sign_update"
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION=2.10.1
# The checksum Sparkle's own Package.swift pins for this archive.
SHA256=4c2e48a3e50ebcc2c88a58a4c7264a1a99c3c31857ce754357afb9cdb323022a
URL="https://github.com/sparkle-project/Sparkle/releases/download/$VERSION/Sparkle-for-Swift-Package-Manager.zip"
WORK="$ROOT/target/sparkle/$VERSION"
if [ ! -f "$WORK/.complete" ]; then
    mkdir -p "$WORK"
    ARCHIVE="$WORK/../Sparkle-$VERSION-spm.zip"
    /usr/bin/curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 "$URL" -o "$ARCHIVE.part" >&2
    ACTUAL="$(/usr/bin/shasum -a 256 "$ARCHIVE.part" | awk '{print $1}')"
    [ "$ACTUAL" = "$SHA256" ] || { rm -f "$ARCHIVE.part"; echo "Sparkle $VERSION archive hash mismatch" >&2; exit 1; }
    rm -rf "$WORK"
    mkdir -p "$WORK"
    /usr/bin/ditto -x -k "$ARCHIVE.part" "$WORK"
    rm -f "$ARCHIVE.part"
    touch "$WORK/.complete"
fi
echo "$WORK"
