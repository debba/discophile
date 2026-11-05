import type { Locale } from "@/lib/i18n";
import { APP_STORE_URL, REPO_URL, latestRelease } from "@/lib/releases";
import { SITE_URL } from "@/lib/seo";

const DESCRIPTION: Record<Locale, string> = {
  en: "A music player for Navidrome, Subsonic and Jellyfin libraries, with Audiobookshelf audiobooks, internet radio and a music assistant.",
  it: "Un player musicale per le librerie Navidrome, Subsonic e Jellyfin, con gli audiolibri di Audiobookshelf, le radio online e un assistente musicale.",
};

/** Schema.org data for search engines: the app, its platforms, its free downloads and the paid iPhone app. */
export async function StructuredData({ locale }: { locale: Locale }) {
  const release = await latestRelease();
  const data = {
    "@context": "https://schema.org",
    "@graph": [
      {
        "@type": "WebSite",
        "@id": `${SITE_URL}/#website`,
        url: SITE_URL,
        name: "Discophile",
        inLanguage: ["en", "it"],
      },
      {
        "@type": "SoftwareApplication",
        "@id": `${SITE_URL}/#app`,
        name: "Discophile",
        url: SITE_URL,
        description: DESCRIPTION[locale],
        applicationCategory: "MultimediaApplication",
        applicationSubCategory: "Music player",
        operatingSystem: "iOS 17, Android 8, macOS 11, Windows 10, Linux, Web",
        image: `${SITE_URL}/assets/icon-512.png`,
        screenshot: [`${SITE_URL}/assets/screens/desktop/now-playing.webp`, `${SITE_URL}/assets/screens/now-playing.webp`],
        softwareVersion: release?.version,
        downloadUrl: release?.page ?? `${REPO_URL}/releases/latest`,
        license: "https://opensource.org/licenses/MIT",
        codeRepository: REPO_URL,
        inLanguage: ["en", "it"],
        offers: [
          { "@type": "Offer", price: "0", priceCurrency: "EUR", url: `${SITE_URL}${locale === "en" ? "" : "/it"}/download`, category: "Android, Mac, Windows, Linux, Web" },
          { "@type": "Offer", price: "2.99", priceCurrency: "EUR", url: APP_STORE_URL, category: "iPhone" },
        ],
        author: { "@type": "Person", name: "Andrea Debernardi", url: "https://debbaweb.it" },
      },
    ],
  };
  return (
    <script
      type="application/ld+json"
      // JSON.stringify escapes everything that matters here; "<" is escaped so the script can't be closed early.
      dangerouslySetInnerHTML={{ __html: JSON.stringify(data).replace(/</g, "\\u003c") }}
    />
  );
}
