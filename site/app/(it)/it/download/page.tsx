import type { Metadata } from "next";
import { DownloadPage } from "@/components/DownloadPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Discophile su iPhone, Android, Mac, Windows, Linux e nel browser: cosa fa su ognuno, cosa serve e dove scaricarlo.";

export const metadata: Metadata = pageMetadata({
  locale: "it",
  path: "/download",
  title: "Scarica Discophile per iPhone, Android, Mac, Windows e Linux",
  socialTitle: "Discophile, ovunque tu ascolti",
  description,
  type: "website",
  imageAlt: OG_ALT.it,
});

export default function Download() {
  return <DownloadPage locale="it" />;
}
