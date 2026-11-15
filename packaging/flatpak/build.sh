#!/usr/bin/env bash
# Builds the Flatpak and exports a single-file bundle to dist/.
#
# Needs flatpak, flatpak-builder and network access; the SDK is installed from Flathub.
set -euo pipefail
cd "$(dirname "$0")/../.."

APP_ID=io.github.sotirismorf.LibreAmp
MANIFEST=packaging/flatpak/$APP_ID.yml
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

mkdir -p build dist
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
    --repo=build/repo \
    build/build-dir "$MANIFEST"
flatpak build-bundle build/repo "dist/LibreAmp-$VERSION-x86_64.flatpak" "$APP_ID" \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
echo "dist/LibreAmp-$VERSION-x86_64.flatpak"
