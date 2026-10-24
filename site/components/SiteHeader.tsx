"use client";

import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { localePath, ui, type Locale } from "@/lib/i18n";

export function SiteHeader({ locale }: { locale: Locale }) {
  const t = ui[locale];
  const pathname = usePathname();
  const home = localePath(locale, "/");
  const links = [
    { href: `${home}#listen`, label: t.nav.listen },
    { href: `${home}#assistant`, label: t.nav.assistant },
    { href: `${home}#audiobooks`, label: t.nav.audiobooks },
    { href: `${home}#release`, label: t.nav.beta },
    { href: localePath(locale, "/privacy"), label: t.nav.privacy },
  ];
  const [open, setOpen] = useState(false);
  const toggleRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    document.body.classList.toggle("menu-open", open);
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setOpen(false);
        toggleRef.current?.focus();
      }
    };
    const wide = matchMedia("(min-width: 721px)");
    const onResize = () => setOpen(false);
    document.addEventListener("keydown", onKeyDown);
    wide.addEventListener("change", onResize);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      wide.removeEventListener("change", onResize);
    };
  }, [open]);

  if (pathname === "/video" || pathname === "/it/video") return null;

  return (
    <header className="top">
      <a className="brand" href={home}>
        <img src="/assets/icon-72.png" alt="" width="36" height="36" />
        <span>Discophile</span>
      </a>
      <button
        ref={toggleRef}
        className="menu-toggle"
        type="button"
        aria-expanded={open}
        aria-controls="site-nav"
        onClick={() => setOpen(!open)}
      >
        <span className="menu-icon" aria-hidden="true" />
        <span className="menu-label">{t.menu}</span>
      </button>
      <nav id="site-nav" aria-label={t.mainNav} onClick={(event) => (event.target as Element).closest("a") && setOpen(false)}>
        {links.map((link) => (
          <a key={link.href} href={link.href} aria-current={pathname === link.href ? "page" : undefined}>
            {link.label}
          </a>
        ))}
        <LanguageSwitcher locale={locale} pathname={pathname} />
      </nav>
    </header>
  );
}
