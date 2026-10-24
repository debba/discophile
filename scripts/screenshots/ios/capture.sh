#!/usr/bin/env bash
# Captures the website's iPhone screenshots in light and dark from the iOS app (../ios-app) in the simulator,
# against the demo servers of scripts/screenshots, and converts them into site/public/assets/screens.
#   scripts/screenshots/ios/capture.sh [simulator id]
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
IOS="$ROOT/../ios-app"
SIM="${1:-$(xcrun simctl list devices available | grep -m1 -oE 'iPhone 17 \(([0-9A-F-]+)\)' | grep -oE '[0-9A-F-]{36}')}"
PROJECT="$IOS/.cache/simulator-project"
OUT="${SHOT_ROOT:-/tmp/iosshots}"

python3 "$ROOT/scripts/screenshots/fixture_user.py"
python3 "$ROOT/scripts/screenshots/book_progress.py" fixture-user fixture-password

# The App Store staging project, with the validation bundle ID (fixture flags work only there) and a UI test target.
sed 's|".cache/appstore-project"|".cache/simulator-project"|' "$IOS/scripts/prepare-appstore.py" > /tmp/prepare-simulator.py
python3 /tmp/prepare-simulator.py >/dev/null
python3 - "$PROJECT/appstore/project.yml" "$HERE" <<'PY'
import sys
p, tests = sys.argv[1], sys.argv[2]
s = open(p).read().replace("PRODUCT_BUNDLE_IDENTIFIER: it.debba.discophile\n", "PRODUCT_BUNDLE_IDENTIFIER: it.debba.discophile.validation\n")
s = s.replace("schemes:\n", f"""  Screenshots:
    type: bundle.ui-testing
    platform: iOS
    sources:
      - {tests}
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: it.debba.discophile.screenshots
        GENERATE_INFOPLIST_FILE: YES
schemes:
""").replace("        Discophile: all\n", "        Discophile: all\n    test:\n      targets:\n        - Screenshots\n", 1)
open(p, "w").write(s)
PY
cd "$PROJECT"
xcodegen generate --spec appstore/project.yml --project appstore >/dev/null
xcrun simctl boot "$SIM" 2>/dev/null || true
xcrun simctl spawn "$SIM" defaults write -g AppleLanguages -array en
# Ad-hoc signing gives the app its keychain identity in the simulator; unsigned builds can't keep the Audiobookshelf token.
SIGN=(CODE_SIGN_IDENTITY=- CODE_SIGN_STYLE=Manual DEVELOPMENT_TEAM=)
xcodebuild -project appstore/DiscophileAppStore.xcodeproj -scheme Discophile -configuration Debug -destination "id=$SIM" -derivedDataPath build "${SIGN[@]}" build >/dev/null
xcrun simctl install "$SIM" build/Build/Products/Debug-iphonesimulator/Discophile.app

for mode in dark light; do
  xcrun simctl ui "$SIM" appearance $mode
  xcrun simctl status_bar "$SIM" override --time 9:41 --dataNetwork wifi --wifiMode active --wifiBars 3 --cellularMode active --cellularBars 4 --batteryState charged --batteryLevel 100
  SHOT_DIR="$OUT/$mode" TEST_RUNNER_SHOT_DIR="$OUT/$mode" xcodebuild -project appstore/DiscophileAppStore.xcodeproj -scheme Discophile \
    -destination "id=$SIM" -derivedDataPath build "${SIGN[@]}" test -only-testing:Screenshots/SiteScreenshots | grep -E "Test Case .*(passed|failed)"
done

# Website files: 645 x 1398 WebP, dark in screens/, light in screens/light/. Needs Pillow.
python3 - "$OUT" "$ROOT/site/public/assets/screens" <<'PY'
import sys
from PIL import Image
src, dest = sys.argv[1], sys.argv[2]
NAMES = {"listen-now": "listen-now", "album": "album", "now-playing": "now-playing", "library-albums": "library-albums",
         "library-artists": "library-artists", "search": "search", "statistics-top": "statistics", "statistics": "statistics-charts",
         "audiobook": "audiobook", "audiobook-player": "audiobook-player", "ai-local": "ai-local", "ai-chatgpt": "ai-chatgpt",
         "ai-chatgpt-songs": "ai-chatgpt-songs"}
for mode, folder in (("dark", dest), ("light", f"{dest}/light")):
    for shot, name in NAMES.items():
        image = Image.open(f"{src}/{mode}/{shot}.png").convert("RGB")
        image = image.resize((645, round(image.height * 645 / image.width)), Image.LANCZOS).crop((0, 0, 645, 1398))
        image.save(f"{folder}/{name}.webp", "WEBP", quality=86, method=6)
print("saved", len(NAMES) * 2, "screenshots")
PY
