# Unified desktop, web, Android and iPhone releases

## One version, owned by iOS

`discophile-ios/ios/Info.plist` is the source of truth:

- `CFBundleShortVersionString`: three integers, `major.minor.patch`, without `v`, prerelease suffixes, build metadata or leading zeroes. This becomes the Cargo workspace version, Android `versionName`, Helm chart version/appVersion and GitHub `vX.Y.Z` tag.
- `CFBundleVersion`: a positive integer from 1 to 9999 for this shared scheme, also used as Android `versionCode`. Increase it for every distributed mobile update. The synchronizer refuses to lower Android's code. Desktop/web have no corresponding numeric mobile build field.

From the desktop repository, with the usual sibling checkouts (`../ios-app`, `../android-app`):

```sh
python3 scripts/release-version.py --sync
python3 scripts/release-version.py
python3 -m unittest discover -s scripts/tests
```

The synchronizer updates Cargo.toml, both workspace package versions in Cargo.lock, Android's source build configuration and the Helm chart. It never changes the iOS source or publishes anything. Alternate paths are accepted through `--ios-root`, `--android-root` and `--desktop-root`. Source snapshots and release artifacts must agree; CI never silently stamps a different version into a mobile checkout.

Current alignment is **1.0.2**, mobile build **8**. The website and Connect service are not player clients and retain their independent package/deployment versions.

## Before enabling the workflow

The two mobile repositories are private. Configure these secrets on `debba/discophile` through GitHub's secret-management UI or an approved secure channel, never in source, an issue, a commit or chat:

- `MOBILE_REPOS_TOKEN`: a fine-grained token with **Contents: read** on `debba/discophile-ios` and `debba/discophile-android`. The default workflow token cannot read the other private repositories. All such checkouts use `persist-credentials: false`.
- `ANDROID_KEYSTORE_BASE64`: the single-line base64 representation of the persistent Android release keystore.
- `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS`, `ANDROID_KEY_PASSWORD`: its signing parameters.

Back up the Android key securely; changing it prevents ordinary updates to previously signed APKs. Release builds fail if signing configuration is absent and never fall back to the debug keystore. Debug builds are unaffected. Locally, release Gradle tasks accept `ANDROID_KEYSTORE_PATH` plus the three signing parameters above. No secrets have been configured by the source changes described here.

The public IPA is intentionally **unsigned for sideloading**. CI archives the existing iPhone-only XcodeGen project without signing and rejects any embedded provisioning profile, wrong bundle ID or version. It publishes no device IDs, Apple development certificates or development profiles. App Store/TestFlight distribution remains a separate signed export/upload procedure. See [mobile installation guidance](../packaging/mobile/INSTALLATION.md).

The iOS job uses GitHub's `xcode-27` label because the source requires the iOS 27 SDK. It fails clearly if an older SDK is selected. Runner availability/capacity and actual Xcode archive success still require a real CI run; local Linux checks cannot validate them. The desktop macOS job uses `macos-15`.

## Release procedure (only after device approval)

1. Finish and review changes in all three client repositories. Change the iOS version/build, synchronize the siblings, and run their tests/builds.
2. Install development builds on Android/iPhone and obtain approval before publishing. Preserve existing application identifiers and signing identities; do not uninstall apps to fix a signing mismatch.
3. Commit and push each repository's reviewed changes. Create and push the **same** `vX.Y.Z` tag in both mobile repositories first. Never move a published tag.
4. Add reviewed release notes as `docs/releases/X.Y.Z.md` in the desktop repository, then commit and push the desktop changes and matching tag. That tag starts the unified workflow.
5. The preflight checks all source versions against iOS and the tag, then resolves the mobile tags once into immutable commit SHAs. Build jobs use those SHAs, not moving branches. `RELEASE-SOURCES.json` records all three source commits and the common version/build.
6. Publishing waits for macOS, Windows, Linux, Android, iPhone, web image and Helm builds. It requires every expected desktop/Helm/APK/IPA asset plus installation guidance and provenance, calculates SHA-256 checksums, uploads into a **draft** GitHub release, then makes it public only after upload succeeds. A failed upload can leave a draft requiring inspection before retry. Container/chart pushes still happen in their tag-triggered build jobs, as before; GitHub draft gating is not an atomic deployment of those registries.

Manual `workflow_dispatch` takes a `mobile_ref` (default `main`) that must exist in both mobile repositories. It performs the same checks and builds downloadable workflow artifacts, but does not create a GitHub release, push container/chart tags or update AUR. Required mobile access/signing secrets are checked before build jobs start even for manual runs.

## Locally built mobile packages

The owner requested locally built IPA and APK packages for the current release when CI mobile signing and repository access are not configured. Preserve the persistent Android release signing key and make a secure backup. A release APK signed with a new key cannot update an installation signed with the development key. The public IPA must be unsigned and must not include a provisioning profile.

For this fallback, check all three source versions and tags locally before building. Set the repository variable `DISCOPHILE_LOCAL_MOBILE` to `true` before pushing the desktop tag. The workflow then builds the desktop and Helm artifacts without mobile access and does not publish a GitHub release. After all required desktop and Helm jobs succeed, collect their workflow artifacts, add the locally verified mobile packages, installation guide and a `RELEASE-SOURCES.json` identifying the three tagged commits and shared version/build, and publish them together as a draft before making it public. Reset the variable after the workflow run so later releases use the normal unified checks. This fallback does not update AUR automatically.

Do not trigger a workflow or push a tag merely to test these source changes. Local tooling tests cover synchronization/idempotence, strict iOS versions, matching tags, nondecreasing Android builds, missing manifest fields, IPA identity/version and exclusion of private provisioning profiles. Live private-repository access, release signing, the Xcode archive and publishing require configured CI and explicit release approval.
