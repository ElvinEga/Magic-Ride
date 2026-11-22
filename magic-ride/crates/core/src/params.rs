//! Parameter model recovered from the magic.RIDE 1.3.1 binary.
//!
//! The original uses a JUCE AudioProcessorValueTreeState. The IDs below were
//! observed as ASCII strings in the VST3 binary (see ANALYSIS.md, evidence
//! E-PARAM-IDS). Plain-value ranges/defaults are reconstructed from the
//! factory presets and the UI label code; they are *inferences*, not observed
//! source values.

/// All automatable parameters, in the order used by the wrappers.
pub const PARAM_IDS: &[&str] = &[
    "targetLevel",
    "range",
    "speed",
    "cutRange",
    "boostRange",
    "attack",
    "release",
    "hold",
    "breathReduction",
    "transientPreservation",
    "naturalMode",
    "smartSilence",
    "outputTrim",
    "noiseFloor",
    "gainOutput",
];

/// Plain (non-normalized) ranges and defaults.
/// Tuple is (min, max, default).
pub const PARAM_RANGES: &[(f32, f32, f32)] = &[
    (-40.0, 0.0, -18.0),    // targetLevel  dBFS
    (0.0, 100.0, 50.0),     // range        dB
    (0.0, 100.0, 6.0),      // speed        %
    (0.0, 200.0, 100.0),    // cutRange     dB
    (0.0, 500.0, 400.0),    // boostRange   dB
    (0.1, 50.0, 2.0),       // attack       ms
    (0.1, 50.0, 10.0),      // release      ms
    (0.0, 200.0, 50.0),     // hold         ms
    (0.0, 1.0, 1.0),        // breathReduction (normalized bool-ish)
    (0.0, 100.0, 0.0),      // transientPreservation %
    (0.0, 1.0, 1.0),        // naturalMode  bool
    (0.0, 1.0, 0.0),        // smartSilence bool
    (-12.0, 12.0, 0.0),     // outputTrim   dB
    (-100.0, 0.0, -100.0),  // noiseFloor   dBFS
    (-1.0, 12.0, -1.0),     // gainOutput   dB (-1 = automatic)
];

pub const PARAM_LABELS: &[&str] = &[
    "Target Level",
    "Range (legacy)",
    "Speed",
    "Cut Range",
    "Boost Range",
    "Attack",
    "Release",
    "Hold",
    "Breath Reduction",
    "Transient Preservation",
    "Natural Mode",
    "Smart Silence",
    "Output Trim",
    "Noise Floor",
    "Gain Output",
];

pub const PARAM_UNITS: &[&str] = &[
    "dB", "dB", "%", "dB", "dB", "ms", "ms", "ms", "", "%", "", "", "dB", "dB", "dB",
];

/// Non-automatable state keys also observed in the binary.
pub const STATE_KEYS: &[&str] = &[
    "attackMs", "releaseMs", "holdMs", "useLufs", "automationMode", "lookAheadMode",
    "rangeLocked", "vocalFocusEnabled", "sidechainEnabled", "sidechainFollow",
    "sidechainAmount", "presetIndex", "scrollSpeed", "uiScalePercent", "windowSizeIndex",
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub target_level: f32,
    pub range: f32,
    pub speed: f32,
    pub cut_range: f32,
    pub boost_range: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub hold_ms: f32,
    pub breath_reduction: f32,
    pub transient_preservation: f32,
    pub natural_mode: bool,
    pub smart_silence: bool,
    pub output_trim: f32,
    pub noise_floor: f32,
    pub gain_output: f32,
    // Non-automatable state recovered from the preset structs.
    pub use_lufs: bool,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            target_level: -18.0,
            range: 50.0,
            speed: 6.0,
            cut_range: 100.0,
            boost_range: 400.0,
            attack_ms: 2.0,
            release_ms: -1.0,
            hold_ms: 50.0,
            breath_reduction: 1.0,
            transient_preservation: 0.0,
            natural_mode: true,
            smart_silence: false,
            output_trim: 0.0,
            noise_floor: -100.0,
            gain_output: -1.0,
            use_lufs: false,
        }
    }
}

impl Params {
    /// Map plain values (ordered as `PARAM_IDS`) onto the struct.
    pub fn from_plain(values: &[f32; 15]) -> Self {
        let mut p = Self::default();
        p.target_level = values[0];
        p.range = values[1];
        p.speed = values[2];
        p.cut_range = values[3];
        p.boost_range = values[4];
        p.attack_ms = values[5];
        p.release_ms = if values[6] < 0.0 { 10.0 } else { values[6] };
        p.hold_ms = values[7];
        p.breath_reduction = values[8];
        p.transient_preservation = values[9];
        p.natural_mode = values[10] >= 0.5;
        p.smart_silence = values[11] >= 0.5;
        p.output_trim = values[12];
        p.noise_floor = values[13];
        p.gain_output = values[14];
        p
    }

    pub fn plain(&self) -> [f32; 15] {
        [
            self.target_level,
            self.range,
            self.speed,
            self.cut_range,
            self.boost_range,
            self.attack_ms,
            self.release_ms,
            self.hold_ms,
            self.breath_reduction,
            self.transient_preservation,
            if self.natural_mode { 1.0 } else { 0.0 },
            if self.smart_silence { 1.0 } else { 0.0 },
            self.output_trim,
            self.noise_floor,
            self.gain_output,
        ]
    }

    pub fn normalize(index: usize, value: f32) -> f32 {
        let (min, max, _) = PARAM_RANGES[index];
        ((value.clamp(min, max) - min) / (max - min)).clamp(0.0, 1.0)
    }

    pub fn denormalize(index: usize, norm: f32) -> f32 {
        let (min, max, _) = PARAM_RANGES[index];
        min + norm.clamp(0.0, 1.0) * (max - min)
    }
}
