#!/usr/bin/env bash
# Builds the AppImage into dist/.
#
# SDL 3 is built from source and linked statically (cargo feature static-sdl); it loads
# Wayland, X11, PipeWire/PulseAudio and the rest from the host at run time. Build on the
# oldest distribution you want to support (CI uses Ubuntu 22.04), since glibc is not bundled.
# Needs cmake and the SDL build dependencies; see .github/workflows/release.yml.
set -euo pipefail
cd "$(dirname "$0")/../.."

APP_ID=io.github.sotirismorf.LibreAmp
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
ARCH=$(uname -m)
APPDIR=build/AppDir
OUT=dist/LibreAmp-$VERSION-$ARCH.AppImage

cargo build --release --locked --features static-sdl

rm -rf "$APPDIR"
install -Dm755 target/release/libreamp "$APPDIR/usr/bin/libreamp"
install -Dm644 packaging/$APP_ID.desktop "$APPDIR/usr/share/applications/$APP_ID.desktop"
install -Dm644 packaging/$APP_ID.metainfo.xml "$APPDIR/usr/share/metainfo/$APP_ID.appdata.xml"
install -Dm644 packaging/$APP_ID.svg "$APPDIR/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 packaging/$APP_ID.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"
cp packaging/$APP_ID.desktop packaging/$APP_ID.png "$APPDIR/"
ln -s $APP_ID.png "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/libreamp" "$@"
APPRUN
chmod +x "$APPDIR/AppRun"

mkdir -p build dist
TOOL=build/appimagetool-$ARCH.AppImage
if [[ ! -x $TOOL ]]; then
    curl -fsSL -o "$TOOL" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
    chmod +x "$TOOL"
fi
# Lets AppImageUpdate find newer releases on GitHub.
UPDATE=()
if [[ -n ${GITHUB_REPOSITORY:-} ]]; then
    UPDATE=(-u "gh-releases-zsync|${GITHUB_REPOSITORY%%/*}|${GITHUB_REPOSITORY##*/}|latest|LibreAmp-*-$ARCH.AppImage.zsync")
fi
# Runs without FUSE, as in CI containers.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=$ARCH "$TOOL" "${UPDATE[@]}" "$APPDIR" "$OUT"
if [[ -f $(basename "$OUT").zsync ]]; then
    mv "$(basename "$OUT").zsync" dist/
fi
echo "$OUT"
