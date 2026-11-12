# Discophile website

Next.js (App Router) site for Discophile at [discophile.app](https://discophile.app), deployed on Vercel from
this directory (the Vercel project's Root Directory is `site`).

The [design guide](DESIGN.md) documents the visual identity, shared tokens, components, and accessibility
rules for the website and related Discophile interfaces.

```sh
npm install
npm run dev        # http://localhost:3000
npm run build      # production build and type check
npm test           # release parsing, mobile detection and server-rendered download/guide regressions
```

## Languages

English lives at `/` and Italian at `/it`, as two route groups, `app/(en)` and `app/(it)`,
each with its own root layout so `<html lang>` is right. Interface strings are in
`lib/i18n.ts`, the home page copy in `components/HomePage.tsx`, and each legal page has
one file per language. Every page lists the other language with `hreflang`, and the
header links to the same page in the other language. Use the Italian wording of the
app (`../Localization/strings.json`) for feature names.

## iPhone app on the App Store

Discophile for iPhone is a free app on the App Store. Its ID and URL are `APP_STORE_ID` and `APP_STORE_URL`
in `lib/releases.ts`. The root metadata sets `itunes.appId`, so Safari on iPhone shows its Smart App Banner at the top
of every page, offering the App Store page or opening the installed app. Other iPhone browsers do not show it.

On an iPhone the main download button links straight to the App Store; elsewhere the dialog's iPhone tab, the
Download page and the FAQ link to it and state that it is free. App Store links are counted in Matomo as `App Store` events.
The unsigned IPA and the TestFlight request form, offered while approval was pending, have been removed, along with
`/api/testflight` and its SMTP settings. Never link an IPA or a TestFlight invitation again.

## Analytics and cookies

Matomo (site ID 9 on `analytics.debbaweb.it`) always runs cookieless. Measurement
cookies are set only after consent in the cookie banner, and are deleted when consent
is withdrawn from **Cookie preferences** in the footer. The choice is stored in local
storage as `discophile-cookie-consent`. Keep `/cookie-policy` in sync when anything
stored in the browser changes.

## Intro video

**Watch the video** links to `/video` (Italian: `/it/video`) using Next.js client-side
navigation, without reloading the document. The page keeps the full-screen vinyl
background, marquee, player, description and lyrics. Close or Escape returns to the
localized home with client-side navigation.

The player requests autoplay with sound on entry. A direct external visit may be
blocked by the browser's autoplay policy; native controls remain available. Both
routes include canonical, hreflang and social metadata. Reduced motion disables
animation, and decorative motion ends within four seconds.

The captions `<track>` joins the video only after its metadata has loaded. With the track
there from the start, iPhone Safari 27 stalled before loading any video, black at 00:00 or
with a crossed-out play button, while the same video without the track played, in a dialog
or not. The video element exists only while the dialog is open: the tap creates it with
its source and starts it with sound, and closing removes it, so it always starts from the
beginning. The MP4 revalidates on every load, like Vercel's default, instead of the
week-long cache of the other assets; the `?v=2` in its URL skips copies browsers kept
under the old header.

The video element never moves: the sweep and the jolt run on the grooves layer, and a
stand-in showing the poster (or the current frame when closing) flies between the button
and the stage. Keep animations off the video's ancestors.

`public/assets/video/discophile-intro.mp4` is `promo/video/out/discophile-fake-16x9.mp4`
remuxed with `-movflags +faststart`. The captions, `discophile-intro.en.vtt`, come
from `promo/data/audio/discophile.lrc`, and the poster is the closing frame.

## Accessibility

The cookie notice follows the footer in document flow, so it cannot obscure focused
controls. Footer and navigation controls have a minimum 44 px height. See
`../docs/VALIDATION.md` for the latest audit scope and remaining manual checks.


Checked with axe-core (WCAG 2.2 AA and best practices) at 1280 and 390 px widths, on
every page, with the former TestFlight dialog open and showing errors, and with the cookie
banner visible. Keyboard checks cover the skip link, the menu (Escape closes it), the
dialog (focus moves to the first field, then to the first invalid one, then back to
the button that opened it) and the banner. Text links use `--link`, since
`--raspberry` is only 4:1 on the rose background.

`tools/og-source.html` is the source of `public/assets/og.png`; open it in a browser
at 1200x630 to capture it.

## Client releases and mobile installation

`lib/releases.ts` reads the latest published GitHub release. It accepts only nonempty assets with the exact release version and this repository's GitHub download URLs, ignoring drafts, prereleases and malformed metadata. Desktop-only releases remain usable without inventing mobile links. The Linux dialog offers Debian, AppImage and tarball assets named `discophile_VERSION_amd64.deb`, `Discophile-VERSION-x86_64.AppImage` and `Discophile-VERSION-linux-x86_64.tar.gz`.

The Download page and shared download dialog also support `Discophile-VERSION-android.apk`, with installation guides in English and Italian. Any IPA in a release is ignored: the iPhone tab and section link to the App Store. iPhone/Android detection selects the matching tab from the home-page action; iPadOS reporting macOS is recognized by its touch support. The five-tab picker scrolls on narrow screens and supports arrow keys, Home and End. Missing mobile assets show explicit availability text instead of a file download action.

Android guidance explains installation-source permissions and signing-key conflicts without recommending uninstalling or disabling Play Protect. The same guide remains available after starting the APK download.

The coordinated mobile release workflow is documented in [Unified releases](../docs/MOBILE_RELEASES.md). Publish an approved release before rebuilding the production site so static pages immediately contain the new assets; otherwise the hourly revalidation picks them up. These website edits do not publish a release, upload artifacts or deploy the site.

Tests cover matching-version Android assets, ignored IPAs, stale/invalid metadata and URLs, older desktop-only releases, failed fetches, device detection, the App Store links and free availability on the Download page, FAQ and structured data, the iPhone download button, and server-rendered download pages.

## Connect

Home and Download present optional desktop Connect plans and link to the Connect service for activation by email. Keep the plan copy, FAQ and privacy pages in both languages consistent with the deployed service. iPhone and browser Connect integration is not advertised as available.
