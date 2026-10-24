/** The latest desktop release, read on the server from GitHub and cached for an hour. */

export const REPO_URL = "https://github.com/debba/discophile";
export const RELEASES_URL = `${REPO_URL}/releases/latest`;
export const WEB_GUIDE_URL = `${REPO_URL}/blob/main/docs/WEB.md`;
export const AUR_URL = "https://aur.archlinux.org/packages/discophile-bin";

export type Platform = "mac" | "windows" | "linux";

export type Release = {
  version: string;
  page: string;
  files: Partial<Record<Platform, { name: string; url: string; size: number }>>;
};

const PATTERNS: Record<Platform, RegExp> = {
  mac: /-macos-universal\.dmg$/,
  windows: /-windows-x86_64\.zip$/,
  linux: /-linux-x86_64\.tar\.gz$/,
};

export async function latestRelease(): Promise<Release | null> {
  try {
    const response = await fetch("https://api.github.com/repos/debba/discophile/releases/latest", {
      headers: { Accept: "application/vnd.github+json" },
      next: { revalidate: 3600 },
    });
    if (!response.ok) return null;
    const json = (await response.json()) as {
      tag_name: string;
      html_url: string;
      assets: { name: string; browser_download_url: string; size: number }[];
    };
    const files: Release["files"] = {};
    for (const platform of Object.keys(PATTERNS) as Platform[]) {
      const asset = json.assets.find((a) => PATTERNS[platform].test(a.name));
      if (asset) files[platform] = { name: asset.name, url: asset.browser_download_url, size: asset.size };
    }
    return { version: json.tag_name.replace(/^v/, ""), page: json.html_url, files };
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
