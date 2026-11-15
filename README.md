# LibreAmp

A music player for Linux (Wayland first) that uses classic Winamp `.wsz` skins and scales
them by whole numbers with nearest-neighbour sampling, so skins stay sharp on HiDPI screens.

The UI is drawn at 1x into a small framebuffer and upscaled by SDL3; audio is decoded with
symphonia and played through SDL3.

## Install

Download from the [latest release](https://github.com/sotirismorf/libreamp/releases/latest):

- **AppImage**: `chmod +x LibreAmp-*-x86_64.AppImage` and run it. It needs glibc 2.35 or
  newer (Ubuntu 22.04, Debian 12, Fedora 36 and later).
- **Flatpak**: `flatpak install --user LibreAmp-*-x86_64.flatpak`, then start LibreAmp from
  your application menu or with `flatpak run io.github.sotirismorf.LibreAmp`. The runtime is
  fetched from Flathub, which needs to be set up as a remote.

The Flatpak can read your home folder and mounted drives, but not change them; playlists are
saved wherever you choose in the save dialog.

## Features

- Classic skins: `.wsz`/`.zip` archives or unpacked skin folders. Anything a skin lacks comes
  from the bundled default skin.
- Integer scaling that follows the display's scale factor (Ctrl + / Ctrl − to change).
- Main window, equalizer and playlist, docked in one window, plus an optional album art panel
  framed in the skin's own style.
- MP3, FLAC, Ogg Vorbis, M4A (AAC/ALAC) and WAV; M3U and PLS playlists.
- 10-band equalizer with Winamp's built-in presets.
- Spectrum analyzer and oscilloscope in skin colours.
- Album art from the file's tags (MP3, FLAC, Ogg, M4A) or a `cover`/`folder`/`front` image
  next to it, drawn at full screen resolution.
- Playlist text in Terminus, so non-Latin titles (Greek, Cyrillic, …) are readable.
- Remembers the playlist, EQ, volume, skin and window layout in `~/.config/libreamp`
  (`~/.var/app/io.github.sotirismorf.LibreAmp/config/libreamp` for the Flatpak).

## Using it

```sh
libreamp [--scale N] [skin.wsz] [files, folders or playlists...]
```

Drop audio files, folders or playlists on the window to play them; while the playlist is open
they are added to it instead. Drop a `.wsz` to change skin. Drag title bars to move the window.

| Key | Action |
|---|---|
| Z X C V B | Previous, play, pause, stop, next |
| L | Open files |
| Ctrl+E / Ctrl+G / Ctrl+I | Show or hide the playlist / equalizer / album art |
| Ctrl + / Ctrl − | Scale up / down |
| Ctrl+Q | Quit |
| Enter, Delete, arrows, Ctrl+A | Play, remove, move and select in the playlist |

Click the visualizer to switch between spectrum, oscilloscope and off.

## Building

Requires Rust 1.88 or newer and SDL 3.2 or newer (`sdl3` on Arch, `SDL3-devel` on Fedora).

```sh
cargo build --release
./target/release/libreamp
```

Without SDL 3 installed, `cargo build --release --features static-sdl` builds it from source
and links it in. That needs CMake and SDL's build dependencies;
`packaging/install-sdl-build-deps.sh` installs them on Debian and Ubuntu.

### Packages

- `packaging/appimage/build.sh` builds `dist/LibreAmp-<version>-x86_64.AppImage`.
- `packaging/flatpak/build.sh` builds `dist/LibreAmp-<version>-x86_64.flatpak` (needs
  `flatpak-builder`).

Pushing a `v*` tag builds both on GitHub Actions and publishes a release; see
[RELEASING.md](RELEASING.md).

## License

LibreAmp is free software under the [GNU General Public License v3.0 or later](LICENSE).

## Credits

- Sprite coordinates and layout follow [Webamp](https://github.com/captbaritone/webamp).
- `skins/libreamp-base.wsz` is Winamp's classic base skin (`base-2.91.wsz`, as distributed with
  Webamp), with its title bars relettered and its logo redrawn by `tools/rebrand_skin.py`. The
  same script writes the application icon from the redrawn logo:
  `tools/rebrand_skin.py base-2.91.wsz skins/libreamp-base.wsz --icon packaging/io.github.sotirismorf.LibreAmp`.
- `assets/terminus-6x12.bin` is converted from [Terminus Font](https://terminus-font.sourceforge.net/)
  by Dimitar Zhekov, under the SIL Open Font License (`assets/terminus-OFL.txt`). Regenerate it
  with `tools/bdf2bin.py ter-u12n.bdf assets/terminus-6x12.bin`.
