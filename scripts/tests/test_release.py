import importlib.util
import plistlib
import tempfile
import unittest
import zipfile
from pathlib import Path

SCRIPTS = Path(__file__).resolve().parents[1]


def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


versions = load("release_version", "release-version.py")
ipa_check = load("verify_mobile_ipa", "verify-mobile-ipa.py")


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        root = Path(self.temp.name)
        self.desktop, self.ios, self.android = [root / name for name in ("desktop", "ios", "android")]
        self.plist = self.ios / "ios/Info.plist"
        files = {
            self.desktop / "Cargo.toml": '[workspace.package]\nversion = "0.1.8"\n',
            self.desktop / "Cargo.lock": '[[package]]\nname = "discophile"\nversion = "0.1.8"\n\n[[package]]\nname = "discophile-core"\nversion = "0.1.8"\n',
            self.desktop / "deploy/helm/discophile-web/Chart.yaml": 'version: 0.1.0\nappVersion: "0.1.0"\n',
            self.android / "app/build.gradle.kts": 'defaultConfig {\n    versionCode = 6\n    versionName = "0.6.0"\n}\n',
        }
        for path, content in files.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(content)
        self.plist.parent.mkdir(parents=True)
        self.set_ios()

    def set_ios(self, version="1.0.1", build="7"):
        self.plist.write_bytes(plistlib.dumps({"CFBundleShortVersionString": version, "CFBundleVersion": build}))

    def check(self, **kwargs):
        return versions.check(self.desktop, self.ios, self.android, **kwargs)

    def test_mismatch_fails_without_editing_sources(self):
        before = {p: p.read_bytes() for p in Path(self.temp.name).rglob("*") if p.is_file()}
        with self.assertRaisesRegex(ValueError, "Versions differ"):
            self.check()
        self.assertEqual(before, {p: p.read_bytes() for p in before})

    def test_sync_uses_ios_for_every_manifest_and_is_idempotent(self):
        source = self.plist.read_bytes()
        self.assertEqual(self.check(sync=True), {"version": "1.0.1", "build": 7})
        self.assertEqual(self.check(tag="v1.0.1"), self.check(sync=True))
        self.assertEqual(self.plist.read_bytes(), source)
        self.assertEqual((self.desktop / "Cargo.lock").read_text().count('version = "1.0.1"'), 2)
        self.assertIn("versionCode = 7", (self.android / "app/build.gradle.kts").read_text())

    def test_tag_must_match_ios(self):
        with self.assertRaisesRegex(ValueError, "does not match iOS"):
            self.check(tag="v0.1.8", sync=True)

    def test_ios_rejects_prerelease_leading_zero_and_partial_versions(self):
        for value in ("1.0", "1.0.1-beta.1", "01.0.1", "1.0.1+build", "1.0.1\n"):
            with self.subTest(value=value):
                self.set_ios(version=value)
                with self.assertRaises(ValueError):
                    self.check(sync=True)

    def test_ios_build_is_a_shared_monotonic_integer(self):
        for build in ("0", "01", "7.1", "10000", "7-beta"):
            with self.subTest(build=build):
                self.set_ios(build=build)
                with self.assertRaises(ValueError):
                    self.check(sync=True)
        self.set_ios(build="5")
        with self.assertRaisesRegex(ValueError, "Refusing to lower"):
            self.check(sync=True)

    def test_missing_or_duplicate_manifest_fields_fail_closed(self):
        gradle = self.android / "app/build.gradle.kts"
        gradle.write_text(gradle.read_text() + 'versionName = "duplicate"\n')
        with self.assertRaisesRegex(ValueError, "Expected one version field"):
            self.check(sync=True)
        self.assertIn('version = "0.1.8"', (self.desktop / "Cargo.toml").read_text())


class IpaTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / "Discophile.ipa"

    def make_ipa(self, version="1.0.1", build="7", bundle="it.debba.discophile", profile=False, executable=True):
        info = {"CFBundleIdentifier": bundle, "CFBundleShortVersionString": version, "CFBundleVersion": build,
                "CFBundlePackageType": "APPL", "CFBundleExecutable": "Discophile"}
        with zipfile.ZipFile(self.path, "w") as ipa:
            ipa.writestr("Payload/Discophile.app/Info.plist", plistlib.dumps(info))
            if executable:
                ipa.writestr("Payload/Discophile.app/Discophile", b"test executable")
            if profile:
                ipa.writestr("Payload/Discophile.app/embedded.mobileprovision", b"device-bound fixture")

    def test_accepts_expected_unsigned_sideload_identity(self):
        self.make_ipa()
        ipa_check.verify(self.path, "1.0.1", 7)

    def test_rejects_wrong_version_build_or_prefixed_bundle(self):
        for kwargs in ({"version": "0.1.8"}, {"build": "6"}, {"bundle": "XTL-example.it.debba.discophile"}):
            with self.subTest(kwargs=kwargs):
                self.make_ipa(**kwargs)
                with self.assertRaises(ValueError):
                    ipa_check.verify(self.path, "1.0.1", 7)

    def test_rejects_publishing_device_profiles(self):
        self.make_ipa(profile=True)
        with self.assertRaisesRegex(ValueError, "provisioning profile"):
            ipa_check.verify(self.path, "1.0.1", 7)

    def test_rejects_missing_executable(self):
        self.make_ipa(executable=False)
        with self.assertRaisesRegex(ValueError, "no app executable"):
            ipa_check.verify(self.path, "1.0.1", 7)


if __name__ == "__main__":
    unittest.main()
