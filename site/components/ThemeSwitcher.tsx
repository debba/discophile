"use client";

import { useEffect, useRef, useState } from "react";
import { ui, type Locale } from "@/lib/i18n";

export type Theme = "auto" | "light" | "dark";
export const THEME_KEY = "discophile-theme";
const THEME_COLORS = { light: "#f4d9e2", dark: "#24111f" };

/** Runs in <head> before the first paint, so a saved light or dark choice never flashes the other theme. */
export const THEME_SCRIPT = `try{var t=localStorage.getItem("${THEME_KEY}");if(t==="light"||t==="dark")document.documentElement.dataset.theme=t}catch(e){}`;

function apply(theme: Theme) {
  const root = document.documentElement;
  if (theme === "auto") delete root.dataset.theme;
  else root.dataset.theme = theme;
  const dark = theme === "dark" || (theme === "auto" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.querySelectorAll('meta[name="theme-color"]').forEach((meta) => meta.setAttribute("content", dark ? THEME_COLORS.dark : THEME_COLORS.light));
}

const ICONS: Record<Theme, React.ReactNode> = {
  auto: <><circle cx="12" cy="12" r="8" /><path d="M12 4a8 8 0 0 1 0 16Z" fill="currentColor" /></>,
  light: <><circle cx="12" cy="12" r="4" /><path d="M12 2v2.5M12 19.5V22M4.9 4.9l1.8 1.8M17.3 17.3l1.8 1.8M2 12h2.5M19.5 12H22M4.9 19.1l1.8-1.8M17.3 6.7l1.8-1.8" /></>,
  dark: <path d="M20 14.5A8 8 0 0 1 9.5 4a8 8 0 1 0 10.5 10.5Z" />,
};

/** Automatic, Light or Dark, as a radio group of three icon buttons. */
export function ThemeSwitcher({ locale }: { locale: Locale }) {
  const t = ui[locale].theme;
  const [theme, setTheme] = useState<Theme>("auto");
  const buttons = useRef<(HTMLButtonElement | null)[]>([]);
  const options: Theme[] = ["auto", "light", "dark"];

  useEffect(() => {
    let saved: string | null = null;
    try {
      saved = localStorage.getItem(THEME_KEY);
    } catch {}
    const initial: Theme = saved === "light" || saved === "dark" ? saved : "auto";
    setTheme(initial);
    apply(initial);
    // Automatic follows the system as it changes.
    const media = matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      if (!document.documentElement.dataset.theme) apply("auto");
    };
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, []);

  const choose = (next: Theme, focus = false) => {
    setTheme(next);
    apply(next);
    try {
      if (next === "auto") localStorage.removeItem(THEME_KEY);
      else localStorage.setItem(THEME_KEY, next);
    } catch {}
    if (focus) buttons.current[options.indexOf(next)]?.focus();
  };

  return (
    <div className="theme-switch" role="radiogroup" aria-label={t.label}>
      {options.map((option, index) => (
        <button
          key={option}
          ref={(element) => { buttons.current[index] = element; }}
          type="button"
          role="radio"
          aria-checked={theme === option}
          tabIndex={theme === option ? 0 : -1}
          title={t[option]}
          onClick={() => choose(option)}
          onKeyDown={(event) => {
            const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
            if (step) {
              event.preventDefault();
              choose(options[(index + step + options.length) % options.length], true);
            }
          }}
        >
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{ICONS[option]}</svg>
          <span className="sr-only">{t[option]}</span>
        </button>
      ))}
    </div>
  );
}
