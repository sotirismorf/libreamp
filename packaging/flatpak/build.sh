#!/usr/bin/env bash
# Builds the Flatpak and exports a single-file bundle to dist/.
#
# Needs flatpak, flatpak-builder, Python 3 with aiohttp and tomlkit (for
# flatpak-cargo-generator), and the Flathub remote (the SDK is installed from it).
set -euo pipefail
cd "$(dirname "$0")/../.."

APP_ID=io.github.sotirismorf.LibreAmp
MANIFEST=packaging/flatpak/$APP_ID.yml
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
# flatpak-builder-tools has no releases; pin the generator to a known commit when it changes.
GENERATOR_URL=${GENERATOR_URL:-https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py}

mkdir -p build dist
if [[ ! -f build/flatpak-cargo-generator.py ]]; then
    curl -fsSL "$GENERATOR_URL" -o build/flatpak-cargo-generator.py
fi
# Offline cargo sources for the build sandbox, from Cargo.lock.
python3 build/flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json

flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak-builder --user --install-deps-from=flathub --force-clean --disable-rofiles-fuse \
    --repo=build/repo \
    build/build-dir "$MANIFEST"
flatpak build-bundle build/repo "dist/LibreAmp-$VERSION-x86_64.flatpak" "$APP_ID" \
    --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo
echo "dist/LibreAmp-$VERSION-x86_64.flatpak"
