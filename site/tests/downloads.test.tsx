import assert from "node:assert/strict";
import { afterEach, mock, test } from "node:test";
import { renderToStaticMarkup } from "react-dom/server";
import { DownloadButton, DownloadProvider } from "../components/Download";
import { MobileDownload } from "../components/MobileDownload";
import { MobileInstallGuide } from "../components/MobileInstallGuide";
import { DownloadPage } from "../components/DownloadPage";
import { FaqPage } from "../components/FaqPage";
import { StructuredData } from "../components/StructuredData";
import { PhoneShot } from "../components/DesktopShot";
import { detectPlatform } from "../lib/platform";
import { APP_STORE_URL, latestRelease, parseRelease, REPO_URL, type Release } from "../lib/releases";
import { ui } from "../lib/i18n";

const asset = (name: string, tag = "v1.0.1") => ({ name, browser_download_url: `${REPO_URL}/releases/download/${tag}/${name}`, size: 24_000_000 });
const payload = (assets: unknown[] = []) => ({ tag_name: "v1.0.1", html_url: `${REPO_URL}/releases/tag/v1.0.1`, draft: false, prerelease: false, assets });
// Release 1.0.2 shipped an unsigned IPA before the App Store launch; the site must never offer it.
const mobileAssets = [asset("Discophile-1.0.1-android.apk"), asset("Discophile-1.0.1-ios-unsigned.ipa")];
const release = parseRelease(payload(mobileAssets)) as Release;
afterEach(() => mock.restoreAll());

test("only screenshots with partially captured sheet controls are trimmed", () => {
  for (const name of ["now-playing", "ai-local", "ai-chatgpt", "ai-chatgpt-songs"]) {
    assert.match(renderToStaticMarkup(<PhoneShot name={name} alt="" />), /data-trim-bottom="true"/);
  }
  assert.doesNotMatch(renderToStaticMarkup(<PhoneShot name="album" alt="" />), /data-trim-bottom/);
});

test("recognizes the APK from the published version and ignores an IPA", () => {
  assert.equal(release.version, "1.0.1");
  assert.equal(release.files.android?.name, mobileAssets[0].name);
  assert.deepEqual(Object.keys(release.files), ["android"]);
});

test("does not mix older mobile assets into a newer release or advertise debug APKs", () => {
  const result = parseRelease(payload([
    asset("Discophile-1.0.0-android.apk"), asset("Discophile-1.0.0-ios-unsigned.ipa"),
    asset("app-debug.apk"), asset("Discophile-1.0.1-ios.ipa"),
  ]));
  assert.deepEqual(result?.files, {});
});

test("older desktop-only releases still work without inventing mobile URLs", () => {
  const result = parseRelease(payload([asset("Discophile-1.0.1-macos-universal.dmg"), asset("discophile_1.0.1_amd64.deb")]));
  assert.ok(result?.files.mac);
  assert.ok(result?.files.deb);
  assert.equal(result?.files.android, undefined);
});

test("rejects drafts, prereleases and malformed release metadata", () => {
  for (const value of [null, {}, { ...payload(), draft: true }, { ...payload(), prerelease: true },
    { ...payload(), tag_name: "v1.0.1-beta" }, { ...payload(), assets: null }, { ...payload(), html_url: "https://example.com" }]) {
    assert.equal(parseRelease(value), null);
  }
});

test("ignores foreign asset URLs, empty files and non-finite sizes", () => {
  for (const invalid of [
    { ...mobileAssets[0], browser_download_url: "https://example.com/fake.apk" },
    { ...mobileAssets[0], size: 0 }, { ...mobileAssets[0], size: Number.NaN },
    { ...mobileAssets[0], browser_download_url: mobileAssets[0].browser_download_url.replace("v1.0.1", "v1.0.0") },
  ]) assert.deepEqual(parseRelease(payload([invalid]))?.files, {});
});

test("release fetch handles GitHub failures without a broken download link", async () => {
  mock.method(globalThis, "fetch", async () => new Response("rate limited", { status: 403 }));
  assert.equal(await latestRelease(), null);
});

test("release fetch handles invalid JSON and network failures", async () => {
  const fetch = mock.method(globalThis, "fetch", async () => new Response("not json", { status: 200 }));
  assert.equal(await latestRelease(), null);
  fetch.mock.mockImplementation(async () => { throw new Error("offline"); });
  assert.equal(await latestRelease(), null);
});

test("detects Android as mobile rather than Linux, and touch Macs as iPads", () => {
  const detect = (userAgent: string, maxTouchPoints = 0, platform?: string) => detectPlatform({ userAgent, maxTouchPoints, userAgentData: { platform } });
  assert.equal(detect("Mozilla/5.0 (Linux; Android 15; Pixel)", 5, "Linux"), "android");
  assert.equal(detect("Mozilla/5.0 (iPhone; CPU iPhone OS 17_0)", 5), "ios");
  assert.equal(detect("Mozilla/5.0 (Macintosh; Intel Mac OS X)", 5, "macOS"), "ios");
  assert.equal(detect("Mozilla/5.0 (Macintosh)", 0, "macOS"), "mac");
  assert.equal(detect("Mozilla/5.0 (Windows NT 10.0)", 0, "Windows"), "windows");
  assert.equal(detect("Mozilla/5.0 (X11; Linux x86_64)", 0), "linux");
  assert.equal(detect("unknown"), null);
});

