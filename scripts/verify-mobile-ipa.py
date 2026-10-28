#!/usr/bin/env python3
"""Verify release IPA identity/version and reject bundled private provisioning profiles."""
import argparse
import plistlib
import zipfile
from pathlib import PurePosixPath


def verify(path, version, build):
    with zipfile.ZipFile(path) as ipa:
        entries = ipa.namelist()
        if any(PurePosixPath(name).name == "embedded.mobileprovision" for name in entries):
            raise ValueError("Public sideload IPA must not contain a device-bound provisioning profile")
        info = plistlib.loads(ipa.read("Payload/Discophile.app/Info.plist"))
        expected = {"CFBundleIdentifier": "it.debba.discophile", "CFBundleShortVersionString": version,
                    "CFBundleVersion": str(build), "CFBundlePackageType": "APPL"}
        for key, value in expected.items():
            if info.get(key) != value:
                raise ValueError(f"IPA {key}: expected {value!r}, found {info.get(key)!r}")
        executable = info.get("CFBundleExecutable")
        if not executable or f"Payload/Discophile.app/{executable}" not in entries:
            raise ValueError("IPA has no app executable")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ipa")
    parser.add_argument("version")
    parser.add_argument("build")
    args = parser.parse_args()
    try:
        verify(args.ipa, args.version, args.build)
    except (ValueError, OSError, KeyError, zipfile.BadZipFile) as error:
        parser.exit(1, f"IPA verification failed: {error}\n")
    print(f"Verified public sideload IPA {args.version} ({args.build})")


if __name__ == "__main__":
    main()
