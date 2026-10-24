import type { Metadata } from "next";
import { FaqPage } from "@/components/FaqPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Which servers Discophile works with, what it needs on each device, what happens to your data, and how the assistant and the web player work.";

export const metadata: Metadata = pageMetadata({
  locale: "en",
  path: "/faq",
  title: "FAQ | Discophile",
  socialTitle: "Discophile: questions, answered",
  description,
  type: "website",
  imageAlt: OG_ALT.en,
});

export default function Faq() {
  return <FaqPage locale="en" />;
}
