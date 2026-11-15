# Releasing

Releases are tagged `vX.Y.Z` and built by `.github/workflows/release.yml`: an AppImage
(built on Ubuntu 22.04), a Flatpak bundle and `SHA256SUMS`, attached to a GitHub release whose
notes come from `CHANGELOG.md`. Tags with a `-` (`v0.2.0-rc.1`) become pre-releases.

While on 0.1.x, bump the patch version for fixes and the minor version for anything bigger.

1. Update the version in three places; the workflow refuses to publish if they disagree with
   the tag:
   - `version` in `Cargo.toml` (then run `cargo check` so `Cargo.lock` follows),
   - a new `<release version="X.Y.Z" date="YYYY-MM-DD">` at the top of `<releases>` in
     `packaging/io.github.sotirismorf.LibreAmp.metainfo.xml`,
   - a `## [X.Y.Z] - YYYY-MM-DD` section in `CHANGELOG.md`, moved from `[Unreleased]`, with
     the comparison links at the bottom updated.
2. Commit, then tag and push:

   ```sh
   git tag -a vX.Y.Z -m "LibreAmp X.Y.Z"
   git push origin main vX.Y.Z
   ```

3. Check the release on GitHub once the workflow finishes.
