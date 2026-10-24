import type { Metadata } from "next";
import { FeaturesPage } from "@/components/FeaturesPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Listen Now mixes, your whole library, synced lyrics, a music assistant, audiobooks, statistics and a ten-band equalizer, on iPhone and on the desktop.";

export const metadata: Metadata = pageMetadata({
  locale: "en",
  path: "/features",
  title: "Features | Discophile",
  socialTitle: "Everything Discophile does",
  description,
  type: "website",
  imageAlt: OG_ALT.en,
});

export default function Features() {
  return <FeaturesPage locale="en" />;
}
