//! Desktop updates: the latest published GitHub release and the download that suits this computer.
//! Release assets are named `Discophile-<version>-<os>-<arch>.<ext>` by the release workflow, with
//! `universal` as the macOS architecture.

use serde::{Deserialize, Serialize};

pub const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/debba/discophile/releases/latest";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Release {
    /// The tag without its leading `v`, for example `1.2.0`.
    pub version: String,
    pub page_url: String,
    pub notes: String,
    pub assets: Vec<Asset>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<AssetJson>,
}

#[derive(Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Parses a GitHub release document; drafts and pre-releases are not offered as updates.
pub fn parse_release(json: &str) -> Option<Release> {
    let release: ReleaseJson = serde_json::from_str(json).ok()?;
    if release.draft || release.prerelease {
        return None;
    }
    let version = release.tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    parse_version(&version)?;
    Some(Release {
        version,
        page_url: release.html_url,
        notes: release.body.unwrap_or_default().trim().to_string(),
        assets: release.assets.into_iter().map(|a| Asset { name: a.name, url: a.browser_download_url, size: a.size }).collect(),
    })
}

/// `major.minor.patch`, ignoring a leading `v` and any pre-release or build suffix. Missing parts are 0.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    let core = text.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    let patch = parts.next().map_or(Some(0), |p| p.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(candidate), Some(current)) => candidate > current,
        _ => false,
    }
}

impl Release {
    /// The download for `os` and `arch` as in `std::env::consts` (`macos`, `windows`, `linux`;
    /// `x86_64`, `aarch64`).
    pub fn asset_for(&self, os: &str, arch: &str) -> Option<&Asset> {
        let preferred: &[&str] = match os {
            "macos" => &[".dmg", ".zip"],
            "windows" => &[".msi", ".exe", ".zip"],
            _ => &[".appimage", ".tar.gz"],
        };
        let matches = |asset: &&Asset, extension: &str| {
            let name = asset.name.to_ascii_lowercase();
            let suffix = format!("-{os}-");
            let arch_ok = name.contains(&format!("{suffix}{arch}.")) || (os == "macos" && name.contains("-macos-universal."));
            name.starts_with("discophile-") && name.contains(&suffix) && arch_ok && name.ends_with(extension)
        };
        preferred.iter().find_map(|extension| self.assets.iter().find(|asset| matches(asset, extension)))
    }
}

/// Fetches the latest release. `None` when there is none, it cannot be read, or the request fails.
pub async fn fetch_latest() -> Option<Release> {
    let response = crate::http::public_client().get(LATEST_RELEASE_URL).header("Accept", "application/vnd.github+json").send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    parse_release(&response.text().await.ok()?)
}

/// Published release notes only: open/unmerged PRs are not installed changes.
/// Includes every skipped release, newest first. Fails as a whole rather than presenting partial notes.
pub async fn fetch_notes_between(previous: &str, current: &str) -> Option<String> {
    let client = crate::http::public_client();
    let mut notes = Vec::new();
    for page in 1..=10 {
        let response = client
            .get(format!("https://api.github.com/repos/debba/discophile/releases?per_page=100&page={page}"))
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        let releases: Vec<serde_json::Value> = response.json().await.ok()?;
        for json in &releases {
            if let Some(release) = parse_release(&json.to_string())
                && is_newer(&release.version, previous)
                && !is_newer(&release.version, current)
            {
                notes.push(release);
            }
        }
        if releases.len() < 100 {
            return Some(format_notes(notes));
        }
    }
    None
}

fn format_notes(mut releases: Vec<Release>) -> String {
    releases.sort_by_key(|release| std::cmp::Reverse(parse_version(&release.version)));
    releases.into_iter().map(|release| format!("# Discophile {}\n\n{}", release.version, release.notes)).collect::<Vec<_>>().join("\n\n---\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(names: &[&str]) -> Release {
        Release {
            version: "1.2.0".into(),
            page_url: String::new(),
            notes: String::new(),
            assets: names.iter().map(|n| Asset { name: n.to_string(), url: format!("https://x/{n}"), size: 1 }).collect(),
        }
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0-beta", "0.1.0"));
        assert!(!is_newer("garbage", "0.1.0"));
        assert_eq!(parse_version("2.3.4+build"), Some((2, 3, 4)));
        assert_eq!(parse_version("1.2.3.4"), None);
    }

    #[test]
    fn parses_releases_and_skips_prereleases() {
        let json = r#"{"tag_name":"v1.2.0","html_url":"https://github.com/r","body":" Notes ","draft":false,"prerelease":false,
            "assets":[{"name":"Discophile-1.2.0-linux-x86_64.tar.gz","browser_download_url":"https://d/l","size":5}]}"#;
        let release = parse_release(json).unwrap();
        assert_eq!(release.version, "1.2.0");
        assert_eq!(release.notes, "Notes");
        assert_eq!(release.assets[0].size, 5);
        assert!(parse_release(&json.replace(r#""prerelease":false"#, r#""prerelease":true"#)).is_none());
        assert!(parse_release(&json.replace("v1.2.0", "nightly")).is_none());
    }

    #[test]
    fn picks_the_download_for_this_computer() {
        let release = release(&[
            "Discophile-1.2.0-macos-universal.dmg",
            "Discophile-1.2.0-windows-x86_64.zip",
            "Discophile-1.2.0-linux-x86_64.tar.gz",
            "Discophile-1.2.0-linux-aarch64.tar.gz",
            "checksums.txt",
        ]);
        assert_eq!(release.asset_for("macos", "aarch64").unwrap().name, "Discophile-1.2.0-macos-universal.dmg");
        assert_eq!(release.asset_for("windows", "x86_64").unwrap().name, "Discophile-1.2.0-windows-x86_64.zip");
        assert_eq!(release.asset_for("linux", "aarch64").unwrap().name, "Discophile-1.2.0-linux-aarch64.tar.gz");
        assert!(release.asset_for("windows", "aarch64").is_none());
    }

    #[test]
    fn changelog_preserves_markdown_and_orders_skipped_versions() {
        let mut older = release(&[]);
        older.version = "0.9.0".into();
        older.notes = "## Fixes\n\n* **Playback** ([#12](https://github.com/debba/discophile/pull/12))".into();
        let mut newer = release(&[]);
        newer.version = "0.10.0".into();
        newer.notes = "## Features\n\n* `Markdown` notes".into();
        let markdown = format_notes(vec![older.clone(), newer.clone()]);
        assert!(markdown.starts_with("# Discophile 0.10.0"));
        assert!(markdown.find(&newer.notes).unwrap() < markdown.find(&older.notes).unwrap());
        assert!(markdown.contains("**Playback**"));
        assert!(markdown.contains("[#12](https://github.com/debba/discophile/pull/12)"));
    }
}
