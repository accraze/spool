//! A tape machine.
//!
//! Signal chain:
//! 1. Transport: a modulated delay line gives wow (slow, drifting) and flutter (fast). Its fixed
//!    base delay is reported as latency, and the dry signal is read at that base so bypass lines up.
//! 2. Record head: pre-emphasis, tape saturation at `OVERSAMPLING`x, then de-emphasis, so the
//!    highs saturate first like real tape. Drive is compensated so quiet signals keep their level.
//! 3. Playback: head bump and high-frequency loss (Colour, Wear), hiss and dropouts (Wear, each
//!    part of Wear also has its own trim).
//! 4. Output.

use nih_plug::prelude::*;
use nih_plug_egui::EguiState;
use std::f32::consts::{PI, TAU};
use std::sync::Arc;

mod editor;

const OVERSAMPLING: usize = 4;

// Transport, delays in seconds. BASE_DELAY must exceed the full modulation depth, which is
// 4x WOW_DEPTH plus 2x FLUTTER_DEPTH with both trims at 200%.
const BASE_DELAY: f32 = 0.025;
const WOW_DEPTH: f32 = 0.006;
const WOW_RATE: f32 = 0.5;
const DRIFT_FC: f32 = 0.3;
const FLUTTER_DEPTH: f32 = 0.00015;
const FLUTTER_RATE: f32 = 7.0;
/// Dropouts per second at full wear
const DROPOUT_RATE: f32 = 2.0;
const DROPOUT_FADE: f32 = 0.005;

// Record head
/// High shelf boost before the saturator, +9.5 dB
const EMPHASIS: f32 = 2.0;
const EMPHASIS_FC: f32 = 3000.0;
/// Offsets the saturator for some even harmonics
const BIAS: f32 = 0.15;
const DC_FC: f32 = 10.0;

// Playback head
const BUMP_FC: f32 = 90.0;
const BUMP_Q: f32 = 1.0;
const BUMP_DB: f32 = 4.0;
const HF_OPEN: f32 = 20e3;
/// HF cutoff at full colour, before wear darkens it further
const HF_DARK: f32 = 5e3;
const HF_WORN: f32 = 0.4;
/// Hiss level at full wear, -40 dB
const HISS: f32 = 0.01;

/// Bypass crossfade time in seconds, long enough to avoid clicks
const BYPASS_FADE: f32 = 0.01;

/// Topology-preserving one-pole filter, gives both low-pass and high-pass outputs.
#[derive(Default, Clone, Copy)]
struct OnePole {
    s: f32,
}

impl OnePole {
    /// `g` is `tan(pi * fc / fs)`. Returns the low-pass output.
    fn lp(&mut self, x: f32, g: f32) -> f32 {
        let v = (x - self.s) * g / (1.0 + g);
        let y = v + self.s;
        self.s = y + v;
        y
    }

    fn hp(&mut self, x: f32, g: f32) -> f32 {
        x - self.lp(x, g)
    }
}

fn prewarp(fc: f32, fs: f32) -> f32 {
    (PI * fc.min(fs * 0.45) / fs).tan()
}

/// Normalized biquad coefficients, kept apart from the state so they can change every sample.
#[derive(Default, Clone, Copy)]
struct Bq {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Bq {
    fn lowpass(fc: f32, fs: f32, q: f32) -> Self {
        let (sin, cos) = (2.0 * PI * fc.min(fs * 0.45) / fs).sin_cos();
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha;
        Self {
            b0: (1.0 - cos) / 2.0 / a0,
            b1: (1.0 - cos) / a0,
            b2: (1.0 - cos) / 2.0 / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha) / a0,
        }
    }

