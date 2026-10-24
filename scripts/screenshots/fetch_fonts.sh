#!/usr/bin/env bash
# Downloads the OFL fonts used by covers.py into .screenshots/fonts.
set -euo pipefail
out="$(cd "$(dirname "$0")/../.." && pwd)/.screenshots/fonts"
mkdir -p "$out"
base=https://raw.githubusercontent.com/google/fonts/main/ofl
fetch() { curl -sfL "$base/$1" -o "$out/$2"; }
fetch instrumentserif/InstrumentSerif-Regular.ttf InstrumentSerif-Regular.ttf
fetch instrumentserif/InstrumentSerif-Italic.ttf InstrumentSerif-Italic.ttf
fetch archivoblack/ArchivoBlack-Regular.ttf ArchivoBlack-Regular.ttf
fetch dmserifdisplay/DMSerifDisplay-Regular.ttf DMSerifDisplay-Regular.ttf
fetch dmserifdisplay/DMSerifDisplay-Italic.ttf DMSerifDisplay-Italic.ttf
fetch "unbounded/Unbounded%5Bwght%5D.ttf" Unbounded.ttf
fetch bebasneue/BebasNeue-Regular.ttf BebasNeue-Regular.ttf
fetch "inter/Inter%5Bopsz,wght%5D.ttf" Inter.ttf
echo "fonts in $out"
