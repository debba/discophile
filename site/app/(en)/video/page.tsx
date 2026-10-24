import type { Metadata } from "next";
import { FullscreenVideoPage } from "@/components/HeroVideo";
import { alternates, localePath, ui } from "@/lib/i18n";

const locale = "en";
const t = ui[locale].video;
export const metadata: Metadata = {
  title: `${t.title} | Discophile`,
  description: t.note,
  alternates: { ...alternates("/video"), canonical: localePath(locale, "/video") },
  openGraph: {
    type: "video.other",
    url: localePath(locale, "/video"),
    title: `${t.title} | Discophile`,
    description: t.note,
    images: [{ url: "/assets/video/discophile-intro-poster.webp", width: 1920, height: 1080, alt: t.region }],
    videos: [{ url: "/assets/video/discophile-intro.mp4", type: "video/mp4", width: 1920, height: 1080 }],
  },
  twitter: { card: "summary_large_image", title: t.title, description: t.note, images: ["/assets/video/discophile-intro-poster.webp"] },
};

export default function VideoPage() {
  return <FullscreenVideoPage locale={locale} />;
}
