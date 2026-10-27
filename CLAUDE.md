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
- `BASE_DELAY` is reported as latency and must stay larger than `4 * WOW_DEPTH + 2 * FLUTTER_DEPTH` (trims at 200%)
  (`wear_wobbles_and_stays_in_the_buffer` checks it). Delay buffers are allocated in `initialize()`,
  because `reset()` can run on the audio thread.
- `Wear` bundles the Wear amount (0..1) with the Wow/Flutter/Dropouts/Hiss trims (0..2, default 1). Each part runs
  its curve on `amount * trim`, so all-1 trims sound exactly like 0.1.0's single knob; `Wear::all(x)` is that case.
- `Coeffs::new(drive_db, colour, &wear, fs)` maps knob values to coefficients; it's shared by `process()` and
  the tests, so change the mapping there only.
- Wear is mostly squared (wow depth, hiss) or cubed (dropout rate) so low settings stay subtle for mastering.
- GUI lives in `src/editor.rs`: a hand-painted reel-to-reel faceplate. `knob()`, `screw()` and window scaling
  come from Rodent (see `../rodent/CLAUDE.md`). The pilot lamp is the bypass button.
  `knob()` takes a radius: 28 for the main row, 18 for the Wear trims row.
- `PRESETS` in `lib.rs` are tape types expressed in the knobs (cleanest first). The first must equal the param
  defaults (`first_preset_is_the_default`); `output_db` is level-matched to it by `presets_are_level_matched`
  (`--nocapture` prints every offset), so re-run it after edits. The selector's matching and applying both go
  through `preset_values()` in `editor.rs`, so add new params there too.
- `Meters` (in `lib.rs`, an `Arc` shared with the editor) carries each channel's mean-square output as `AtomicU32` bits
  and whether the reels should run (host playing and not bypassed). `process()` only stores them while the editor is
  open. The editor redraws every frame, keeps the tape position in egui memory and turns each reel by
  `travel / pack radius`, wrapping at `TAPE_WRAP` so the angle never jumps. VU needles map linear voltage, not dB.
- README screenshot: same standalone recipe as `../wailer/CLAUDE.md` (match the `"Spool": ()` window).
- The audio thread must not allocate (`assert_process_allocs` is enabled).
- `CLAP_ID` and `VST3_CLASS_ID` must never change, or saved Bitwig projects lose the plugin.
