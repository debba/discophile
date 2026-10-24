import type { Viewport } from "next";
import { RootShell, rootMetadata } from "@/components/RootShell";

export const metadata = rootMetadata("it");
export const viewport: Viewport = { themeColor: "#f4d9e2" };

export default function ItalianLayout({ children }: { children: React.ReactNode }) {
  return <RootShell locale="it">{children}</RootShell>;
}
