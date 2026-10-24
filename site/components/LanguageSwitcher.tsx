"use client";

import { useEffect, useId, useRef, useState } from "react";
import { LOCALES, LOCALE_COOKIE, basePath, localePath, ui, type Locale } from "@/lib/i18n";

const NAMES: Record<Locale, string> = { en: "English", it: "Italiano" };

function Flag({ locale }: { locale: Locale }) {
  const id = useId();
  if (locale === "it") {
    return (
      <svg className="flag" viewBox="0 0 3 2" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
        <path fill="#009246" d="M0 0h1v2H0z" />
        <path fill="#fff" d="M1 0h1v2H1z" />
        <path fill="#ce2b37" d="M2 0h1v2H2z" />
      </svg>
    );
  }
  return (
    <svg className="flag" viewBox="0 0 60 30" preserveAspectRatio="xMidYMid slice" aria-hidden="true">
      <clipPath id={`${id}-t`}><path d="M30 15h30v15zv15h-30zh-30v-15zv-15h30z" /></clipPath>
      <path fill="#012169" d="M0 0h60v30H0z" />
      <path stroke="#fff" strokeWidth="6" d="m0 0 60 30m0-30L0 30" />
      <path stroke="#c8102e" strokeWidth="4" d="m0 0 60 30m0-30L0 30" clipPath={`url(#${id}-t)`} />
      <path stroke="#fff" strokeWidth="10" d="M30 0v30M0 15h60" />
      <path stroke="#c8102e" strokeWidth="6" d="M30 0v30M0 15h60" />
    </svg>
  );
}

/** Disclosure button that lists the site's languages, each linking to the same page. */
export function LanguageSwitcher({ locale, pathname }: { locale: Locale; pathname: string }) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      // Keep the mobile menu open: only this list closes.
      event.stopPropagation();
      setOpen(false);
      buttonRef.current?.focus();
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const page = basePath(pathname);

  return (
    <div
      ref={rootRef}
      className="lang-switch"
      onBlur={(event) => {
        if (!rootRef.current?.contains(event.relatedTarget as Node)) setOpen(false);
      }}
    >
      <button
        ref={buttonRef}
        type="button"
        className="lang-button"
        aria-expanded={open}
        aria-controls={menuId}
        onClick={() => setOpen(!open)}
      >
        <Flag locale={locale} />
        <span className="sr-only">{ui[locale].language}: </span>
        <span>{NAMES[locale]}</span>
        <svg className="lang-chevron" width="12" height="12" viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 4.5 6 8l3.5-3.5" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" /></svg>
      </button>
      <ul id={menuId} className="lang-menu" hidden={!open}>
        {LOCALES.map((option) => (
          <li key={option}>
            <a
              href={localePath(option, page)}
              lang={option}
              hrefLang={option}
              aria-current={option === locale ? "true" : undefined}
              onClick={() => {
                const secure = location.protocol === "https:" ? "; Secure" : "";
                document.cookie = `${LOCALE_COOKIE}=${option}; Path=/; Max-Age=31536000; SameSite=Lax${secure}`;
              }}
            >
              <Flag locale={option} />
              <span>{NAMES[option]}</span>
              {option === locale && (
                <svg className="lang-check" width="16" height="16" viewBox="0 0 16 16" aria-hidden="true"><path d="m3.5 8.5 3 3 6-7" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" /></svg>
              )}
            </a>
          </li>
        ))}
      </ul>
    </div>
  );
}
