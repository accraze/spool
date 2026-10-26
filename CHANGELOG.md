# Changelog

All notable changes to Spool are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- Wow, Flutter, Dropouts and Hiss trims under the main knobs, 0–200%. Each one sets how hard its
  part of Wear is pushed, so you can have a wobbly tape without hiss, lots of wow at low Wear, or
  go past the old maximum (up to 4x the wow depth). They default to 100%, so existing projects
  sound the same.

### Changed
- Latency is now 25 ms (was 7 ms) to make room for the deeper wow. Hosts compensate for it.
- The window is taller to fit the trims. Projects saved with 0.1.0 open at the new default size.

## [0.1.0] - 2026-10-26

### Added
- Tape machine model: pre-emphasised tape saturation at 4x oversampling, head bump, high-frequency
  loss, wow and flutter, hiss and dropouts.
- Drive, Colour, Wear and Output knobs. Drive is level-compensated, so quiet material keeps its
  level and only the peaks get squashed.
- Bypass on the pilot lamp, with a click-free crossfade and host bypass integration. Reports 7 ms
  of latency (the wow/flutter delay line), and bypass stays time-aligned.
- Hand-drawn reel-to-reel GUI with resizable window.

[Unreleased]: https://github.com/accraze/spool/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/accraze/spool/releases/tag/v0.1.0
