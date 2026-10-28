# Discophile mobile downloads

All files in this release share the version shown in its title. The mobile build number and exact source commits are recorded in `RELEASE-SOURCES.json`; download checksums are in `SHA256SUMS.txt`.

## Android

`Discophile-VERSION-android.apk` is a signed release APK. Install it on Android 8 or later, enabling installation from the downloading app when Android asks. Updates must use the same package ID and signing key as the installed app.

Development/debug APKs use a different signing identity. If Android rejects an update because signatures differ, do not uninstall or clear app data just to bypass it: that can remove downloads, accounts and listening progress. Keep the existing installation until you have a migration/backup plan.

## iPhone

`Discophile-VERSION-ios-unsigned.ipa` is an unsigned, arm64 device IPA for iOS 17 or later. It cannot be installed by tapping the download. Use an iOS sideloading tool and your own Apple signing account to sign and provision it. Trust/Developer Mode requirements and renewal limits depend on that signing method. Siri and other restricted entitlements require a profile authorized for those capabilities. This iPhone-only package does not embed the optional Watch companion.

The public IPA contains no private certificate or device-bound provisioning profile, and is not an App Store or TestFlight upload. Keep the same signing identity and bundle identifier when updating an existing installation; a sideloading tool that changes the bundle ID installs a separate app rather than retaining its data. Do not apply an identifier prefix twice to an already signed local development IPA.
