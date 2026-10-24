import type { Metadata } from "next";
import { Bricolage_Grotesque, Public_Sans } from "next/font/google";
import { CookieConsent } from "@/components/CookieConsent";
import { SiteHeader } from "@/components/SiteHeader";
import { TestFlightProvider } from "@/components/TestFlight";
import { alternates, ui, type Locale } from "@/lib/i18n";
import "@/app/globals.css";

const display = Bricolage_Grotesque({ subsets: ["latin"], axes: ["opsz", "wdth"], variable: "--font-display", display: "swap" });
const body = Public_Sans({ subsets: ["latin"], weight: ["400", "500", "600"], style: ["normal", "italic"], variable: "--font-body", display: "swap" });

const copy = {
  en: {
    title: "Discophile, a music player for your own server",
    description: "A native iPhone player for Navidrome, Subsonic and Jellyfin libraries, with Audiobookshelf audiobooks, internet radio and a music assistant that runs on your iPhone or with ChatGPT.",
    ogDescription: "Your music server, played like a record collection. A native iPhone player for Navidrome, Subsonic, Jellyfin and Audiobookshelf.",
    twitterDescription: "A native iPhone player for Navidrome, Subsonic, Jellyfin and Audiobookshelf.",
    ogAlt: "Discophile on two iPhones in front of a vinyl record, with the headline Your music server, played like a record collection.",
    ogLocale: "en_US",
  },
  it: {
    title: "Discophile, un player musicale per il tuo server",
    description: "Un player musicale nativo per iPhone, per le librerie Navidrome, Subsonic e Jellyfin, con gli audiolibri di Audiobookshelf, le radio online e un assistente che funziona sull'iPhone o con ChatGPT.",
    ogDescription: "La musica del tuo server, come una collezione di dischi. Un player nativo per iPhone, per Navidrome, Subsonic, Jellyfin e Audiobookshelf.",
    twitterDescription: "Un player musicale nativo per iPhone, per Navidrome, Subsonic, Jellyfin e Audiobookshelf.",
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
      images: [{ url: "/assets/og.png?v=2", width: 1200, height: 630, alt: c.ogAlt }],
    },
    twitter: { card: "summary_large_image", title: c.title, description: c.twitterDescription, images: ["/assets/og.png?v=2"] },
  };
}

export function RootShell({ locale, children }: { locale: Locale; children: React.ReactNode }) {
  return (
    <html lang={locale} className={`${display.variable} ${body.variable}`}>
      <body>
        <a className="skip" href="#main">{ui[locale].skip}</a>
        <TestFlightProvider locale={locale}>
          <SiteHeader locale={locale} />
          {children}
        </TestFlightProvider>
        <CookieConsent locale={locale} />
      </body>
    </html>
  );
}
