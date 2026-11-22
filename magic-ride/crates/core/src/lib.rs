//! magic.RIDE — reverse-engineered Rust core.
//!
//! Target: macOS VST3 / CLAP reconstruction of magic.RIDE 1.3.1 (MBM Audio).
//! See ../../ANALYSIS.md for the evidence behind every recovered fact.

pub mod dsp;
pub mod params;
pub mod preset_xml;
pub mod presets;

pub use dsp::{process_stereo, Rider};
pub use params::{Params, PARAM_IDS, PARAM_LABELS, PARAM_RANGES, PARAM_UNITS, STATE_KEYS};
pub use presets::{FactoryPreset, FACTORY_PRESETS};

/// Stable plugin identifiers used by both wrappers.
pub const VENDOR: &str = "MBM Audio (RE reconstruction)";
pub const PRODUCT: &str = "magic.RIDE";
pub const VERSION: &str = "1.3.1";
pub const CLAP_ID: &str = "com.mbmaudio.magicride.re";
