# Solus Release Checklist

Use this checklist before publishing a production release tag.

## Scope

- Stable release target: `v1.x.y`
- Official release source: GitHub Releases
- Primary macOS install path: Homebrew tap (`brew install abdullah-binmadhi/tap/solus`)
- Primary Linux install path: GitHub binary or `cargo install --git https://github.com/abdullah-binmadhi/Solus.git`

## Pre-Release Validation

```bash
cargo check
cargo test
cargo clippy --all-targets
```

Manual smoke test:

- Launch in Kitty / WezTerm / Ghostty
- Verify `Home`, `Detail`, `Search`, `Help`, `Settings`, and `Setup` overlays
- Verify banner, card covers, and episode preview stills rendering
- Verify `Split-Deck` episode browser (`v`), `AniSkip` intro skips, and `AnimeFillerList` badges
- Verify `Tailored Discovery` questionnaire (`w` or `t`) and `Anime Wrapped` telemetry (`a`)
- Verify playback opens cleanly in `mpv` with live timestamp tracking and exact resume
- Verify offline episode downloader (`Shift+D`)

## Versioning

1. Update the version in `Cargo.toml`
2. Confirm the README install instructions match the current release plan
3. Confirm `Formula/solus.rb` version and URL matches the new release tag

## Tag + Release

1. Commit all release-ready changes
2. Create the tag:

```bash
git tag v1.0.6
git push origin v1.0.6
```

3. Wait for the GitHub Actions release workflow to build and attach:
   - `solus-macos-aarch64`
   - `solus-macos-x86_64`
   - `solus-linux-x86_64`

## Release Notes Checklist

Include:

- Stable TUI browsing, search, and resume flow
- Astrological Persona Matrix & Tailored Recommendation Crucible
- Offline batch episode downloader into `~/Movies/Solus/`
- Real-time Anime4K GLSL upscaling toggle
- Multi-source subtitle harvester and 2-tier automated sync engine
- Exact timestamp resume and 85% scrobble threshold via mpv IPC
- Settings overlay (`s`)
- Setup / dependency checks (`!`)
- Supported players: `mpv`, `iina`, `vlc`

## Post-Release

1. Update the Homebrew tap formula with the new version and SHA256
2. Verify install from:
   - Homebrew on macOS
   - release binary on Linux
3. Open a fresh shell and verify:

```bash
solus
```

4. Confirm the setup overlay and help overlay reflect the released keybindings
