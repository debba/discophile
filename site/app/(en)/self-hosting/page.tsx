import type { Metadata } from "next";
import { SelfHostingPage } from "@/components/SelfHostingPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Host the Discophile web player with Docker or Kubernetes, next to Navidrome, Jellyfin or Audiobookshelf, with a built-in proxy so you don't have to set up CORS.";

export const metadata: Metadata = pageMetadata({
  locale: "en",
  path: "/self-hosting",
  title: "Self-hosting the web player | Discophile",
  socialTitle: "Discophile: your player, on your server",
  description,
  type: "website",
  imageAlt: OG_ALT.en,
});

export default function SelfHosting() {
  return <SelfHostingPage locale="en" />;
}
