# Changelog

All notable changes to LibreAmp are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/) (0.x: anything may change between minor versions).

## [Unreleased]

### Added

- Album art panel (Ctrl+I), docked below the playlist and framed with the skin's generic
  window graphics (`gen.bmp`); skins without them get a plain border in their playlist
  colours instead of the default skin's frame. Shows the cover embedded in the track, or a
  `cover`, `folder` or `front` image next to it, at full screen resolution. Square by
  default; resizable from its bottom-right corner (dragging back near square snaps to
  square); remembered between runs.
- Right-click menu: show or hide the equalizer, playlist and album art; choose the
  visualizer, scale and skin, including going back to the default skin or opening one with
  a file dialog. Works with the mouse or the arrow keys. Submenus stay open while the
  pointer heads into them, even across other items.
- Skin collection: every skin loaded (dropped, opened or given on the command line) is copied
  to `~/.local/share/libreamp/skins`, without duplicates, and listed in the Skin submenu.
  Menus too long for the window scroll.
- Shaped skins: the parts a skin's `region.txt` cuts out of the main window and equalizer
  are see-through (they were black) and ignore clicks.
- Resumes where you left off: the track that was playing or paused at exit comes back paused
  at the same position.

### Changed

- Sizes are now 100%, 150% and 200% of the classic skin (default 200%), the same on any
  display. 150% on a standard display stays sharp: it is drawn at 200% and shrunk smoothly
  rather than with uneven pixels. `--scale` takes a percentage; a previously saved scale
  resets to 200%.

## [0.1.0] - 2026-11-15

First public release.

### Added

- Classic Winamp 2 skins (`.wsz`, `.zip` or unpacked folders), with missing parts filled in
  from the default skin.
- Default skin: Winamp's classic base skin, rebranded LibreAmp.
- Integer scaling with nearest-neighbour sampling that follows the display's scale factor.
- Main window, equalizer and playlist editor, docked in one window.
- MP3, FLAC, Ogg Vorbis, M4A (AAC and ALAC) and WAV playback; M3U and PLS playlists.
- 10-band equalizer with Winamp's presets.
- Spectrum analyzer and oscilloscope.
- Playlist text in Terminus, covering Greek, Cyrillic and other non-Latin scripts.
- Settings, playlist and skin remembered between runs.
- AppImage and Flatpak builds.

[Unreleased]: https://github.com/sotirismorf/libreamp/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/sotirismorf/libreamp/releases/tag/v0.1.0
