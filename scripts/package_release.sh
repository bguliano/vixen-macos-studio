#!/bin/bash
# Builds, signs, notarizes and zips a release of the app.
#
#   VIXEN_SIGN_IDENTITY="Developer ID Application: <Team Name> (<TEAMID>)" \
#   VIXEN_NOTARY_PROFILE=<notarytool keychain profile> \
#   scripts/package_release.sh <release name, e.g. 3.13.4-macos-alpha.1>
#
# Create the notary profile once with:
#   xcrun notarytool store-credentials <profile> --apple-id <Apple ID> --team-id <TEAMID>
#
# Writes target/release/Vixen-Studio-<name>.zip and .dmg, each with a .sha256.  # STUDIO: Studio's archive name
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NAME="${1:?usage: scripts/package_release.sh <release name>}"
IDENTITY="${VIXEN_SIGN_IDENTITY:?set VIXEN_SIGN_IDENTITY to a Developer ID Application identity}"
PROFILE="${VIXEN_NOTARY_PROFILE:?set VIXEN_NOTARY_PROFILE to a notarytool keychain profile}"
OUT="$ROOT/target/release"
# STUDIO-BEGIN release-names: Studio ships as its own app and archives, so it installs beside the vanilla port
APP="$OUT/Vixen Studio.app"
ZIP="$OUT/Vixen-Studio-$NAME.zip"
# STUDIO-END release-names
cd "$ROOT"

rm -rf "$OUT"
mkdir -p "$OUT"
APP_OUT="$APP" scripts/build_app.sh release

echo "==> Sign ($IDENTITY)"
# Inside out: helpers first, then the app. Hardened runtime and a secure timestamp are required for notarization.
for helper in "$APP"/Contents/Helpers/*; do
    codesign --force --options runtime --timestamp --sign "$IDENTITY" "$helper"
done
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP"
codesign --verify --deep --strict --verbose=2 "$APP"

echo "==> Notarize"
ditto -c -k --keepParent "$APP" "$OUT/notarize.zip"
xcrun notarytool submit "$OUT/notarize.zip" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$APP"
xcrun stapler validate "$APP"
spctl --assess --type execute --verbose=2 "$APP"
rm "$OUT/notarize.zip"

echo "==> Zip"
ditto -c -k --keepParent "$APP" "$ZIP"
( cd "$OUT" && shasum -a 256 "$(basename "$ZIP")" > "$(basename "$ZIP").sha256" )
echo "Built $ZIP"

scripts/make_dmg.sh "$APP" "$OUT/Vixen-Studio-$NAME.dmg"  # STUDIO: Studio's archive name
