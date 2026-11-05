"use client";

import { trackEvent } from "@/lib/analytics";
import { ui, type Locale } from "@/lib/i18n";
import { APP_STORE_URL } from "@/lib/releases";

/** Opens the iPhone app's App Store page, counted in analytics. */
export function AppStoreLink({ locale, source, className = "button primary", children }: {
  locale: Locale;
  source: string;
  className?: string;
  children?: React.ReactNode;
}) {
  return (
    <a className={className || undefined} href={APP_STORE_URL} onClick={() => trackEvent("App Store", "Open", source)}>
      {className.includes("button") && (
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
          <rect x="6.5" y="2.5" width="11" height="19" rx="2.5" /><path d="M10 5h4M11 18.5h2" />
        </svg>
      )}
      {children ?? ui[locale].download.buttons.ios}
    </a>
  );
}
