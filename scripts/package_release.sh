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
# STUDIO-BEGIN sparkle-release-usage: automatic updates (B49, ops/RELEASES.md)
#
# VIXEN_SPARKLE_PUBLIC_KEY  the EdDSA public key (generate_keys -p) that signs updates. With it the app checks
#                           SPARKLE_FEED_URL below; without it the app is built with its updater off.
# VIXEN_BUILD_NUMBER        CFBundleVersion, which Sparkle orders releases by and which must always grow. Defaults to
#                           the number of commits in HEAD's history.
# STUDIO-END sparkle-release-usage
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

# STUDIO-BEGIN sparkle-release-stamp: version every release for Sparkle, and give it the feed only with a signing key
# Shipped apps read this feed for as long as they're installed: never change it.
SPARKLE_FEED_URL="https://raw.githubusercontent.com/bguliano/vixen-macos-studio/appcast/appcast.xml"
PLIST="$APP/Contents/Info.plist"
BUILD_NUMBER="${VIXEN_BUILD_NUMBER:-$(git rev-list --count HEAD)}"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $BUILD_NUMBER" "$PLIST"
/usr/libexec/PlistBuddy -c "Delete :SUFeedURL" "$PLIST" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Delete :SUPublicEDKey" "$PLIST" 2>/dev/null || true
if [ -n "${VIXEN_SPARKLE_PUBLIC_KEY:-}" ]; then
    /usr/libexec/PlistBuddy -c "Add :SUFeedURL string $SPARKLE_FEED_URL" "$PLIST"
    /usr/libexec/PlistBuddy -c "Add :SUPublicEDKey string $VIXEN_SPARKLE_PUBLIC_KEY" "$PLIST"
    echo "==> Automatic updates on (build $BUILD_NUMBER, feed $SPARKLE_FEED_URL)"
else
    echo "WARNING: no VIXEN_SPARKLE_PUBLIC_KEY: building with automatic updates off (build $BUILD_NUMBER)"
fi
# STUDIO-END sparkle-release-stamp

echo "==> Sign ($IDENTITY)"
# Inside out: helpers first, then the app. Hardened runtime and a secure timestamp are required for notarization.
for helper in "$APP"/Contents/Helpers/*; do
    codesign --force --options runtime --timestamp --sign "$IDENTITY" "$helper"
done
# STUDIO-BEGIN sparkle-release-sign: Sparkle's helpers inside out, then its framework (Sparkle's documented order)
SPARKLE="$APP/Contents/Frameworks/Sparkle.framework"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$SPARKLE/Versions/B/XPCServices/Installer.xpc"
codesign --force --options runtime --timestamp --sign "$IDENTITY" --preserve-metadata=entitlements "$SPARKLE/Versions/B/XPCServices/Downloader.xpc"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$SPARKLE/Versions/B/Autoupdate"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$SPARKLE/Versions/B/Updater.app"
codesign --force --options runtime --timestamp --sign "$IDENTITY" "$SPARKLE"
# STUDIO-END sparkle-release-sign
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
