"use client";

import { usePathname } from "next/navigation";
import { useEffect, useRef, useState } from "react";
import { LanguageSwitcher } from "@/components/LanguageSwitcher";
import { ThemeSwitcher } from "@/components/ThemeSwitcher";
import { localePath, ui, type Locale } from "@/lib/i18n";

export function SiteHeader({ locale }: { locale: Locale }) {
  const t = ui[locale];
  const pathname = usePathname();
  const home = localePath(locale, "/");
  const links = [
    { href: localePath(locale, "/features"), label: t.nav.features },
    { href: localePath(locale, "/download"), label: t.nav.download },
    { href: localePath(locale, "/self-hosting"), label: t.nav.selfHosting },
    { href: localePath(locale, "/faq"), label: t.nav.faq },
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
        <div className="nav-tools">
          <LanguageSwitcher locale={locale} pathname={pathname} />
          <ThemeSwitcher locale={locale} />
          <a className="github-link" href="https://github.com/debba/discophile" title={ui[locale].footer.github} aria-label={ui[locale].footer.github}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M12 .5a11.5 11.5 0 0 0-3.64 22.41c.58.1.79-.25.79-.56v-2c-3.2.7-3.88-1.37-3.88-1.37-.53-1.33-1.28-1.69-1.28-1.69-1.05-.71.08-.7.08-.7 1.16.08 1.77 1.19 1.77 1.19 1.03 1.77 2.7 1.26 3.36.96.1-.75.4-1.26.73-1.55-2.56-.29-5.25-1.28-5.25-5.68 0-1.26.45-2.28 1.19-3.09-.12-.29-.52-1.46.11-3.05 0 0 .97-.31 3.17 1.18a11 11 0 0 1 5.77 0c2.2-1.49 3.17-1.18 3.17-1.18.63 1.59.23 2.76.11 3.05.74.81 1.19 1.83 1.19 3.09 0 4.41-2.69 5.38-5.26 5.67.41.36.78 1.06.78 2.14v3.17c0 .31.21.67.8.56A11.5 11.5 0 0 0 12 .5Z" /></svg>
            <span className="github-label" aria-hidden="true">GitHub</span>
          </a>
        </div>
      </nav>
    </header>
  );
}
