use nice_plug::prelude::*;
use std::sync::Arc;

#[derive(Params)]
pub struct MagicRideParams {
    #[id = "targetLevel"]
    pub target_level: FloatParam,

    #[id = "range"]
    pub range: FloatParam,

    #[id = "speed"]
    pub speed: FloatParam,

    #[id = "cutRange"]
    pub cut_range: FloatParam,

    #[id = "boostRange"]
    pub boost_range: FloatParam,

    #[id = "attack"]
    pub attack: FloatParam,

    #[id = "release"]
    pub release: FloatParam,

    #[id = "hold"]
    pub hold: FloatParam,

    #[id = "breathReduction"]
    pub breath_reduction: FloatParam,

    #[id = "transientPreservation"]
    pub transient_preservation: FloatParam,

    #[id = "naturalMode"]
    pub natural_mode: BoolParam,

    #[id = "smartSilence"]
    pub smart_silence: BoolParam,

    #[id = "outputTrim"]
    pub output_trim: FloatParam,

    #[id = "noiseFloor"]
    pub noise_floor: FloatParam,

    #[id = "gainOutput"]
    pub gain_output: FloatParam,
}

impl Default for MagicRideParams {
    fn default() -> Self {
        Self {
            target_level: FloatParam::new(
                "Target Level",
                -24.0,
                FloatRange::Linear {
                    min: -40.0,
                    max: 0.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(Arc::new(|v| format!("{v:.1} dB"))),

            range: FloatParam::new(
                "Range (legacy)",
                46.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit(" dB")
            .with_value_to_string(Arc::new(|v| {
                format!("±{:.1} dB", (v * 0.24).clamp(0.0, 24.0))
            })),

            speed: FloatParam::new(
                "Speed",
                55.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit("%")
            .with_value_to_string(Arc::new(|v| format!("{v:.0}%"))),

            cut_range: FloatParam::new(
                "Cut Range",
                100.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 200.0,
                },
            )
            .with_unit(" dB"),

            boost_range: FloatParam::new(
                "Boost Range",
                400.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 500.0,
                },
            )
            .with_unit(" dB"),

            attack: FloatParam::new(
                "Attack",
                2.0,
                FloatRange::Linear {
                    min: 0.1,
                    max: 50.0,
                },
            )
            .with_unit(" ms"),

            release: FloatParam::new(
                "Release",
                10.0,
                FloatRange::Linear {
                    min: 0.1,
                    max: 50.0,
                },
            )
            .with_unit(" ms"),

            hold: FloatParam::new(
                "Hold",
                50.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 200.0,
                },
            )
            .with_unit(" ms"),

            breath_reduction: FloatParam::new(
                "Breath Reduction",
                1.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 1.0,
                },
            ),

            transient_preservation: FloatParam::new(
                "Transient Preservation",
                0.0,
                FloatRange::Linear {
                    min: 0.0,
                    max: 100.0,
                },
            )
            .with_unit("%"),

            natural_mode: BoolParam::new("Natural Mode", true),
            smart_silence: BoolParam::new("Smart Silence", false),

            output_trim: FloatParam::new(
                "Output Trim",
                0.0,
                FloatRange::Linear {
                    min: -12.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB"),

            noise_floor: FloatParam::new(
                "Noise Floor",
                -100.0,
                FloatRange::Linear {
                    min: -100.0,
                    max: 0.0,
                },
            )
            .with_unit(" dBFS"),

            gain_output: FloatParam::new(
                "Gain Output",
                -1.0,
                FloatRange::Linear {
                    min: -1.0,
                    max: 12.0,
                },
            )
            .with_unit(" dB"),
        }
    }
}

impl MagicRideParams {
    pub fn to_core(&self) -> magic_ride_core::Params {
        let mut p = magic_ride_core::Params::default();
        p.target_level = self.target_level.value();
        p.range = self.range.value();
        p.speed = self.speed.value();
        p.cut_range = self.cut_range.value();
        p.boost_range = self.boost_range.value();
        p.attack_ms = self.attack.value();
        p.release_ms = self.release.value();
        p.hold_ms = self.hold.value();
        p.breath_reduction = self.breath_reduction.value();
        p.transient_preservation = self.transient_preservation.value();
        p.natural_mode = self.natural_mode.value();
        p.smart_silence = self.smart_silence.value();
        p.output_trim = self.output_trim.value();
        p.noise_floor = self.noise_floor.value();
        p.gain_output = self.gain_output.value();
        p
    }

    pub fn set_from_plain(&self, setter: &ParamSetter, plain: &[f32; 15]) {
        let set_f = |param: &FloatParam, val: f32| {
            setter.begin_set_parameter(param);
            let norm = param.preview_normalized(val);
            setter.set_parameter_normalized(param, norm);
            setter.end_set_parameter(param);
        };
        let set_b = |param: &BoolParam, val: bool| {
            setter.begin_set_parameter(param);
            setter.set_parameter(param, val);
            setter.end_set_parameter(param);
        };

        set_f(&self.target_level, plain[0]);
        set_f(&self.range, plain[1]);
        set_f(&self.speed, plain[2]);
        set_f(&self.cut_range, plain[3]);
        set_f(&self.boost_range, plain[4]);
        set_f(&self.attack, plain[5]);
        set_f(&self.release, plain[6]);
        set_f(&self.hold, plain[7]);
        set_f(&self.breath_reduction, plain[8]);
        set_f(&self.transient_preservation, plain[9]);
        set_b(&self.natural_mode, plain[10] >= 0.5);
        set_b(&self.smart_silence, plain[11] >= 0.5);
        set_f(&self.output_trim, plain[12]);
        set_f(&self.noise_floor, plain[13]);
        set_f(&self.gain_output, plain[14]);
    }

    pub fn to_plain(&self) -> [f32; 15] {
        [
            self.target_level.value(),
            self.range.value(),
            self.speed.value(),
            self.cut_range.value(),
            self.boost_range.value(),
            self.attack.value(),
            self.release.value(),
            self.hold.value(),
            self.breath_reduction.value(),
            self.transient_preservation.value(),
            if self.natural_mode.value() { 1.0 } else { 0.0 },
            if self.smart_silence.value() { 1.0 } else { 0.0 },
            self.output_trim.value(),
            self.noise_floor.value(),
            self.gain_output.value(),
        ]
    }
}
