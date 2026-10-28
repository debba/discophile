import { ui, type Locale } from "@/lib/i18n";

export function MobileInstallGuide({ locale, platform, expanded = false }: {
  locale: Locale;
  platform: "ios" | "android";
  expanded?: boolean;
}) {
  const t = ui[locale].mobile[platform];
  const ios = ui[locale].mobile.ios;
  return (
    <details className="mobile-install-guide" open={expanded || undefined}>
      <summary>{t.summary}</summary>
      <div>
        <p>{t.intro}</p>
        {platform === "ios" && (
          <p>
            {ios.tools}{" "}<a href="https://sideloadly.io/">Sideloadly</a>{" "}{ios.or}{" "}
            <a href="https://faq.altstore.io/altstore-classic/">AltStore Classic</a>.
          </p>
        )}
        <ol>{t.steps.map((step) => <li key={step}>{step}</li>)}</ol>
        <p className="installation-warning">{t.warning}</p>
      </div>
    </details>
  );
}
