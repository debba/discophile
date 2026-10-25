//! Stages a verified release next to the installation. A detached helper replaces it after exit.

#[cfg(any(not(target_os = "macos"), test))]
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use discophile_core::updates::Release;
use sha2::{Digest, Sha256};

const MAX_DOWNLOAD: u64 = 1024 * 1024 * 1024;
#[cfg(any(not(target_os = "macos"), test))]
const MAX_EXECUTABLE: u64 = 512 * 1024 * 1024;
pub const PENDING_KEY: &str = "updates/pending";

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PendingUpdate {
    pub previous_version: String,
    pub release: Release,
}

pub struct PreparedUpdate {
    directory: tempfile::TempDir,
    target: PathBuf,
    staged: PathBuf,
}

pub enum Progress {
    Download { received: u64, total: u64 },
    Verifying,
}

pub fn save_pending(pending: &PendingUpdate) -> Result<(), String> {
    let path = super::storage::data_dir().join("state/updates/pending.json");
    std::fs::create_dir_all(path.parent().unwrap()).map_err(error)?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(pending).map_err(error)?).map_err(error)?;
    std::fs::rename(temporary, path).map_err(error)
}

pub fn take_failure() -> Option<String> {
    let path = super::storage::cache_dir().join("updater/error.txt");
    let message = std::fs::read_to_string(&path).ok()?;
    let _ = std::fs::remove_file(path);
    Some(message)
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn release_url(url: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("github.com")
            && url.path().starts_with("/debba/discophile/releases/download/")
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn checksum(manifest: &str, name: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let (hash, file) = line.split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| hash.to_ascii_lowercase())
    })
}

fn target_path(executable: &Path) -> Result<PathBuf, String> {
    #[cfg(target_os = "macos")]
    {
        let app = executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
            .ok_or("Automatic updates require the installed Discophile.app. Move it to Applications first.")?;
        if app.starts_with("/Volumes") || app.to_string_lossy().contains("/AppTranslocation/") {
            return Err("Move Discophile to Applications before installing an update.".into());
        }
        if app.components().any(|part| part.as_os_str() == "Caskroom" || part.as_os_str() == "Cellar") {
            return Err("This installation is managed by your system. Update it using your package manager.".into());
        }
        Ok(app.to_path_buf())
    }
    #[cfg(not(target_os = "macos"))]
    {
        if ["/usr", "/nix", "/snap"].iter().any(|prefix| executable.starts_with(prefix)) {
            return Err("This installation is managed by your system. Update it using your package manager.".into());
        }
        Ok(executable.to_path_buf())
    }
}

// AppImage names intentionally omit "linux-": older tarball updaters must keep selecting the archive.
fn appimage_asset<'a>(release: &'a Release, arch: &str) -> Option<&'a discophile_core::updates::Asset> {
    let name = format!("Discophile-{}-{arch}.AppImage", release.version);
    release.assets.iter().find(|asset| asset.name == name)
}

fn stage_appimage(archive: &Path, staged: &Path) -> Result<(), String> {
    use std::io::Read;
    let mut input = std::fs::File::open(archive).map_err(error)?;
    let mut header = [0; 11];
    input.read_exact(&mut header).map_err(error)?;
    if &header[..4] != b"\x7fELF" || &header[8..11] != b"AI\x02" {
        return Err("The package is not a type 2 AppImage.".into());
    }
    let length = std::fs::metadata(archive).map_err(error)?.len();
    if length > MAX_DOWNLOAD {
        return Err("The update package is too large.".into());
    }
    std::fs::copy(archive, staged).map_err(error)?;
    std::fs::File::open(staged).map_err(error)?.sync_all().map_err(error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755)).map_err(error)?;
    }
    Ok(())
}

