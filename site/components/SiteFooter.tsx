import { CookiePreferencesButton } from "./CookieConsent";
import { localePath, ui, type Locale } from "@/lib/i18n";
import { TestFlightButton } from "./TestFlight";

const PRODUCTS = [
  { href: "https://gitdeck.dev", name: "Gitdeck", icon: "gitdeck" },
  { href: "https://tabularis.dev", name: "Tabularis", icon: "tabularis" },
  { href: "https://melicum.app", name: "Melicum", icon: "melicum" },
] as const;

export function SiteFooter({ locale, current, note }: { locale: Locale; current?: "privacy" | "cookie-policy"; note: string }) {
  const t = ui[locale].footer;
  return (
    <footer className="site-footer">
      <div>
        <a className="brand" href={localePath(locale, "/")}><img src="/assets/icon-72.png" alt="" width="28" height="28" loading="lazy" /><span>Discophile</span></a>
        <p>{t.madeBy} <a href="https://debbaweb.it">Andrea Debernardi</a>.</p>
      </div>
      <nav className="footer-links" aria-label={t.nav}>
        <a href={localePath(locale, "/privacy")} aria-current={current === "privacy" ? "page" : undefined}>{t.privacy}</a>
        <a href={localePath(locale, "/cookie-policy")} aria-current={current === "cookie-policy" ? "page" : undefined}>{t.cookies}</a>
        <CookiePreferencesButton locale={locale} />
        <TestFlightButton className="link-button" />
        <a href="https://github.com/sponsors/debba">{t.sponsor}</a>
        <a href="https://github.com/debba/discophile">{t.github}</a>
      </nav>
      <nav className="footer-products" aria-labelledby="other-projects">
        <h2 id="other-projects">{t.otherProjects}</h2>
        <ul>
          {PRODUCTS.map((product) => (
            <li key={product.name}>
              <a href={product.href} target="_blank" rel="noopener noreferrer">
                <img src={`/assets/products/${product.icon}.svg`} alt="" width="36" height="36" loading="lazy" />
                <span>
                  <strong>{product.name} <span aria-hidden="true">↗</span><span className="sr-only">{t.newTab}</span></strong>
                  <span>{t.products[product.icon]}</span>
                </span>
              </a>
            </li>
          ))}
        </ul>
      </nav>
      <p className="footer-note">{note}</p>
    </footer>
  );
}