    fn peak(fc: f32, fs: f32, q: f32, db: f32) -> Self {
        let (sin, cos) = (2.0 * PI * fc / fs).sin_cos();
        let a = 10f32.powf(db / 40.0);
        let alpha = sin / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        Self {
            b0: (1.0 + alpha * a) / a0,
            b1: -2.0 * cos / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: -2.0 * cos / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Biquad {
    z1: f32,
    z2: f32,
}

impl Biquad {
    fn process(&mut self, x: f32, c: &Bq) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = c.b1 * x - c.a1 * y + self.z2;
        self.z2 = c.b2 * x - c.a2 * y;
        y
    }
}

/// 8th order Butterworth low-pass used as the anti-imaging/anti-aliasing filter.
#[derive(Default, Clone, Copy)]
struct AntiAlias {
    coeffs: [Bq; 4],
    state: [Biquad; 4],
}

impl AntiAlias {
    fn new(fs_base: f32) -> Self {
        let fs = fs_base * OVERSAMPLING as f32;
        let fc = (fs_base * 0.45).min(20e3);
        let qs = [0.5098, 0.6013, 0.9000, 2.5629];
        Self {
            coeffs: qs.map(|q| Bq::lowpass(fc, fs, q)),
            state: Default::default(),
        }
    }

    fn process(&mut self, x: f32) -> f32 {
        self.state
            .iter_mut()
            .zip(&self.coeffs)
            .fold(x, |x, (s, c)| s.process(x, c))
    }
}

/// Circular buffer read with 4-point Hermite interpolation.
#[derive(Default)]
struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    fn push(&mut self, x: f32) {
        self.pos = (self.pos + 1) % self.buf.len();
        self.buf[self.pos] = x;
    }

    /// `delay` in samples, at least 1 and less than the buffer length minus 2.
    fn read(&self, delay: f32) -> f32 {
        let len = self.buf.len();
        let d = delay as usize;
        let t = delay - d as f32;
        let s = |k: usize| self.buf[(self.pos + len - k) % len];
        let (ym1, y0, y1, y2) = (s(d - 1), s(d), s(d + 1), s(d + 2));
        let c1 = 0.5 * (y1 - ym1);
        let c2 = ym1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
        let c3 = 0.5 * (y2 - ym1) + 1.5 * (y0 - y1);
        ((c3 * t + c2) * t + c1) * t + y0
    }
}

/// Wear amount (0..1) plus the trims for each of its parts (0..2). A part acts as if Wear were
/// `amount * trim`, so a trim of 1 reproduces the single Wear knob of 0.1.0.
#[derive(Clone, Copy)]
struct Wear {
    amount: f32,
    wow: f32,
    flutter: f32,
    dropouts: f32,
    hiss: f32,
}

impl Wear {
    /// Wear with every trim at 100%
    #[cfg(test)]
    fn all(amount: f32) -> Self {
        Self {
            amount,
            wow: 1.0,
            flutter: 1.0,
            dropouts: 1.0,
            hiss: 1.0,
        }
    }
}

/// Tape motion shared by both channels: one tape, one capstan.
struct Transport {
    wow_phase: f32,
    flutter_phase: f32,
    drift: OnePole,
    dropout_left: usize,
    dropout_target: f32,
    dropout_gain: f32,
    rng: u32,
}

impl Default for Transport {
    fn default() -> Self {
        Self {
            wow_phase: 0.0,
            flutter_phase: 0.0,
            drift: OnePole::default(),
            dropout_left: 0,
            dropout_target: 1.0,
            dropout_gain: 1.0,
            rng: 0x2545_f491,
        }
    }
}

impl Transport {
    /// White noise in [-1, 1) (xorshift32)
    fn noise(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng as f32 / 2_147_483_648.0 - 1.0
    }

