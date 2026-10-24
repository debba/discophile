#!/usr/bin/env bash
# Wraps a Discophile binary in Discophile.app and a disk image, signed and notarized when credentials are set.
#
#   scripts/package-macos.sh <binary> <version> <output dir> [architecture label, default universal]
#
# Optional environment:
#   MACOS_SIGNING_IDENTITY   "Developer ID Application: …" (ad-hoc signature without it)
#   NOTARY_KEY_PATH, NOTARY_KEY_ID, NOTARY_ISSUER_ID
#                            App Store Connect API key for notarization (skipped without it)
set -euo pipefail

binary=$1
version=$2
out=$3
arch=${4:-universal}
root=$(cd "$(dirname "$0")/.." && pwd)

app="$out/Discophile.app"
dmg="$out/Discophile-$version-macos-$arch.dmg"
rm -rf "$app" "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Discophile"
cp "$root/crates/app/assets/macos/AppIcon.icns" "$app/Contents/Resources/AppIcon.icns"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleLocalizations</key><array><string>en</string><string>it</string></array>
  <key>CFBundleDisplayName</key><string>Discophile</string>
  <key>CFBundleName</key><string>Discophile</string>
  <key>CFBundleExecutable</key><string>Discophile</string>
  <key>CFBundleIdentifier</key><string>app.discophile.desktop</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
  <key>NSHumanReadableCopyright</key><string>© Andrea Debernardi</string>
</dict>
</plist>
PLIST

if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$app"
else
  echo "MACOS_SIGNING_IDENTITY is not set: ad-hoc signature, Gatekeeper will block the download" >&2
  codesign --force --sign - "$app"
fi
codesign --verify --strict --verbose=2 "$app"

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
cp -R "$app" "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "Discophile" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null

if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  codesign --force --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$dmg"
  if [[ -n "${NOTARY_KEY_PATH:-}" && -n "${NOTARY_KEY_ID:-}" && -n "${NOTARY_ISSUER_ID:-}" ]]; then
    xcrun notarytool submit "$dmg" --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID" --wait
    xcrun stapler staple "$dmg"
    spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
  else
    echo "Notary credentials are not set: the disk image is signed but not notarized" >&2
  fi
fi
echo "$dmg"