/// Reports bytes on the task channel; disk work and archive validation run off the UI thread.
pub async fn prepare(release: &Release, progress: impl Fn(Progress)) -> Result<PreparedUpdate, String> {
    #[cfg(target_os = "linux")]
    let appimage = std::env::var_os("APPIMAGE").map(PathBuf::from);
    #[cfg(not(target_os = "linux"))]
    let appimage: Option<PathBuf> = None;
    let asset = if appimage.is_some() {
        appimage_asset(release, std::env::consts::ARCH)
    } else {
        release.asset_for(std::env::consts::OS, std::env::consts::ARCH)
    }.ok_or("There is no update package for this computer.")?;
    let manifest = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS.txt")
        .ok_or("The release has no checksum manifest. Automatic installation is unavailable.")?;
    if !release_url(&asset.url) || !release_url(&manifest.url) {
        return Err("The update package is not hosted in the Discophile release repository.".into());
    }
    let executable = appimage.clone().map_or_else(std::env::current_exe, Ok).map_err(error)?;
    let target = target_path(&executable.canonicalize().map_err(error)?)?;
    let directory = tempfile::Builder::new()
        .prefix(".discophile-update-")
        .tempdir_in(target.parent().ok_or("Invalid installation path.")?)
        .map_err(|e| format!("Cannot write to the installation folder: {e}"))?;
    let client = discophile_core::http::public_client();
    let response = client.get(&manifest.url).send().await.map_err(error)?.error_for_status().map_err(error)?;
    // The workflow manifest is tiny. Bound it independently of Content-Length.
    let mut stream = response.bytes_stream();
    use futures::StreamExt;
    let mut manifest_bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(error)?;
        if manifest_bytes.len() + chunk.len() > 64 * 1024 {
            return Err("Invalid checksum manifest.".into());
        }
        manifest_bytes.extend_from_slice(&chunk);
    }
    let expected = checksum(std::str::from_utf8(&manifest_bytes).map_err(error)?, &asset.name).ok_or("The package checksum is missing or invalid.")?;
    let response = client.get(&asset.url).send().await.map_err(error)?.error_for_status().map_err(error)?;
    let total = response.content_length().unwrap_or(asset.size);
    if total > MAX_DOWNLOAD {
        return Err("The update package is too large.".into());
    }
    let archive = directory.path().join("package");
    let mut file = tokio::fs::File::create(&archive).await.map_err(error)?;
    let mut digest = Sha256::new();
    let mut received = 0;
    let mut last_percent = u64::MAX;
    let mut stream = response.bytes_stream();
    use tokio::io::AsyncWriteExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(error)?;
        received += chunk.len() as u64;
        if received > MAX_DOWNLOAD {
            return Err("The update package is too large.".into());
        }
        file.write_all(&chunk).await.map_err(error)?;
        digest.update(&chunk);
        let percent = (received * 100).checked_div(total).unwrap_or(received / (256 * 1024));
        if percent != last_percent {
            progress(Progress::Download { received, total });
            last_percent = percent;
        }
    }
    file.sync_all().await.map_err(error)?;
    drop(file);
    progress(Progress::Verifying);
    if format!("{:x}", digest.finalize()) != expected {
        return Err("The package checksum does not match. Nothing was installed.".into());
    }
    let staged = directory.path().join(if cfg!(target_os = "macos") { "Discophile.app" } else { "Discophile.new" });
    let unpacked = staged.clone();
    let staging_root = directory.path().to_path_buf();
    let version = release.version.clone();
    let installed = target.clone();
    tokio::task::spawn_blocking(move || {
        if appimage.is_some() {
            stage_appimage(&archive, &unpacked)
        } else {
            unpack(&archive, &unpacked, &staging_root, &version, &installed)
        }
    }).await.map_err(error)??;
    Ok(PreparedUpdate { directory, target, staged })
}

#[cfg(target_os = "macos")]
fn unpack(archive: &Path, staged: &Path, root: &Path, version: &str, installed: &Path) -> Result<(), String> {
    let mount = root.join("mount");
    std::fs::create_dir(&mount).map_err(error)?;
    run(Command::new("/usr/bin/hdiutil").args(["attach", "-readonly", "-nobrowse", "-mountpoint"]).arg(&mount).arg(archive))?;
    let result = (|| {
        let app = mount.join("Discophile.app");
        run(Command::new("/usr/bin/codesign").args(["--verify", "--deep", "--strict"]).arg(&app))?;
        let actual = Command::new("/usr/bin/plutil")
            .args(["-extract", "CFBundleShortVersionString", "raw", "-o", "-"])
            .arg(app.join("Contents/Info.plist"))
            .output()
            .map_err(error)?;
        if !actual.status.success() || String::from_utf8_lossy(&actual.stdout).trim() != version {
            return Err("The package version does not match the release.".into());
        }
        if let Some(team) = signing_team(installed)?
            && signing_team(&app)?.as_ref() != Some(&team)
        {
            return Err("The update is not signed by the same developer as the installed app.".into());
        }
        // Keep the full bundle, resources and signature together.
        run(Command::new("/usr/bin/ditto").arg(&app).arg(staged))?;
        Ok(())
    })();
    let detached = run(Command::new("/usr/bin/hdiutil").arg("detach").arg(&mount));
    result.and(detached)
}

