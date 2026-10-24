import type { Viewport } from "next";
import { RootShell, rootMetadata } from "@/components/RootShell";

export const metadata = rootMetadata("it");
export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f4d9e2" },
    { media: "(prefers-color-scheme: dark)", color: "#24111f" },
  ],
};

export default function ItalianLayout({ children }: { children: React.ReactNode }) {
  return <RootShell locale="it">{children}</RootShell>;
}
