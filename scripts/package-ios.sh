#!/usr/bin/env bash
# Builds an unsigned, device-architecture IPA for sideloading. No provisioning profiles are published.
# Usage: bash scripts/package-ios.sh IOS_SOURCE VERSION BUILD OUTPUT_DIR
set -euo pipefail
source_root=$(cd "${1:?Missing iOS source}" && pwd)
version=${2:?Missing version}
build=${3:?Missing build}
mkdir -p "${4:?Missing output directory}"
out=$(cd "$4" && pwd)
scripts=$(cd "$(dirname "$0")" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

sdk=$(xcrun --sdk iphoneos --show-sdk-version)
[[ ${sdk%%.*} -ge 27 ]] || { echo 'The iOS sources require the Xcode 27 / iOS 27 SDK runner.' >&2; exit 1; }
xcodebuild -version
python3 "$source_root/scripts/generate-localizations.py"
python3 "$source_root/scripts/prepare-appstore.py"
staged="$source_root/.cache/appstore-project"
xcodegen generate --spec "$staged/appstore/project.yml" --project "$staged/appstore"
xcodebuild -project "$staged/appstore/DiscophileAppStore.xcodeproj" -scheme Discophile \
  -configuration Release -sdk iphoneos -destination 'generic/platform=iOS' \
  -archivePath "$work/Discophile.xcarchive" archive \
  CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO CODE_SIGN_IDENTITY=''
mkdir -p "$work/Payload"
cp -R "$work/Discophile.xcarchive/Products/Applications/Discophile.app" "$work/Payload/"
ipa="$out/Discophile-$version-ios-unsigned.ipa"
ditto -c -k --sequesterRsrc --keepParent "$work/Payload" "$ipa"
python3 "$scripts/verify-mobile-ipa.py" "$ipa" "$version" "$build"
echo "$ipa"
