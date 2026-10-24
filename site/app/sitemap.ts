import type { MetadataRoute } from "next";
import { SITE_URL } from "@/lib/seo";

const PATHS = ["/", "/features", "/download", "/self-hosting", "/faq", "/video", "/privacy", "/cookie-policy"];

/** Every page in both languages, each listing the other through hreflang alternates. */
export default function sitemap(): MetadataRoute.Sitemap {
  return PATHS.flatMap((path) => {
    const suffix = path === "/" ? "" : path;
    const languages = { en: `${SITE_URL}${suffix || "/"}`, it: `${SITE_URL}/it${suffix}` };
    const entry = {
      changeFrequency: (path === "/" || path === "/download" ? "weekly" : "monthly") as "weekly" | "monthly",
      priority: path === "/" ? 1 : path === "/privacy" || path === "/cookie-policy" ? 0.3 : 0.8,
      alternates: { languages },
    };
    return [{ url: languages.en, ...entry }, { url: languages.it, ...entry }];
  });
}
