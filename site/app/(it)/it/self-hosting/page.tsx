import type { Metadata } from "next";
import { SelfHostingPage } from "@/components/SelfHostingPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Ospita il player web di Discophile con Docker o Kubernetes, accanto a Navidrome, Jellyfin o Audiobookshelf, con un proxy integrato che ti evita di configurare CORS.";

export const metadata: Metadata = pageMetadata({
  locale: "it",
  path: "/self-hosting",
  title: "Self-hosting del player web | Discophile",
  socialTitle: "Discophile: il tuo player, sul tuo server",
  description,
  type: "website",
  imageAlt: OG_ALT.it,
});

export default function SelfHosting() {
  return <SelfHostingPage locale="it" />;
}
