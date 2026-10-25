#!/usr/bin/env bash
set -euo pipefail

# Run on Ubuntu 22.04 x86_64 after building the release executable.
binary=$(realpath "${1:?Usage: package-linux.sh BINARY VERSION [OUTPUT]}")
version=${2:?Missing version}
output=$(realpath -m "${3:-dist}")
repo=$(cd "$(dirname "$0")/.." && pwd)
[[ $(uname -m) == x86_64 ]] || { echo 'Linux packages require x86_64.' >&2; exit 1; }
[[ $version =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo 'Invalid release version.' >&2; exit 1; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$output" "$work/Discophile"
install -m755 "$binary" "$work/Discophile/discophile"
cp "$repo/packaging/linux/"{discophile.desktop,discophile.png} "$repo/LICENSE" "$work/Discophile/"
tar -C "$work" -czf "$output/Discophile-$version-linux-x86_64.tar.gz" Discophile

# Generate linked dependencies; declare window-system libraries loaded through dlopen separately.
mkdir -p "$work/debian" "$work/deb/DEBIAN"
cat > "$work/debian/control" <<EOF
Source: discophile
Section: sound
Priority: optional
Maintainer: Andrea Debernardi <info@discophile.app>

Package: discophile
Architecture: amd64
Description: Your music server, played like a record collection
EOF
deps=$(cd "$work" && dpkg-shlibdeps -O -e"$binary" | sed -n 's/^shlibs:Depends=//p')
[[ -n $deps ]] || { echo 'No shared-library dependencies found.' >&2; exit 1; }
install -Dm755 "$binary" "$work/deb/usr/bin/discophile"
install -Dm644 "$repo/packaging/linux/discophile.desktop" "$work/deb/usr/share/applications/discophile.desktop"
install -Dm644 "$repo/packaging/linux/discophile.png" "$work/deb/usr/share/icons/hicolor/512x512/apps/discophile.png"
install -Dm644 "$repo/LICENSE" "$work/deb/usr/share/doc/discophile/copyright"
cat > "$work/deb/DEBIAN/control" <<EOF
Package: discophile
Version: $version
Architecture: amd64
Section: sound
Priority: optional
Maintainer: Andrea Debernardi <info@discophile.app>
Homepage: https://discophile.app
Installed-Size: $(du -sk "$work/deb/usr" | cut -f1)
Depends: $deps, libegl1, libgl1, libx11-6, libxcb1, libxcursor1, libxi6, libxrender1, libxrandr2, libxinerama1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libwayland-egl1
Recommends: hicolor-icon-theme, xdg-desktop-portal
Suggests: gnome-keyring, pipewire-alsa | libasound2-plugins
Description: Your music server, played like a record collection
 A free player for Navidrome, Subsonic, Jellyfin, Audiobookshelf and radio.
 Includes offline listening and optional encrypted Discophile Connect services.
EOF
dpkg-deb --root-owner-group --build "$work/deb" "$output/discophile_${version}_amd64.deb"

# Pin both the tool release and digest. Its AppImage output plugin is bundled inside.
curl --fail --location --retry 3 --output "$work/linuxdeploy.AppImage" \
  https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
echo "c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d  $work/linuxdeploy.AppImage" | sha256sum --check
# type2-runtime fedfad51; fail closed if the upstream continuous asset changes.
curl --fail --location --retry 3 --output "$work/runtime-x86_64" \
  https://github.com/AppImage/type2-runtime/releases/download/continuous/runtime-x86_64
echo "617aa77314d57c8b8e4fae9c3f0b35af7b30d5529cb69e98f64f92510ed98a46  $work/runtime-x86_64" | sha256sum --check
chmod +x "$work/linuxdeploy.AppImage"
(cd "$work" && ./linuxdeploy.AppImage --appimage-extract >/dev/null)
install -Dm644 "$repo/LICENSE" "$work/AppDir/usr/share/doc/discophile/copyright"
# ALSA and D-Bus are not baseline desktop libraries. The graphics driver stays on the host.
(cd "$work" && VERSION="$version" ARCH=x86_64 \
  LDAI_RUNTIME_FILE="$work/runtime-x86_64" LINUXDEPLOY_OUTPUT_VERSION="$version" \
  LDAI_OUTPUT="$output/Discophile-$version-x86_64.AppImage" \
  ./squashfs-root/AppRun --appdir "$work/AppDir" \
    --executable "$work/Discophile/discophile" \
    --desktop-file "$repo/packaging/linux/discophile.desktop" \
    --icon-file "$repo/packaging/linux/discophile.png" \
    --library /usr/lib/x86_64-linux-gnu/libasound.so.2 \
    --library /usr/lib/x86_64-linux-gnu/libdbus-1.so.3 \
    --library /usr/lib/x86_64-linux-gnu/libxkbcommon.so.0 \
    --library /usr/lib/x86_64-linux-gnu/libxkbcommon-x11.so.0 \
    --library /usr/lib/x86_64-linux-gnu/libXcursor.so.1 \
    --library /usr/lib/x86_64-linux-gnu/libXi.so.6 \
    --library /usr/lib/x86_64-linux-gnu/libXrandr.so.2 \
    --library /usr/lib/x86_64-linux-gnu/libXinerama.so.1 \
    --output appimage)
chmod +x "$output/Discophile-$version-x86_64.AppImage"
