import type { Metadata } from "next";
import { FeaturesPage } from "@/components/FeaturesPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "I mix di Ascolta, tutta la tua libreria, testi sincronizzati, un assistente musicale, audiolibri, statistiche e un equalizzatore a dieci bande, su iPhone, Android e sul computer.";

export const metadata: Metadata = pageMetadata({
  locale: "it",
  path: "/features",
  title: "Funzionalità | Discophile",
  socialTitle: "Tutto quello che fa Discophile",
  description,
  type: "website",
  imageAlt: OG_ALT.it,
});

export default function Features() {
  return <FeaturesPage locale="it" />;
}
