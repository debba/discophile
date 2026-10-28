/** Bump when screenshots are replaced: /assets is cached for a week, so new files need new URLs. */
const V = "?v=2";

/**
 * A desktop screenshot in both appearances; CSS shows the one matching the site's theme. Both load lazily,
 * so the browser fetches only the visible one, right away when it's on screen.
 */
export function DesktopShot({ name, alt, eager }: { name: string; alt: string; eager?: boolean }) {
  const common = { width: 1600, height: 1065, loading: "lazy" as const, fetchPriority: eager ? ("high" as const) : undefined };
  return (
    <>
      <img className="theme-light-only" src={`/assets/screens/desktop/light/${name}.webp${V}`} alt={alt} {...common} />
      <img className="theme-dark-only" src={`/assets/screens/desktop/${name}.webp${V}`} alt={alt} {...common} />
    </>
  );
}

/** An iPhone screenshot in both appearances, like DesktopShot. */
export function PhoneShot({ name, alt, eager }: { name: string; alt: string; eager?: boolean }) {
  const common = { width: 645, height: 1398, loading: "lazy" as const, fetchPriority: eager ? ("high" as const) : undefined, "data-trim-bottom": name === "now-playing" || name.startsWith("ai-") ? "true" : undefined };
  return (
    <>
      <img className="theme-light-only" src={`/assets/screens/light/${name}.webp${V}`} alt={alt} {...common} />
      <img className="theme-dark-only" src={`/assets/screens/${name}.webp${V}`} alt={alt} {...common} />
    </>
  );
}

/** A web player screenshot (1280 x 800) in both appearances. */
export function WebShot({ name, alt }: { name: string; alt: string }) {
  const common = { width: 1280, height: 800, loading: "lazy" as const };
  return (
    <>
      <img className="theme-light-only" src={`/assets/screens/web/light/${name}.webp${V}`} alt={alt} {...common} />
      <img className="theme-dark-only" src={`/assets/screens/web/${name}.webp${V}`} alt={alt} {...common} />
    </>
  );
}

/**
 * Hero screenshots load lazily so only the visible theme is fetched, but lazy images wait for layout.
 * This inline script runs while the page is parsed and preloads the theme's variant at high priority.
 * The MacBook is hidden on narrow screens, so its preload only applies to wide ones.
 */
export function HeroPreload({ desktop, phone }: { desktop: string; phone: string }) {
  const script = `(function(){var t=document.documentElement.dataset.theme,d=t?t==="dark":matchMedia("(prefers-color-scheme: dark)").matches,s=d?"":"light/";[["/assets/screens/desktop/"+s+${JSON.stringify(desktop)},"(min-width: 861px)"],["/assets/screens/"+s+${JSON.stringify(phone)},""]].forEach(function(p){var l=document.createElement("link");l.rel="preload";l.as="image";l.href=p[0]+".webp${V}";l.fetchPriority="high";if(p[1])l.media=p[1];document.head.appendChild(l)})})()`;
  return <script dangerouslySetInnerHTML={{ __html: script }} />;
}