#[cfg(target_os = "macos")]
fn signing_team(app: &Path) -> Result<Option<String>, String> {
    let output = Command::new("/usr/bin/codesign").args(["-dv", "--verbose=4"]).arg(app).output().map_err(error)?;
    if !output.status.success() {
        return Err("Unable to verify the app signing identity.".into());
    }
    Ok(String::from_utf8_lossy(&output.stderr)
        .lines()
        .find_map(|line| line.strip_prefix("TeamIdentifier=").filter(|value| *value != "not set").map(str::to_string)))
}

#[cfg(target_os = "linux")]
fn unpack(archive: &Path, staged: &Path, _root: &Path, _version: &str, _installed: &Path) -> Result<(), String> {
    unpack_linux(archive, staged)
}

#[cfg(any(target_os = "linux", test))]
fn unpack_linux(archive: &Path, staged: &Path) -> Result<(), String> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(std::fs::File::open(archive).map_err(error)?));
    for entry in archive.entries().map_err(error)? {
        let mut entry = entry.map_err(error)?;
        if entry.path().map_err(error)?.as_ref() == Path::new("Discophile/discophile") {
            if !entry.header().entry_type().is_file() {
                return Err("The package executable is not a regular file.".into());
            }
            copy_executable(&mut entry, staged)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(staged, std::fs::Permissions::from_mode(0o755)).map_err(error)?;
            }
            return Ok(());
        }
    }
    Err("The package does not contain the Discophile executable.".into())
}

#[cfg(target_os = "windows")]
fn unpack(archive: &Path, staged: &Path, _root: &Path, _version: &str, _installed: &Path) -> Result<(), String> {
    unpack_windows(archive, staged)
}

#[cfg(any(target_os = "windows", test))]
fn unpack_windows(archive: &Path, staged: &Path) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(archive).map_err(error)?).map_err(error)?;
    let mut entry = archive.by_name("Discophile/Discophile.exe").map_err(error)?;
    copy_executable(&mut entry, staged)
}

#[cfg(any(not(target_os = "macos"), test))]
fn copy_executable(reader: &mut impl Read, path: &Path) -> Result<(), String> {
    let mut output = std::fs::File::create(path).map_err(error)?;
    let length = std::io::copy(&mut reader.take(MAX_EXECUTABLE + 1), &mut output).map_err(error)?;
    if length == 0 || length > MAX_EXECUTABLE {
        return Err("Invalid executable size.".into());
    }
    output.sync_all().map_err(error)
}

#[cfg(target_os = "macos")]
fn run(command: &mut Command) -> Result<(), String> {
    let output = command.output().map_err(error)?;
    if output.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&output.stderr).trim().to_string()) }
}

impl PreparedUpdate {
    pub fn handoff(self) -> Result<(), String> {
        let logs = super::storage::cache_dir().join("updater");
        std::fs::create_dir_all(&logs).map_err(error)?;
        let log = std::fs::File::create(logs.join("install.log")).map_err(error)?;
        let failure = logs.join("error.txt");
        let _ = std::fs::remove_file(&failure);
        let root = self.directory.path();
        let backup = root.join("previous");
        let script = root.join(if cfg!(windows) { "install.ps1" } else { "install.sh" });
        std::fs::write(&script, HELPER).map_err(error)?;
        #[cfg(unix)]
        let mut command = {
            let mut cmd = Command::new("/bin/sh");
            cmd.arg(&script)
                .arg(std::process::id().to_string())
                .arg(&self.target)
                .arg(&self.staged)
                .arg(&backup)
                .arg(root)
                .arg(&failure)
                .arg(if cfg!(target_os = "macos") { "macos" } else { "linux" });
            cmd
        };
        #[cfg(windows)]
        let mut command = {
            use std::os::windows::process::CommandExt;
            let mut cmd = Command::new("powershell.exe");
            cmd.creation_flags(0x08000000)
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(&script)
                .arg(std::process::id().to_string())
                .arg(&self.target)
                .arg(&self.staged)
                .arg(&backup)
                .arg(root)
                .arg(&failure);
            cmd
        };
        command.stdin(Stdio::null()).stdout(log.try_clone().map_err(error)?).stderr(log).spawn().map_err(error)?;
        // The helper owns cleanup only once it has been spawned successfully.
        let _ = self.directory.keep();
        Ok(())
    }
}

