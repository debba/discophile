/** Published client downloads, read on the server from GitHub and cached for an hour. */

export const REPO_URL = "https://github.com/debba/discophile";
export const RELEASES_URL = `${REPO_URL}/releases/latest`;
export const WEB_GUIDE_URL = `${REPO_URL}/blob/main/docs/WEB.md`;
export const AUR_URL = "https://aur.archlinux.org/packages/discophile-bin";

export type Platform = "mac" | "windows" | "linux" | "android" | "ios";
export type DesktopPlatform = Exclude<Platform, "android" | "ios">;
export type DownloadFormat = Platform | "deb" | "appimage";

export type Release = {
  version: string;
  page: string;
  files: Partial<Record<DownloadFormat, { name: string; url: string; size: number }>>;
};

const FILENAMES: Record<DownloadFormat, string> = {
  mac: "Discophile-VERSION-macos-universal.dmg",
  windows: "Discophile-VERSION-windows-x86_64.zip",
  linux: "Discophile-VERSION-linux-x86_64.tar.gz",
  deb: "discophile_VERSION_amd64.deb",
  appimage: "Discophile-VERSION-x86_64.AppImage",
  android: "Discophile-VERSION-android.apk",
  ios: "Discophile-VERSION-ios-unsigned.ipa",
};

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

/** Never offer a stale-version asset, a draft, or an untrusted download URL. */
export function parseRelease(value: unknown): Release | null {
  if (!isObject(value) || value.draft || value.prerelease || typeof value.tag_name !== "string") return null;
  const tag = value.tag_name;
  const version = tag.replace(/^v/, "");
  if (!/^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.test(version)) return null;
  const page = `${REPO_URL}/releases/tag/${tag}`;
  if (value.html_url !== page || !Array.isArray(value.assets)) return null;
  const files: Release["files"] = {};
  for (const format of Object.keys(FILENAMES) as DownloadFormat[]) {
    const name = FILENAMES[format].replace("VERSION", version);
    const url = `${REPO_URL}/releases/download/${tag}/${name}`;
    const asset = value.assets.find((entry: unknown) => isObject(entry) && entry.name === name
      && entry.browser_download_url === url && typeof entry.size === "number" && Number.isFinite(entry.size) && entry.size > 0);
    if (asset) files[format] = { name, url, size: asset.size };
  }
  return { version, page, files };
}

export async function latestRelease(): Promise<Release | null> {
  try {
    const response = await fetch("https://api.github.com/repos/debba/discophile/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
      next: { revalidate: 3600 },
    });
    return response.ok ? parseRelease(await response.json()) : null;
  } catch {
    return null;
  }
}

/** A direct download for the platform, or the release page when the file is missing. */
export function downloadUrl(release: Release | null, platform: Platform) {
  return release?.files[platform]?.url ?? release?.page ?? RELEASES_URL;
}

export function megabytes(bytes: number | undefined) {
  return bytes ? `${Math.round(bytes / 1_000_000)} MB` : "";
}
