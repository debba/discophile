import type { Metadata } from "next";
import { FaqPage } from "@/components/FaqPage";
import { OG_ALT, pageMetadata } from "@/lib/seo";

const description = "Con quali server funziona Discophile, cosa serve su ogni dispositivo, che fine fanno i tuoi dati e come funzionano l'assistente e il player web.";

export const metadata: Metadata = pageMetadata({
  locale: "it",
  path: "/faq",
  title: "Domande frequenti | Discophile",
  socialTitle: "Discophile: domande e risposte",
  description,
  type: "website",
  imageAlt: OG_ALT.it,
});

export default function Faq() {
  return <FaqPage locale="it" />;
}
