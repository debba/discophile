# Discophile mobile downloads

All files in this release share the version shown in its title. The mobile build number and exact source commits are recorded in `RELEASE-SOURCES.json`; download checksums are in `SHA256SUMS.txt`.

## Android

`Discophile-VERSION-android.apk` is a signed release APK. Install it on Android 8 or later, enabling installation from the downloading app when Android asks. Updates must use the same package ID and signing key as the installed app.

Development/debug APKs use a different signing identity. If Android rejects an update because signatures differ, do not uninstall or clear app data just to bypass it: that can remove downloads, accounts and listening progress. Keep the existing installation until you have a migration/backup plan.

## iPhone

Discophile for iPhone is available on the App Store: https://apps.apple.com/app/discophile/id6825932680. Releases on GitHub contain no iPhone package.
