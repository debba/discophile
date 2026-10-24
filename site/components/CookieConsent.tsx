"use client";

import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { CONSENT_KEY, MANAGE_COOKIES_EVENT, initMatomo, manageCookies } from "@/lib/analytics";

type Consent = { measurement: boolean };

function readConsent(): Consent | null {
  try {
    const raw = localStorage.getItem(CONSENT_KEY);
    if (!raw) return null;
    const value = JSON.parse(raw);
    return typeof value?.measurement === "boolean" ? { measurement: value.measurement } : null;
  } catch {
    return null;
  }
}

export function CookieConsent({ locale }: { locale: Locale }) {
  const pathname = usePathname();
  const t = ui[locale].cookies;
  const [visible, setVisible] = useState(false);
  const [current, setCurrent] = useState<Consent | null>(null);
  const bannerRef = useRef<HTMLElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);
  const openerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    const consent = readConsent();
    setCurrent(consent);
    initMatomo(consent?.measurement ?? false);
    if (!consent) setVisible(true);

    const onManage = () => {
      openerRef.current = document.activeElement as HTMLElement | null;
      setCurrent(readConsent());
      setVisible(true);
      requestAnimationFrame(() => headingRef.current?.focus());
    };
    window.addEventListener(MANAGE_COOKIES_EVENT, onManage);
    return () => window.removeEventListener(MANAGE_COOKIES_EVENT, onManage);
  }, []);

  function choose(measurement: boolean) {
    try {
      localStorage.setItem(CONSENT_KEY, JSON.stringify({ measurement }));
    } catch {
      // Private browsing can block storage; the choice still applies to this page.
    }
    initMatomo(measurement);
    setCurrent({ measurement });
    // The banner disappears, so move focus somewhere predictable instead of losing it.
    const hadFocus = bannerRef.current?.contains(document.activeElement);
    setVisible(false);
    const opener = openerRef.current;
    openerRef.current = null;
    if (!hadFocus) return;
    if (opener?.isConnected) opener.focus();
    else document.getElementById("main")?.focus({ preventScroll: true });
  }

  if (!visible || pathname === "/video" || pathname === "/it/video") return null;

  return (
    <section ref={bannerRef} className="cookie-banner" aria-labelledby="cookie-title">
      <h2 id="cookie-title" ref={headingRef} tabIndex={-1}>{t.title}</h2>
      <p>
        {t.text} <a href={localePath(locale, "/cookie-policy")}>{t.link}</a>.
      </p>
      {current && (
        <p className="cookie-current">
          {t.current} {current.measurement ? t.allowed : t.cookieless}.
        </p>
      )}
      <div className="cookie-actions">
        <button type="button" className="button" onClick={() => choose(true)}>{t.allow}</button>
        <button type="button" className="button" onClick={() => choose(false)}>{t.deny}</button>
      </div>
    </section>
  );
}

export function CookiePreferencesButton({ locale }: { locale: Locale }) {
  return (
    <button type="button" className="link-button" onClick={manageCookies}>
      {ui[locale].footer.cookiePreferences}
    </button>
  );
}