    /// Returns the playback delay in samples and the dropout gain.
    fn next(&mut self, w: &Wear, base: f32, fs: f32) -> (f32, f32) {
        self.wow_phase = (self.wow_phase + TAU * WOW_RATE / fs) % TAU;
        self.flutter_phase = (self.flutter_phase + TAU * FLUTTER_RATE / fs) % TAU;

        // Low-passed noise scaled back to roughly unit variance, so the wow wanders off its sine
        let g = prewarp(DRIFT_FC, fs);
        let noise = self.noise();
        let drift = (self.drift.lp(noise, g) * (1.5 / g).sqrt()).clamp(-1.0, 1.0);
        let wow = 0.7 * self.wow_phase.sin() + 0.3 * drift;
        let (wow_amount, flutter_amount) = (w.amount * w.wow, w.amount * w.flutter);
        let delay = base
            + wow_amount * wow_amount * WOW_DEPTH * fs * wow
            + flutter_amount * FLUTTER_DEPTH * fs * self.flutter_phase.sin();

        if self.dropout_left > 0 {
            self.dropout_left -= 1;
        } else {
            self.dropout_target = 1.0;
            let a = w.amount * w.dropouts;
            if (self.noise() + 1.0) * 0.5 < a * a * a * DROPOUT_RATE / fs {
                self.dropout_left = ((0.03 + 0.12 * (self.noise() + 1.0) * 0.5) * fs) as usize;
                // Past 100% the deepest dropouts go fully silent
                self.dropout_target = (1.0 - a * (0.45 + 0.25 * self.noise())).max(0.0);
            }
        }
        self.dropout_gain +=
            (self.dropout_target - self.dropout_gain) / (DROPOUT_FADE * fs).max(1.0);

        (delay, self.dropout_gain)
    }
}

/// Coefficients shared by both channels, recomputed per sample from the smoothed parameters.
struct Coeffs {
    drive: f32,
    g_emph: f32,
    g_deemph: f32,
    g_dc: f32,
    bump: Bq,
    hf: Bq,
    hiss: f32,
}

impl Coeffs {
    fn new(drive_db: f32, colour: f32, wear: &Wear, fs: f32) -> Self {
        let hf_fc = HF_OPEN * (HF_DARK / HF_OPEN).powf(colour) * HF_WORN.powf(wear.amount);
        Self {
            drive: util::db_to_gain(drive_db),
            g_emph: prewarp(EMPHASIS_FC, fs),
            // The exact inverse of the emphasis shelf has its corner lower by 1 + EMPHASIS
            g_deemph: prewarp(EMPHASIS_FC / (1.0 + EMPHASIS), fs),
            g_dc: prewarp(DC_FC, fs),
            bump: Bq::peak(BUMP_FC, fs, BUMP_Q, BUMP_DB * colour),
            hf: Bq::lowpass(hf_fc, fs, 0.707),
            hiss: HISS * (wear.amount * wear.hiss).powi(2),
        }
    }
}

#[derive(Default)]
struct Channel {
    delay: DelayLine,
    emph: OnePole,
    deemph: OnePole,
    up: AntiAlias,
    down: AntiAlias,
    dc: OnePole,
    bump: Biquad,
    hf: Biquad,
}

impl Channel {
    fn new(fs: f32, base: usize) -> Self {
        let aa = AntiAlias::new(fs);
        Self {
            // Room for the base delay plus the full modulation depth and the interpolator's taps
            delay: DelayLine {
                buf: vec![0.0; 2 * base + 4],
                pos: 0,
            },
            up: aa,
            down: aa,
            ..Self::default()
        }
    }

    /// `x` is the tape signal read from the delay line, `noise` this channel's hiss sample.
    fn process(&mut self, x: f32, c: &Coeffs, noise: f32) -> f32 {
        let x = x + EMPHASIS * self.emph.hp(x, c.g_emph);
        let mut out = 0.0;
        for i in 0..OVERSAMPLING {
            // Zero-stuffing upsampler, gain compensated
            let u = self
                .up
                .process(if i == 0 { x * OVERSAMPLING as f32 } else { 0.0 });
            let sat = ((u * c.drive + BIAS).tanh() - BIAS.tanh()) / c.drive;
            out = self.down.process(sat);
        }
        let y = out - EMPHASIS / (1.0 + EMPHASIS) * self.deemph.hp(out, c.g_deemph);
        let y = self.dc.hp(y, c.g_dc);
        let y = self.bump.process(y, &c.bump) + c.hiss * noise;
        self.hf.process(y, &c.hf)
    }
}

struct Spool {
    params: Arc<SpoolParams>,
    sample_rate: f32,
    /// Base delay in whole samples, so the dry read is exact
    base_delay: usize,
    transport: Transport,
    channels: [Channel; 2],
    /// 0 = bypassed, 1 = fully processed
    wet: f32,
}

#[derive(Params)]
struct SpoolParams {
    // Renamed from "editor-size" so projects saved with the 0.1.0 layout open at the taller size
    #[persist = "editor-size-2"]
    editor_state: Arc<EguiState>,

