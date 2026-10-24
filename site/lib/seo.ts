import type { Metadata } from "next";
import { alternates, type Locale } from "@/lib/i18n";

export const SITE_URL = "https://www.discophile.app";
export const OG_IMAGE = { url: "/assets/og.png?v=3", width: 1200, height: 630 };

/** Metadata for an inner page: canonical and hreflang, and social cards that keep the shared image. */
export function pageMetadata({
  locale,
  path,
  title,
  socialTitle,
  description,
  type = "website",
  imageAlt,
}: {
  locale: Locale;
  path: string;
  title: string;
  socialTitle: string;
  description: string;
  type?: "website" | "article";
  imageAlt: string;
}): Metadata {
  const url = locale === "en" ? path : `/it${path}`;
  return {
    title,
    description,
    alternates: { ...alternates(path), canonical: url },
    openGraph: {
      type,
      siteName: "Discophile",
      locale: locale === "en" ? "en_US" : "it_IT",
      url,
      title: socialTitle,
      description,
      images: [{ ...OG_IMAGE, alt: imageAlt }],
    },
    twitter: { card: "summary_large_image", title: socialTitle, description, images: [OG_IMAGE.url] },
  };
}

export const OG_ALT: Record<Locale, string> = {
  en: "Discophile on two iPhones in front of a vinyl record, with the headline Your music server, played like a record collection.",
  it: "Discophile su due iPhone davanti a un disco in vinile, con il titolo in inglese Your music server, played like a record collection.",
};
