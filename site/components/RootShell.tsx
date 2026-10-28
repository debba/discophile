import type { Metadata } from "next";
import { Bricolage_Grotesque, Public_Sans } from "next/font/google";
import { CookieConsent } from "@/components/CookieConsent";
import { DownloadProvider } from "@/components/Download";
import { THEME_SCRIPT } from "@/components/ThemeSwitcher";
import { SiteHeader } from "@/components/SiteHeader";
import { TestFlightProvider } from "@/components/TestFlight";
import { alternates, ui, type Locale } from "@/lib/i18n";
import { latestRelease } from "@/lib/releases";
import "@/app/globals.css";

const display = Bricolage_Grotesque({ subsets: ["latin"], axes: ["opsz", "wdth"], variable: "--font-display", display: "swap" });
const body = Public_Sans({ subsets: ["latin"], weight: ["400", "500", "600"], style: ["normal", "italic"], variable: "--font-body", display: "swap" });

const copy = {
  en: {
    title: "Discophile, a music player for your own server",
    description: "A music player for your Navidrome, Subsonic and Jellyfin libraries, with audiobooks, radio and a music assistant. On iPhone, Android, Mac, Windows, Linux and the web.",
    ogDescription: "Your music server, played like a record collection. A player for Navidrome, Subsonic, Jellyfin and Audiobookshelf on iPhone, Android, Mac, Windows, Linux and the web.",
    twitterDescription: "A player for Navidrome, Subsonic, Jellyfin and Audiobookshelf on iPhone, Android, Mac, Windows, Linux and the web.",
    ogAlt: "Discophile on two iPhones in front of a vinyl record, with the headline Your music server, played like a record collection.",
    ogLocale: "en_US",
  },
  it: {
    title: "Discophile, un player musicale per il tuo server",
    description: "Un player per le tue librerie Navidrome, Subsonic e Jellyfin, con audiolibri, radio e un assistente musicale. Su iPhone, Android, Mac, Windows, Linux e web.",
    ogDescription: "La musica del tuo server, come una collezione di dischi. Un player per Navidrome, Subsonic, Jellyfin e Audiobookshelf su iPhone, Android, Mac, Windows, Linux e web.",
    twitterDescription: "Un player musicale per Navidrome, Subsonic, Jellyfin e Audiobookshelf su iPhone, Android, Mac, Windows, Linux e web.",
    ogAlt: "Discophile su due iPhone davanti a un disco in vinile, con il titolo in inglese Your music server, played like a record collection.",
    ogLocale: "it_IT",
  },
};

export function rootMetadata(locale: Locale): Metadata {
  const c = copy[locale];
  const path = locale === "en" ? "/" : "/it";
  return {
    metadataBase: new URL("https://www.discophile.app"),
    title: c.title,
    description: c.description,
    manifest: "/site.webmanifest",
    icons: {
      icon: [{ url: "/favicon.ico", sizes: "48x48" }, { url: "/assets/favicon.svg", type: "image/svg+xml" }],
      apple: "/assets/apple-touch-icon.png",
    },
    alternates: { ...alternates("/"), canonical: path },
    openGraph: {
      type: "website",
      siteName: "Discophile",
      locale: c.ogLocale,
      url: path,
      title: c.title,
      description: c.ogDescription,
      images: [{ url: "/assets/og.png?v=3", width: 1200, height: 630, alt: c.ogAlt }],
    },
    twitter: { card: "summary_large_image", title: c.title, description: c.twitterDescription, images: ["/assets/og.png?v=3"] },
  };
}

export async function RootShell({ locale, children }: { locale: Locale; children: React.ReactNode }) {
  const release = await latestRelease();
  return (
    // The theme script sets data-theme before React hydrates, so that attribute may differ from the server's.
    <html lang={locale} className={`${display.variable} ${body.variable}`} suppressHydrationWarning>
      <head>
        <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />
      </head>
      <body>
        <a className="skip" href="#main">{ui[locale].skip}</a>
        <TestFlightProvider locale={locale}>
          <DownloadProvider locale={locale} release={release}>
            <SiteHeader locale={locale} />
            {children}
          </DownloadProvider>
        </TestFlightProvider>
        <CookieConsent locale={locale} />
      </body>
    </html>
  );
}