    #[id = "drive"]
    drive: FloatParam,
    #[id = "colour"]
    colour: FloatParam,
    #[id = "wear"]
    wear: FloatParam,
    #[id = "wow"]
    wow: FloatParam,
    #[id = "flutter"]
    flutter: FloatParam,
    #[id = "dropouts"]
    dropouts: FloatParam,
    #[id = "hiss"]
    hiss: FloatParam,
    #[id = "output"]
    output: FloatParam,
    #[id = "bypass"]
    bypass: BoolParam,
}

fn knob(name: &str, default: f32) -> FloatParam {
    FloatParam::new(name, default, FloatRange::Linear { min: 0.0, max: 1.0 })
        .with_smoother(SmoothingStyle::Linear(20.0))
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

/// Scales one part of Wear, 0-200%. 100% is the knob's centre and its default.
fn trim(name: &str) -> FloatParam {
    FloatParam::new(name, 1.0, FloatRange::Linear { min: 0.0, max: 2.0 })
        .with_smoother(SmoothingStyle::Linear(20.0))
        .with_unit("%")
        .with_value_to_string(formatters::v2s_f32_percentage(0))
        .with_string_to_value(formatters::s2v_f32_percentage())
}

impl Default for SpoolParams {
    fn default() -> Self {
        Self {
            editor_state: EguiState::from_size(editor::WIDTH, editor::HEIGHT),
            drive: FloatParam::new(
                "Drive",
                3.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 24.0,
                },
            )
            .with_smoother(SmoothingStyle::Linear(20.0))
            .with_step_size(0.1)
            .with_unit(" dB"),
            colour: knob("Colour", 0.3),
            wear: knob("Wear", 0.1),
            wow: trim("Wow"),
            flutter: trim("Flutter"),
            dropouts: trim("Dropouts"),
            hiss: trim("Hiss"),
            output: FloatParam::new(
                "Output",
                util::db_to_gain(0.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-12.0),
                    max: util::db_to_gain(12.0),
                    factor: FloatRange::gain_skew_factor(-12.0, 12.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(50.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            bypass: BoolParam::new("Bypass", false).make_bypass(),
        }
    }
}

impl Default for Spool {
    fn default() -> Self {
        Self {
            params: Arc::new(SpoolParams::default()),
            sample_rate: 48000.0,
            base_delay: 0,
            transport: Transport::default(),
            channels: Default::default(),
            wet: 1.0,
        }
    }
}

impl Plugin for Spool {
    const NAME: &'static str = "Spool";
    const VENDOR: &'static str = "accraze";
    const URL: &'static str = "";
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(2),
            main_output_channels: NonZeroU32::new(2),
            ..AudioIOLayout::const_default()
        },
        AudioIOLayout {
            main_input_channels: NonZeroU32::new(1),
            main_output_channels: NonZeroU32::new(1),
            ..AudioIOLayout::const_default()
        },
    ];

    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        editor::create(self.params.clone())
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        self.base_delay = (BASE_DELAY * self.sample_rate).round() as usize;
        context.set_latency_samples(self.base_delay as u32);
        // Allocate here: reset() can run on the audio thread
        self.channels = [(); 2].map(|_| Channel::new(self.sample_rate, self.base_delay));
        self.reset();
        true
    }

    fn reset(&mut self) {
        let aa = AntiAlias::new(self.sample_rate);
        for ch in &mut self.channels {
            let buf = std::mem::take(&mut ch.delay.buf);
            *ch = Channel {
                delay: DelayLine { buf, pos: 0 },
                up: aa,
                down: aa,
                ..Channel::default()
            };
            ch.delay.buf.fill(0.0);
        }
        self.transport = Transport::default();
        self.wet = if self.params.bypass.value() { 0.0 } else { 1.0 };
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let fs = self.sample_rate;
        let base = self.base_delay as f32;
        let fade_step = 1.0 / (BYPASS_FADE * fs);

        for mut frame in buffer.iter_samples() {
            let wear = Wear {
                amount: self.params.wear.smoothed.next(),
                wow: self.params.wow.smoothed.next(),
                flutter: self.params.flutter.smoothed.next(),
                dropouts: self.params.dropouts.smoothed.next(),
                hiss: self.params.hiss.smoothed.next(),
            };
            let coeffs = Coeffs::new(
                self.params.drive.smoothed.next(),
                self.params.colour.smoothed.next(),
                &wear,
                fs,
            );
            let output = self.params.output.smoothed.next();
            let (delay, dropout) = self.transport.next(&wear, base, fs);
            let target = if self.params.bypass.value() { 0.0 } else { 1.0 };
            self.wet += (target - self.wet).clamp(-fade_step, fade_step);

            // Keep processing while bypassed so the tape path stays warm for a click-free return
            for (sample, channel) in frame.iter_mut().zip(self.channels.iter_mut()) {
                channel.delay.push(*sample);
                let dry = channel.delay.read(base);
                let tape = channel.delay.read(delay);
                let noise = self.transport.noise();
                let wet = channel.process(tape, &coeffs, noise) * dropout * output;
                *sample = dry + (wet - dry) * self.wet;
            }
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for Spool {
    const CLAP_ID: &'static str = "com.accraze.spool";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Tape machine: drive, wear and colour");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Mono,
        ClapFeature::Distortion,
        ClapFeature::Mastering,
    ];
}

impl Vst3Plugin for Spool {
    const VST3_CLASS_ID: [u8; 16] = *b"SpoolTapeMachine";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] = &[
        Vst3SubCategory::Fx,
        Vst3SubCategory::Distortion,
        Vst3SubCategory::Mastering,
    ];
}

nih_export_clap!(Spool);
nih_export_vst3!(Spool);

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs `secs` of a sine through one channel the way `process()` does and returns the output,
    /// skipping the first 10% while filters settle.
    fn render(drive: f32, colour: f32, wear: f32, fs: f32, freq: f32, amp: f32) -> Vec<f32> {
        render_worn(drive, colour, Wear::all(wear), fs, freq, amp)
    }

    fn render_worn(drive: f32, colour: f32, wear: Wear, fs: f32, freq: f32, amp: f32) -> Vec<f32> {
        let base = (BASE_DELAY * fs).round() as usize;
        let c = Coeffs::new(drive, colour, &wear, fs);
        let mut ch = Channel::new(fs, base);
        let mut tr = Transport::default();
        let n = fs as usize;
        (0..n)
            .map(|i| {
                let (delay, dropout) = tr.next(&wear, base as f32, fs);
                ch.delay.push(amp * (2.0 * PI * freq * i as f32 / fs).sin());
                let tape = ch.delay.read(delay);
                ch.process(tape, &c, tr.noise()) * dropout
            })
            .skip(n / 10)
            .collect()
    }

    fn peak(x: &[f32]) -> f32 {
        x.iter().fold(0.0, |m, y| m.max(y.abs()))
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|y| y * y).sum::<f32>() / x.len() as f32).sqrt()
    }

    #[test]
    fn output_is_bounded_and_not_silent() {
        for fs in [44100.0, 48000.0, 96000.0, 192000.0] {
            for (drive, colour, wear) in [(0.0, 0.0, 0.0), (3.0, 0.3, 0.1), (24.0, 1.0, 1.0)] {
                let out = render(drive, colour, wear, fs, 220.0, 0.9);
                assert!(out.iter().all(|y| y.is_finite()));
                let peak = peak(&out);
                assert!(
                    peak > 0.05 && peak < 1.5,
                    "fs {fs} drive {drive} colour {colour} wear {wear}: peak {peak}"
                );
            }
        }
    }

    #[test]
    fn silence_in_silence_out_without_wear() {
        let out = render(24.0, 1.0, 0.0, 48000.0, 0.0, 0.0);
        assert!(peak(&out) < 1e-6);
    }

    #[test]
    fn hiss_trim_silences_worn_tape() {
        let quiet = Wear {
            hiss: 0.0,
            ..Wear::all(1.0)
        };
        assert!(peak(&render_worn(24.0, 1.0, quiet, 48000.0, 0.0, 0.0)) < 1e-6);
        assert!(peak(&render(24.0, 1.0, 1.0, 48000.0, 0.0, 0.0)) > 1e-4);
    }

    #[test]
    fn clean_settings_are_transparent() {
        // Emphasis and de-emphasis cancel, and quiet signals stay below saturation
        for freq in [100.0, 1000.0, 10000.0] {
            let gain = rms(&render(0.0, 0.0, 0.0, 48000.0, freq, 0.01)) / (0.01 / 2f32.sqrt());
            let db = util::gain_to_db(gain);
            assert!(db.abs() < 0.5, "{freq} Hz: {db:+.2} dB");
        }
    }

    #[test]
    fn drive_saturates() {
        // Crest factor (peak / RMS) is 1.41 for a sine and falls as it squashes towards a square
        let crest = |drive| {
            let out = render(drive, 0.0, 0.0, 48000.0, 220.0, 0.5);
            peak(&out) / rms(&out)
        };
        let (clean, driven) = (crest(0.0), crest(24.0));
        assert!(
            clean > 1.35 && driven < clean - 0.15,
            "clean {clean}, driven {driven}"
        );
    }

    #[test]
    fn colour_and_wear_darken() {
        let level = |colour, wear| rms(&render(0.0, colour, wear, 48000.0, 8000.0, 0.1));
        let (open, dark, worn) = (level(0.0, 0.0), level(1.0, 0.0), level(1.0, 1.0));
        assert!(
            dark < 0.5 * open && worn < 0.5 * dark,
            "open {open}, dark {dark}, worn {worn}"
        );
    }

    #[test]
    fn wear_wobbles_and_stays_in_the_buffer() {
        let fs = 48000.0;
        let base = (BASE_DELAY * fs).round();
        // Returns the (min, max) playback delay over 10 s of tape
        let swing = |wear: Wear| {
            let mut tr = Transport::default();
            (0..10 * fs as usize).fold((f32::MAX, f32::MIN), |(lo, hi), _| {
                let d = tr.next(&wear, base, fs).0;
                (lo.min(d), hi.max(d))
            })
        };
        assert_eq!(swing(Wear::all(0.0)), (base, base));
        let steady = Wear {
            wow: 0.0,
            flutter: 0.0,
            ..Wear::all(1.0)
        };
        assert_eq!(swing(steady), (base, base));
        let (lo, hi) = swing(Wear::all(1.0));
        assert!(hi - lo > 0.004 * fs, "swing {lo}..{hi}");

        // Trims past 100% wobble more at low wear, and maxed out stay inside the buffer
        let low = |wow| {
            let (lo, hi) = swing(Wear {
                wow,
                ..Wear::all(0.3)
            });
            hi - lo
        };
        assert!(
            low(2.0) > 3.0 * low(1.0),
            "200% {} vs 100% {}",
            low(2.0),
            low(1.0)
        );
        let (lo, hi) = swing(Wear {
            amount: 1.0,
            wow: 2.0,
            flutter: 2.0,
            dropouts: 2.0,
            hiss: 2.0,
        });
        // Never closer than 1 sample or past the buffer's 2 * base
        assert!(hi - lo > 0.016 * fs, "swing {lo}..{hi}");
        assert!(lo >= 1.0 && hi + 2.0 < 2.0 * base, "swing {lo}..{hi}");
    }

    #[test]
    fn delay_line_reads_whole_samples_exactly() {
        let mut d = DelayLine {
            buf: vec![0.0; 16],
            pos: 0,
        };
        for i in 0..20 {
            d.push(i as f32);
        }
        assert_eq!(d.read(3.0), 16.0);
        assert!((d.read(3.5) - 15.5).abs() < 1e-6);
    }
}
