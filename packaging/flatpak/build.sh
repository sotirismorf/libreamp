#!/usr/bin/env bash
# Builds the Flatpak and exports a single-file bundle to dist/.
#
# Needs flatpak, network access and a flatpak-builder: Flathub's packaged one
# (org.flatpak.Builder) when installed, as in CI, since it brings current AppStream tooling;
# otherwise the system's flatpak-builder. The SDK is installed from Flathub.
set -euo pipefail
cd "$(dirname "$0")/../.."

APP_ID=io.github.sotirismorf.LibreAmp
MANIFEST=packaging/flatpak/$APP_ID.yml
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

if flatpak info org.flatpak.Builder >/dev/null 2>&1; then
    BUILDER=(flatpak run org.flatpak.Builder)
else
    BUILDER=(flatpak-builder)
fi

mkdir -p build dist
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
"${BUILDER[@]}" --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
    --repo=build/repo \
    build/build-dir "$MANIFEST"
flatpak build-bundle build/repo "dist/LibreAmp-$VERSION-x86_64.flatpak" "$APP_ID" \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
echo "dist/LibreAmp-$VERSION-x86_64.flatpak"
