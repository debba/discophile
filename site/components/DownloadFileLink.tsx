"use client";

import { trackEvent } from "@/lib/analytics";
import type { DownloadFormat, Release } from "@/lib/releases";

export function DownloadFileLink({ file, format, label, primary = true }: {
  file: NonNullable<Release["files"][DownloadFormat]>;
  format: DownloadFormat;
  label: string;
  primary?: boolean;
}) {
  return (
    <a className={primary ? "button primary" : "button"} href={file.url} onClick={() => trackEvent("Download", "File", format)}>
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d="M12 3v12m0 0-5-5m5 5 5-5M5 21h14" /></svg>
      {label}
    </a>
  );
}
