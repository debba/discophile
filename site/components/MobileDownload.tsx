import { DownloadFileLink } from "@/components/DownloadFileLink";
import { MobileInstallGuide } from "@/components/MobileInstallGuide";
import { ui, type Locale } from "@/lib/i18n";
import { RELEASES_URL, megabytes, type Release } from "@/lib/releases";

export function MobileDownload({ locale, platform, release }: {
  locale: Locale;
  platform: "android";
  release: Release | null;
}) {
  const t = ui[locale];
  const file = release?.files[platform];
  return (
    <div className="mobile-download">
      {file ? (
        <div className="card-actions">
          <DownloadFileLink file={file} format={platform} label={t.download.buttons[platform]} />
          <p className="fineprint file">{t.download.version} {release?.version} · {megabytes(file.size)} · {file.name}</p>
        </div>
      ) : (
        <p className="download-unavailable">
          {t.mobile.unavailable}{" "}<a href={release?.page ?? RELEASES_URL}>{t.download.notes}</a>.
        </p>
      )}
      <p className="fineprint mobile-download-tip">{t.download.tips[platform]}</p>
      <MobileInstallGuide locale={locale} platform={platform} />
    </div>
  );
}
