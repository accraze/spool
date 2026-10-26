# Spool

Tape machine plugin (CLAP/VST3) in Rust using nih-plug. Single crate, all DSP in `src/lib.rs`.
The user runs it in Bitwig Studio on Linux (CLAP). Scaffolding, CI and editor widgets are copied from `../rodent`.

## Commands

```sh
cargo xtask bundle spool --release && cp target/bundled/Spool.clap ~/.clap/   # build + install
cargo test --release --lib -- --nocapture                                     # DSP sanity test
cargo clippy --release --lib
```

- The system has an old distro Rust (1.75) in `/usr/bin` that comes before `~/.cargo/bin` on PATH and
  can't build nih-plug. Prefix commands with `PATH="$HOME/.cargo/bin:$PATH"`.
- Bitwig needs the plugin re-added or a restart to pick up a new build.
- Add user-facing changes to `CHANGELOG.md` under `[Unreleased]` as you make them.
- Releases: follow `docs/RELEASING.md` (CI publishes on annotated `v*` tags; never move an existing tag).
- The "trailing semicolon in macro" warnings come from nih-plug's export macros, not our code.

## DSP structure

- Tuning values are named constants at the top of `lib.rs`.
- Per frame: `Transport::next` (shared by both channels, one tape) gives the wow/flutter delay and the
  dropout gain. Each channel pushes its input into its `DelayLine`, reads dry at the fixed base delay and
  tape at the modulated delay, then `Channel::process` runs emphasis → oversampled saturation → de-emphasis →
  head bump → hiss → HF loss.
- `BASE_DELAY` is reported as latency and must stay larger than `WOW_DEPTH + FLUTTER_DEPTH`
  (`wear_wobbles_and_stays_in_the_buffer` checks it). Delay buffers are allocated in `initialize()`,
  because `reset()` can run on the audio thread.
- `Coeffs::new(drive_db, colour, wear, fs)` maps knob values to coefficients; it's shared by `process()` and
  the tests, so change the mapping there only.
- Wear is mostly squared (wow depth, hiss) or cubed (dropout rate) so low settings stay subtle for mastering.
- GUI lives in `src/editor.rs`: a hand-painted reel-to-reel faceplate. `knob()`, `screw()` and window scaling
  come from Rodent (see `../rodent/CLAUDE.md`). The pilot lamp is the bypass button. Reels are static.
- README screenshot: same standalone recipe as `../wailer/CLAUDE.md` (match the `"Spool": ()` window).
- The audio thread must not allocate (`assert_process_allocs` is enabled).
- `CLAP_ID` and `VST3_CLASS_ID` must never change, or saved Bitwig projects lose the plugin.
