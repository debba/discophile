import type { Viewport } from "next";
import { RootShell, rootMetadata } from "@/components/RootShell";

export const metadata = rootMetadata("en");
export const viewport: Viewport = { themeColor: "#f4d9e2" };

export default function EnglishLayout({ children }: { children: React.ReactNode }) {
  return <RootShell locale="en">{children}</RootShell>;
}