test("mobile cards expose localized download actions only when the asset exists", () => {
  for (const locale of ["en", "it"] as const) {
    for (const platform of ["android"] as const) {
      const present = renderToStaticMarkup(<MobileDownload locale={locale} platform={platform} release={release} />);
      assert.ok(present.includes(ui[locale].download.buttons[platform]));
      assert.ok(present.includes(`href="${release.files[platform]!.url}"`));
      assert.ok(present.includes(release.files[platform]!.name));
      const missing = renderToStaticMarkup(<MobileDownload locale={locale} platform={platform} release={parseRelease(payload())} />);
      assert.doesNotMatch(missing, /class="button primary"/);
      assert.ok(missing.includes(locale === "it" ? "non è ancora" : "not in the latest"));
      assert.match(missing, /releases\/tag\/v1\.0\.1/);
    }
  }
});

test("download page links directly to each available Linux package", async () => {
  const linuxAssets = [asset("discophile_1.0.1_amd64.deb"), asset("Discophile-1.0.1-x86_64.AppImage"), asset("Discophile-1.0.1-linux-x86_64.tar.gz")];
  mock.method(globalThis, "fetch", async () => Response.json(payload([...mobileAssets, ...linuxAssets])));
  const html = renderToStaticMarkup(await DownloadPage({ locale: "it" }));
  for (const file of [mobileAssets[0], ...linuxAssets]) assert.ok(html.includes(`href="${file.browser_download_url}"`));
  assert.match(html, /Scarica \.deb/);
  assert.match(html, /Scarica AppImage/);
  assert.match(html, /Scarica \.tar\.gz/);
});

test("iPhone section sends visitors to the App Store, with the price, and never to an IPA or TestFlight", async () => {
  mock.method(globalThis, "fetch", async () => Response.json(payload(mobileAssets)));
  for (const locale of ["en", "it"] as const) {
    const html = renderToStaticMarkup(await DownloadPage({ locale }));
    const iphone = html.slice(html.indexOf('id="iphone"'), html.indexOf('id="android"'));
    assert.ok(iphone.includes(`class="button primary" href="${APP_STORE_URL}"`));
    assert.ok(iphone.includes(ui[locale].download.buttons.ios));
    assert.ok(iphone.includes(locale === "it" ? "2,99 €" : "€2.99"));
    assert.doesNotMatch(html, /\.ipa|TestFlight|sideload/i);
  }
});

test("on an iPhone the download button opens the App Store instead of the dialog", () => {
  const html = renderToStaticMarkup(<DownloadProvider locale="en" release={release}><DownloadButton platform="ios" labels={{ any: "Download" }} /></DownloadProvider>);
  const button = html.slice(0, html.indexOf("<dialog"));
  assert.ok(button.includes(`href="${APP_STORE_URL}"`));
  assert.ok(button.includes("Download on the App Store"));
  assert.doesNotMatch(button, /aria-haspopup/);
});

test("Android guide does not suggest turning off Play Protect or deleting app data", () => {
  const html = renderToStaticMarkup(<MobileInstallGuide locale="en" platform="android" />);
  assert.match(html, /do not disable Play Protect/);
  assert.match(html, /do not uninstall or clear app data/);
});

test("the shared download dialog exposes all five named platform tabs", () => {
  const html = renderToStaticMarkup(<DownloadProvider locale="it" release={release}><DownloadButton platform="ios" labels={{ any: "Download" }} /></DownloadProvider>);
  for (const platform of ["mac", "windows", "linux", "android", "ios"]) {
    assert.ok(html.includes(`id="download-tab-${platform}"`));
  }
  assert.match(html, /Scarica su App Store/);
});

test("download page includes mobile files and installation guides in both languages", async () => {
  mock.method(globalThis, "fetch", async () => Response.json(payload(mobileAssets)));
  for (const locale of ["en", "it"] as const) {
    const html = renderToStaticMarkup(await DownloadPage({ locale }));
    assert.match(html, /id="android"/);
    assert.match(html, /id="iphone"/);
    assert.match(html, /Discophile-1\.0\.1-android\.apk/);
    assert.doesNotMatch(html, /ios-unsigned/);
    assert.match(html, /<th scope="col">Android<\/th>/);
    assert.match(html, /data-label="Android"/);
    assert.match(html, /role="region" aria-label="[^"]+" tabindex="0"/);
    assert.match(html, locale === "it" ? /dispositiv[io] compatibil/ : /compatible (devices|phones)/);
  }
});

test("FAQ and search data state the iPhone price and link the App Store", async () => {
  mock.method(globalThis, "fetch", async () => Response.json(payload(mobileAssets)));
  for (const locale of ["en", "it"] as const) {
    const faq = renderToStaticMarkup(FaqPage({ locale }));
    assert.ok(faq.includes(`href="${APP_STORE_URL}"`));
    assert.ok(faq.includes(locale === "it" ? "2,99 €" : "€2.99"));
    assert.doesNotMatch(faq, /coming soon|arriva presto|try the beta|provare la beta|testflight: true/i);
  }
  const data = JSON.parse(renderToStaticMarkup(await StructuredData({ locale: "en" })).replace(/^<script[^>]*>|<\/script>$/g, ""));
  const offers = data["@graph"][1].offers;
  assert.deepEqual(offers.map((offer: { price: string }) => offer.price), ["0", "2.99"]);
  assert.equal(offers[1].url, APP_STORE_URL);
});
