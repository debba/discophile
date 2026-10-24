import type { Metadata } from "next";
import { DownloadPage } from "@/components/DownloadPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Discophile on iPhone, Mac, Windows, Linux and in the browser: what each one does, what it needs, and where to download it.";

export const metadata: Metadata = pageMetadata({
  locale: "en",
  path: "/download",
  title: "Download Discophile for iPhone, Mac, Windows and Linux",
  socialTitle: "Discophile, wherever you listen",
  description,
  type: "website",
  imageAlt: OG_ALT.en,
});

export default function Download() {
  return <DownloadPage locale="en" />;
}
