# Discophile mobile downloads

All files in this release share the version shown in its title. The mobile build number and exact source commits are recorded in `RELEASE-SOURCES.json`; download checksums are in `SHA256SUMS.txt`.

## Android

`Discophile-VERSION-android.apk` is a signed release APK. Install it on Android 8 or later, enabling installation from the downloading app when Android asks. Updates must use the same package ID and signing key as the installed app.

Development/debug APKs use a different signing identity. If Android rejects an update because signatures differ, do not uninstall or clear app data just to bypass it: that can remove downloads, accounts and listening progress. Keep the existing installation until you have a migration/backup plan.

## Android 1.0.3 signing transition

Android 1.0.3 (versionCode 10) introduces a new persistent signing certificate. It cannot update an installed APK signed with the older release or development key. New installations can install the APK normally; future updates will keep the new signing identity.

Before removing an older installation:

1. Verify that any listening progress you want to keep has reached your server. Progress remaining only on the device may be lost.
2. Keep your server addresses, account credentials and any Connect recovery code needed to reconnect. Configuration synchronization does not back up downloaded audio.
3. Download the new APK. If you accept losing app-local data, uninstall the older Discophile app, install 1.0.3, reconnect your accounts and download media again. Uninstalling removes local accounts/settings, downloads and unsynchronized progress.

The new release certificate SHA-256 is `8a3a7b91302ebac1e43ef96a40eb3c23a5e2b1a002d802adeea4a52b8e19ad02`. Signing continuity starts again with 1.0.3; this certificate is different from the earlier release certificate `5eaf05f851d0f30125ed8911c663595d5b29b5152cb75c508c32a53354e23549`. The change is intentional and is not an in-place key rotation.

## iPhone

Discophile for iPhone is available on the App Store: https://apps.apple.com/app/discophile/id6825932680. Releases on GitHub contain no iPhone package.
