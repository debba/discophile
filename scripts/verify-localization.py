#!/usr/bin/env python3
"""Lists interface keys used in Rust sources that have no Italian translation."""
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
translations = {}
for path in sorted((ROOT / "Localization").glob("*.json")):
    if path.name == "plurals.json":
        translations.update({k: v for k, v in json.loads(path.read_text()).items()})
        continue
    translations.update(json.loads(path.read_text()))

pattern = re.compile(r'\b(?:t|tf|count|l10n::t|l10n::tf|l10n::count)\(\s*"((?:[^"\\]|\\.)*)"')
missing = {}
for source in ROOT.glob("crates/**/*.rs"):
    if source.name == "l10n.rs":
        continue
    text = source.read_text()
    for match in pattern.finditer(text):
        key = bytes(match.group(1), "utf-8").decode("unicode_escape").encode("latin-1").decode("utf-8")
        if key not in translations:
            line = text.count("\n", 0, match.start()) + 1
            missing.setdefault(key, f"{source.relative_to(ROOT)}:{line}")

for key, where in sorted(missing.items()):
    print(f"{where}\t{key}")
print(f"{len(missing)} untranslated keys", file=sys.stderr)
sys.exit(1 if missing else 0)
