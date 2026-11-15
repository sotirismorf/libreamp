# Changelog

All notable changes to LibreAmp are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (0.x: anything may change between minor versions).

## [Unreleased]

## [0.1.0] - 2026-11-15

First public release.

### Added

- Classic Winamp 2 skins (`.wsz`, `.zip` or unpacked folders), with missing parts filled in
  from the default skin. Shaped skins (`region.txt`) get see-through windows.
- Default skin: Winamp's classic base skin, rebranded LibreAmp.
- Skin collection: every skin loaded is copied to `~/.local/share/libreamp/skins` and listed
  in the right-click menu.
- Sizes of 100%, 150% and 200% (the default), the same on any display and pixel-sharp,
  including 150% on standard displays.
- Main window, equalizer and playlist editor, docked in one window.
- Album art panel (Ctrl+I): the cover embedded in the track, or a `cover`, `folder` or
  `front` image next to it, at full screen resolution. Framed with the skin's `gen.bmp`, or
  a plain border in its playlist colours; square by default and resizable.
- Right-click menu for the windows, visualizer, size and skin, with keyboard navigation.
- MP3, FLAC, Ogg Vorbis, M4A (AAC and ALAC) and WAV playback; M3U and PLS playlists.
- 10-band equalizer with Winamp's presets.
- Spectrum analyzer and oscilloscope.
- Playlist text in Terminus, covering Greek, Cyrillic and other non-Latin scripts.
- Remembers settings, playlist, skin and the playback position between runs.
- AppImage and Flatpak builds.

[Unreleased]: https://github.com/sotirismorf/libreamp/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sotirismorf/libreamp/releases/tag/v0.1.0
