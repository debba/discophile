import { ui, type Locale } from "@/lib/i18n";

export function MobileInstallGuide({ locale, platform, expanded = false }: {
  locale: Locale;
  platform: "android";
  expanded?: boolean;
}) {
  const t = ui[locale].mobile[platform];
  return (
    <details className="mobile-install-guide" open={expanded || undefined}>
      <summary>{t.summary}</summary>
      <div>
        <p>{t.intro}</p>
        <ol>{t.steps.map((step) => <li key={step}>{step}</li>)}</ol>
        <p className="installation-warning">{t.warning}</p>
      </div>
    </details>
  );
}
