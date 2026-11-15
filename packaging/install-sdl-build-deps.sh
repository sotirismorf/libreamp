#!/usr/bin/env bash
# Installs what building SDL 3 from source (cargo feature static-sdl) needs on Debian/Ubuntu.
# SDL loads these libraries at run time; the headers decide which backends get built in.
set -euo pipefail
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
    cmake pkg-config \
    libasound2-dev libpulse-dev libpipewire-0.3-dev \
    libwayland-dev wayland-protocols libdecor-0-dev libxkbcommon-dev \
    libx11-dev libxext-dev libxrandr-dev libxcursor-dev libxfixes-dev libxi-dev libxss-dev libxtst-dev \
    libdrm-dev libgbm-dev libgl-dev libegl-dev libgles-dev \
    libdbus-1-dev libudev-dev
