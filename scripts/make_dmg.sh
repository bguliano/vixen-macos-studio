#!/bin/bash
# Wraps an already signed and notarized Vixen.app in a drag-to-Applications disk image, then signs, notarizes and
# staples the image itself. scripts/package_release.sh calls this; it can also be run on its own.
#
#   VIXEN_SIGN_IDENTITY="Developer ID Application: <Team Name> (<TEAMID>)" \
#   VIXEN_NOTARY_PROFILE=<notarytool keychain profile> \
#   scripts/make_dmg.sh <path to Vixen.app> <output .dmg>
set -euo pipefail
APP="${1:?usage: scripts/make_dmg.sh <Vixen.app> <output.dmg>}"
DMG="${2:?usage: scripts/make_dmg.sh <Vixen.app> <output.dmg>}"
IDENTITY="${VIXEN_SIGN_IDENTITY:?set VIXEN_SIGN_IDENTITY to a Developer ID Application identity}"
PROFILE="${VIXEN_NOTARY_PROFILE:?set VIXEN_NOTARY_PROFILE to a notarytool keychain profile}"
STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

echo "==> Disk image"
ditto "$APP" "$STAGE/Vixen Studio.app"  # STUDIO: Studio's app name
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -quiet -volname "Vixen Studio" -srcfolder "$STAGE" -fs APFS -format ULFO -ov "$DMG"  # STUDIO: Studio's volume name
codesign --force --timestamp --sign "$IDENTITY" "$DMG"

echo "==> Notarize disk image"
xcrun notarytool submit "$DMG" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$DMG"
xcrun stapler validate "$DMG"
spctl --assess --type open --context context:primary-signature --verbose=2 "$DMG"
( cd "$(dirname "$DMG")" && shasum -a 256 "$(basename "$DMG")" > "$(basename "$DMG").sha256" )
echo "Built $DMG"
