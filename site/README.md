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

## TestFlight requests

The **Try in TestFlight** buttons open a modal form that posts to `/api/testflight`,
which sends the request by SMTP with nodemailer. Copy `.env.example` to `.env.local`
for local work, and set the same variables in the Vercel project:

| Variable | Purpose |
|----------|---------|
| `SMTP_HOST`, `SMTP_PORT` | SMTP server (port 587 by default) |
| `SMTP_SECURE` | `true` for implicit TLS (port 465), otherwise STARTTLS |
| `SMTP_USER`, `SMTP_PASS` | SMTP credentials, if the server needs them |
| `SMTP_FROM` | Sender of the notification email |
| `TESTFLIGHT_TO` | Where requests are delivered; replies go to the requester |

Without `SMTP_HOST`, `SMTP_FROM` and `TESTFLIGHT_TO` the endpoint answers 503. Spam
protection is a hidden honeypot field plus a per-IP limit of 5 requests every 10
minutes, kept in memory per server instance.

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
every page, with the TestFlight dialog open and showing errors, and with the cookie
banner visible. Keyboard checks cover the skip link, the menu (Escape closes it), the
dialog (focus moves to the first field, then to the first invalid one, then back to
the button that opened it) and the banner. Text links use `--link`, since
`--raspberry` is only 4:1 on the rose background.

`tools/og-source.html` is the source of `public/assets/og.png`; open it in a browser
at 1200x630 to capture it.

## Client releases and mobile installation

`lib/releases.ts` reads the latest published GitHub release. It accepts only nonempty assets with the exact release version and this repository's GitHub download URLs, ignoring drafts, prereleases and malformed metadata. Desktop-only releases remain usable without inventing mobile links. The Linux dialog offers Debian, AppImage and tarball assets named `discophile_VERSION_amd64.deb`, `Discophile-VERSION-x86_64.AppImage` and `Discophile-VERSION-linux-x86_64.tar.gz`.

The Download page and shared download dialog also support `Discophile-VERSION-android.apk` and `Discophile-VERSION-ios-unsigned.ipa`. Both English and Italian include installation guides. iPhone/Android detection selects the matching tab from the home-page action; iPadOS reporting macOS is recognized by its touch support. The five-tab picker scrolls on narrow screens and supports arrow keys, Home and End. Missing mobile assets show explicit availability text instead of a file download action.

The public IPA is unsigned for sideloading while App Store approval is pending. The guide links to the official Sideloadly and AltStore Classic instructions, explains the computer/Apple Account requirement and typical free-account renewal period, and warns about keeping the signing identity/bundle identifier to preserve data. It does not collect Apple credentials or profiles. Android guidance explains installation-source permissions and signing-key conflicts without recommending uninstalling or disabling Play Protect. The same guides remain available after starting a mobile download. TestFlight invitation requests remain a secondary option, not a substitute for the IPA.

The coordinated mobile release workflow is documented in [Unified releases](../docs/MOBILE_RELEASES.md). Publish an approved release before rebuilding the production site so static pages immediately contain the new assets; otherwise the hourly revalidation picks them up. These website edits do not publish a release, upload artifacts or deploy the site.

Validation for this change: 13 automated tests passed, along with `npm run typecheck` and `npm run build` (Next.js 16.4.2). Tests cover matching-version mobile assets, stale/invalid metadata and URLs, older desktop-only releases, failed fetches, device detection, translated mobile cards/guides and server-rendered download pages. No browser, keyboard/screen-reader or device installation review was performed for this website change; earlier accessibility audit results above do not cover the new UI.

## Connect

Home and Download present optional desktop Connect plans and link to the Connect service for activation by email. Keep the plan copy, FAQ and privacy pages in both languages consistent with the deployed service. iPhone and browser Connect integration is not advertised as available.