#[cfg(unix)]
const HELPER: &str = include_str!("updater/install.sh");
#[cfg(windows)]
const HELPER: &str = include_str!("updater/install.ps1");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appimage_updates_select_the_image_and_validate_its_header() {
        use discophile_core::updates::Asset;
        let mut release = Release { version: "0.1.3".into(), page_url: String::new(), notes: String::new(), assets: vec![] };
        for name in ["Discophile-0.1.3-linux-x86_64.tar.gz", "Discophile-0.1.3-x86_64.AppImage"] {
            release.assets.push(Asset { name: name.into(), url: String::new(), size: 0 });
        }
        assert!(appimage_asset(&release, "x86_64").unwrap().name.ends_with(".AppImage"));
        assert!(release.asset_for("linux", "x86_64").unwrap().name.ends_with(".tar.gz"));
        assert!(appimage_asset(&release, "aarch64").is_none());
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("image");
        let staged = temp.path().join("staged");
        std::fs::write(&archive, b"not an image").unwrap();
        assert!(stage_appimage(&archive, &staged).is_err());
        assert!(!staged.exists());
        let image = b"\x7fELF\x02\x01\x01\x00AI\x02payload";
        std::fs::write(&archive, image).unwrap();
        stage_appimage(&archive, &staged).unwrap();
        assert_eq!(std::fs::read(staged).unwrap(), image);
    }

    #[test]
    fn checksum_requires_exact_filename_and_sha256() {
        let hash = "a".repeat(64);
        let manifest = format!("{hash}  Discophile.zip\n{} *other.zip\n", "b".repeat(64));
        assert_eq!(checksum(&manifest, "Discophile.zip"), Some(hash));
        assert!(checksum(&manifest, "cophile.zip").is_none());
        assert!(checksum("bad Discophile.zip", "Discophile.zip").is_none());
    }

    #[test]
    fn rejects_foreign_release_packages() {
        assert!(release_url("https://github.com/debba/discophile/releases/download/v1/test.zip"));
        for url in [
            "http://github.com/debba/discophile/releases/download/v1/test.zip",
            "https://example.com/package",
            "https://github.com/other/repo/releases/download/v1/test.zip",
        ] {
            assert!(!release_url(url));
        }
    }

    #[test]
    fn windows_archive_extracts_only_the_expected_executable() {
        use std::io::Write;
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&package).unwrap());
        zip.start_file("Discophile/Discophile.exe", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"new executable").unwrap();
        zip.start_file("unrelated.txt", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(b"do not extract").unwrap();
        zip.finish().unwrap();
        let staged = temp.path().join("staged");
        unpack_windows(&package, &staged).unwrap();
        assert_eq!(std::fs::read(staged).unwrap(), b"new executable");
        assert!(!temp.path().join("unrelated.txt").exists());
    }

    #[test]
    fn linux_archive_rejects_symlinks_and_extracts_regular_executable() {
        for symlink in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let package = temp.path().join("package.tar.gz");
            let encoder = flate2::write::GzEncoder::new(std::fs::File::create(&package).unwrap(), flate2::Compression::fast());
            let mut archive = tar::Builder::new(encoder);
            let mut header = tar::Header::new_gnu();
            header.set_mode(0o755);
            if symlink {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_link_name("../unrelated").unwrap();
                header.set_size(0);
                archive.append_data(&mut header, "Discophile/discophile", &b""[..]).unwrap();
            } else {
                header.set_size(14);
                archive.append_data(&mut header, "Discophile/discophile", &b"new executable"[..]).unwrap();
            }
            archive.into_inner().unwrap().finish().unwrap();
            let staged = temp.path().join("staged");
            let result = unpack_linux(&package, &staged);
            assert_eq!(result.is_err(), symlink);
            if !symlink {
                assert_eq!(std::fs::read(staged).unwrap(), b"new executable");
            }
        }
    }

    #[cfg(unix)]
    fn helper_fixture(missing_package: bool, failed_launch: bool) {
        use std::os::unix::fs::PermissionsExt;
        // Exercise the real helper, but only with throwaway executables and a PID that cannot exist.
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("update with spaces and 'quotes'");
        std::fs::create_dir(&root).unwrap();
        let target = temporary.path().join("Discophile old 'version'");
        let staged = root.join("Discophile.new");
        let marker = temporary.path().join("started");
        let failure = temporary.path().join("error.txt");
        std::fs::write(&target, "#!/bin/sh\nprintf old > \"$DISCOPHILE_UPDATER_TEST_MARKER\"\nsleep 2\n").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !missing_package {
            let executable = if failed_launch { "#!/bin/sh\nexit 1\n" } else { "#!/bin/sh\nprintf new > \"$DISCOPHILE_UPDATER_TEST_MARKER\"\nsleep 2\n" };
            std::fs::write(&staged, executable).unwrap();
            std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let status = Command::new("/bin/sh")
            .arg("-c")
            .arg(HELPER)
            .arg("installer-test")
            .arg("2147483647")
            .arg(&target)
            .arg(&staged)
            .arg(root.join("previous"))
            .arg(&root)
            .arg(&failure)
            .arg("linux")
            .env("DISCOPHILE_UPDATER_TEST_MARKER", &marker)
            .status()
            .unwrap();
        let failed = missing_package || failed_launch;
        assert_eq!(status.success(), !failed);
        for _ in 0..100 {
            if std::fs::read_to_string(&marker).is_ok_and(|text| !text.is_empty()) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(std::fs::read_to_string(marker).unwrap(), if failed { "old" } else { "new" });
        assert_eq!(failure.exists(), failed);
        assert!(target.exists());
        if !failed {
            assert!(!root.exists());
        }
    }

    #[cfg(unix)]
    #[test]
    fn helper_installs_and_restarts_with_quoted_paths() {
        helper_fixture(false, false);
    }

    #[cfg(unix)]
    #[test]
    fn helper_restores_and_restarts_previous_version_on_failure() {
        helper_fixture(true, false);
    }

    #[cfg(unix)]
    #[test]
    fn helper_restores_previous_version_when_new_process_cannot_start() {
        helper_fixture(false, true);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn only_installed_bundles_can_update() {
        assert_eq!(target_path(Path::new("/Applications/Discophile.app/Contents/MacOS/Discophile")).unwrap(), PathBuf::from("/Applications/Discophile.app"));
        assert!(target_path(Path::new("/Volumes/Discophile/Discophile.app/Contents/MacOS/Discophile")).is_err());
        assert!(target_path(Path::new("/private/var/folders/x/AppTranslocation/abc/Discophile.app/Contents/MacOS/Discophile")).is_err());
        assert!(target_path(Path::new("/project/target/debug/Discophile")).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "mounts a temporary macOS disk image"]
    fn macos_dmg_stages_verified_bundle_and_rejects_wrong_version() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let app = source.join("Discophile.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::copy("/usr/bin/true", app.join("Contents/MacOS/Discophile")).unwrap();
        std::fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>app.discophile.updater-test</string>
<key>CFBundleExecutable</key><string>Discophile</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>1.2.3</string>
</dict></plist>"#,
        )
        .unwrap();
        run(Command::new("/usr/bin/codesign").args(["--force", "--sign", "-"]).arg(&app)).unwrap();
        let dmg = temp.path().join("update.dmg");
        run(Command::new("/usr/bin/hdiutil")
            .args(["create", "-quiet", "-volname", "Discophile updater test", "-srcfolder"])
            .arg(&source)
            .args(["-format", "UDZO"])
            .arg(&dmg))
        .unwrap();
        let wrong = temp.path().join("wrong");
        let correct = temp.path().join("correct");
        std::fs::create_dir(&wrong).unwrap();
        std::fs::create_dir(&correct).unwrap();
        assert!(unpack(&dmg, &wrong.join("Discophile.app"), &wrong, "9.9.9", &app).is_err());
        assert!(!wrong.join("Discophile.app").exists());
        unpack(&dmg, &correct.join("Discophile.app"), &correct, "1.2.3", &app).unwrap();
        assert!(correct.join("Discophile.app/Contents/MacOS/Discophile").exists());
        assert_eq!(std::fs::read_dir(correct.join("mount")).unwrap().count(), 0);
    }
}
