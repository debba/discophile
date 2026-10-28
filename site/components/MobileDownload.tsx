import { DownloadButton } from "@/components/Download";
import { MobileInstallGuide } from "@/components/MobileInstallGuide";
import { ui, type Locale } from "@/lib/i18n";
import { RELEASES_URL, megabytes, type Release } from "@/lib/releases";

export function MobileDownload({ locale, platform, release }: {
  locale: Locale;
  platform: "ios" | "android";
  release: Release | null;
}) {
  const t = ui[locale];
  const file = release?.files[platform];
  return (
    <div className="mobile-download">
      <p className="fineprint">{t.download.tips[platform]}</p>
      {file ? (
        <div className="card-actions">
          <DownloadButton platform={platform} labels={{ any: t.download.file, ...t.download.buttons }} />
          <p className="fineprint file">{t.download.version} {release?.version} · {megabytes(file.size)} · {file.name}</p>
        </div>
      ) : (
        <p className="download-unavailable">
          {t.mobile.unavailable}{" "}<a href={release?.page ?? RELEASES_URL}>{t.download.notes}</a>.
        </p>
      )}
      <MobileInstallGuide locale={locale} platform={platform} />
    </div>
  );
}
