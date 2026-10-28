#!/usr/bin/env python3
"""Check or synchronize client versions from the iOS source plist. Never publishes."""
import argparse
import json
import plistlib
import re
import tomllib
from pathlib import Path

VERSION = re.compile(r"(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)")


def ios_version(root):
    info = plistlib.loads((root / "ios/Info.plist").read_bytes())
    version, build = info["CFBundleShortVersionString"], info["CFBundleVersion"]
    if not isinstance(version, str) or not VERSION.fullmatch(version):
        raise ValueError("iOS version must be three integers (major.minor.patch), without a prerelease suffix")
    if not isinstance(build, str) or not re.fullmatch(r"[1-9][0-9]{0,3}", build):
        raise ValueError("Shared iOS/Android build must be an integer from 1 to 9999")
    return version, int(build)


def replace_one(text, pattern, value):
    result, count = re.subn(pattern, lambda m: m[1] + value + m[2], text, flags=re.MULTILINE)
    if count != 1:
        raise ValueError(f"Expected one version field matching {pattern!r}, found {count}")
    return result


def expected_files(desktop, android, version, build):
    changes = {}
    cargo = desktop / "Cargo.toml"
    changes[cargo] = replace_one(cargo.read_text(), r'(^version = ")[^"\n]+("$)', version)
    lock = desktop / "Cargo.lock"
    content = lock.read_text()
    for name in ("discophile", "discophile-core"):
        content = replace_one(content, rf'(\[\[package\]\]\nname = "{name}"\nversion = ")[^"\n]+(")', version)
    changes[lock] = content
    gradle = android / "app/build.gradle.kts"
    content = replace_one(gradle.read_text(), r'(^\s*versionName = ")[^"\n]+("$)', version)
    changes[gradle] = replace_one(content, r'(^\s*versionCode = )[0-9]+(\s*$)', str(build))
    chart = desktop / "deploy/helm/discophile-web/Chart.yaml"
    content = replace_one(chart.read_text(), r'(^version: )[^\n]+($)', version)
    changes[chart] = replace_one(content, r'(^appVersion: ")[^"\n]+("$)', version)
    return changes


def check(desktop, ios, android, tag=None, sync=False):
    version, build = ios_version(ios)
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"Tag {tag} does not match iOS v{version}")
    if sync:
        text = (android / "app/build.gradle.kts").read_text()
        current = re.search(r"(?m)^\s*versionCode = ([0-9]+)", text)
        if not current or int(current[1]) > build:
            raise ValueError("Refusing to lower Android versionCode; increment the iOS build first")
    changes = expected_files(desktop, android, version, build)
    mismatches = [path for path, text in changes.items() if path.read_text() != text]
    if mismatches and not sync:
        raise ValueError("Versions differ from iOS; run --sync before committing: " + ", ".join(map(str, mismatches)))
    for path in mismatches:
        path.write_text(changes[path])
    # Parse the synchronized manifests as well as checking their textual fields.
    assert tomllib.loads((desktop / "Cargo.toml").read_text())["workspace"]["package"]["version"] == version
    return {"version": version, "build": build}


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--desktop-root", type=Path, default=root)
    parser.add_argument("--ios-root", type=Path, default=root.parent / "ios-app")
    parser.add_argument("--android-root", type=Path, default=root.parent / "android-app")
    parser.add_argument("--tag")
    parser.add_argument("--sync", action="store_true")
    parser.add_argument("--github-output", type=Path)
    args = parser.parse_args()
    try:
        result = check(args.desktop_root, args.ios_root, args.android_root, args.tag, args.sync)
    except (ValueError, KeyError, OSError, plistlib.InvalidFileException) as error:
        parser.exit(1, f"Version check failed: {error}\n")
    if args.github_output:
        with args.github_output.open("a") as output:
            for key, value in result.items():
                output.write(f"{key}={value}\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
