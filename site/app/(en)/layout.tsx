import type { Viewport } from "next";
import { RootShell, rootMetadata } from "@/components/RootShell";

export const metadata = rootMetadata("en");
export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f4d9e2" },
    { media: "(prefers-color-scheme: dark)", color: "#24111f" },
  ],
};

export default function EnglishLayout({ children }: { children: React.ReactNode }) {
  return <RootShell locale="en">{children}</RootShell>;
}
