//! Gain-riding DSP reconstruction.
//!
//! The original algorithm is not available as source. This implements the
//! behaviour described by the recovered UI strings and parameter semantics:
//! level tracking -> target error -> asymmetric boost/cut limiting -> hold /
//! attack / release ballistics -> output trim. It is a functional
//! reconstruction, not a bit-exact clone.

use crate::params::Params;

const MIN_DB: f32 = -90.0;

#[inline]
fn db_to_lin(db: f32) -> f32 {
    if db <= MIN_DB {
        0.0
    } else {
        10.0f32.powf(db / 20.0)
    }
}

#[inline]
fn lin_to_db(x: f32) -> f32 {
    if x <= 1e-12 {
        MIN_DB
    } else {
        20.0 * x.log10()
    }
}

/// One-pole coefficient for a time constant expressed in milliseconds.
#[inline]
fn coeff_ms(ms: f32, sample_rate: f32) -> f32 {
    if ms <= 0.0 {
        return 1.0;
    }
    let t = (ms / 1000.0 * sample_rate).max(1.0);
    (-1.0 / t).exp()
}

#[derive(Clone, Copy)]
pub struct Rider {
    level: f32,
    gain: f32,
    hold_counter: usize,
    pending_target: f32,
    peak_hold: f32,
    sample_rate: f32,
}

impl Default for Rider {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl Rider {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            level: 0.0,
            gain: 1.0,
            hold_counter: 0,
            pending_target: 1.0,
            peak_hold: 0.0,
            sample_rate,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f32) {
        self.sample_rate = sr;
    }

    pub fn reset(&mut self) {
        self.level = 0.0;
        self.gain = 1.0;
        self.hold_counter = 0;
        self.pending_target = 1.0;
        self.peak_hold = 0.0;
    }

    /// Process one stereo frame in place, returning current gain reduction dB.
    pub fn process_frame(&mut self, left: &mut f32, right: &mut f32, p: &Params) -> f32 {
        let sr = self.sample_rate.max(1.0);

        // 1. Level estimate (RMS of the two channels, with peak influence when
        //    transient preservation is enabled).
        let l = *left;
        let r = *right;
        let square = (l * l + r * r) * 0.5;
        let rms = square.sqrt();
        let peak = l.abs().max(r.abs());
        let detected = if p.transient_preservation > 0.0 {
            peak * (1.0 - p.transient_preservation * 0.01) + rms * (p.transient_preservation * 0.01)
        } else {
            rms
        };
        let det_db = lin_to_db(detected);

        // 2. Noise floor / breath gating.
        let below_floor = det_db < p.noise_floor;
        let breath_attn = if p.breath_reduction > 0.5 && det_db < p.noise_floor + 6.0 {
            -6.0 * p.breath_reduction
        } else {
            0.0
        };
        let effective_db = det_db + breath_attn;

        // 3. Desired gain: pull level towards target, limited by boost/cut.
        let mut error_db = p.target_level - effective_db;
        if p.range > 0.0 {
            error_db = error_db.clamp(-p.range, p.range);
        }
        // Asymmetric ceiling from the recovered presets. `cutRange`/`boostRange`
        // are expressed as percentages of `range` in this reconstruction.
        let cut_limit = -p.range * (p.cut_range / 100.0).max(0.0);
        let boost_limit = p.range * (p.boost_range / 100.0).max(0.0);
        error_db = error_db.clamp(cut_limit, boost_limit);

        if below_floor && p.smart_silence {
            error_db = error_db.min(0.0);
        }

        let target_gain = db_to_lin(error_db).max(1e-6);

        // 4. Hold / attack / release ballistics. `speed` scales all times.
        let speed_scale = (100.0 / (p.speed.max(1.0) + 100.0)).clamp(0.1, 1.0);
        let hold_samples = (p.hold_ms * speed_scale * sr / 1000.0) as usize;
        let a_ms = if p.attack_ms >= 0.0 {
            p.attack_ms
        } else {
            2.0
        };
        let r_ms = if p.release_ms >= 0.0 {
            p.release_ms
        } else {
            10.0
        };
        let a_coeff = coeff_ms((a_ms / speed_scale).max(0.1), sr);
        let r_coeff = coeff_ms((r_ms / speed_scale).max(0.1), sr);
        let level_coeff = coeff_ms(5.0 / speed_scale, sr);
        self.level = self.level * level_coeff + detected * (1.0 - level_coeff);

        if target_gain > self.pending_target && self.hold_counter == 0 {
            self.hold_counter = hold_samples;
            self.pending_target = target_gain;
        }
        if self.hold_counter > 0 {
            self.hold_counter -= 1;
        } else {
            self.pending_target = target_gain;
        }

        if self.pending_target > self.gain {
            self.gain = a_coeff * self.gain + (1.0 - a_coeff) * self.pending_target;
        } else {
            self.gain = r_coeff * self.gain + (1.0 - r_coeff) * self.pending_target;
        }

        // 5. Natural mode softens very fast gain changes.
        if p.natural_mode {
            let smooth = coeff_ms(3.0, sr);
            self.gain = smooth * self.gain + (1.0 - smooth) * (target_gain * 0.7 + self.gain * 0.3);
        }

        // 6. Output trim + optional explicit gain output.
        let trim = db_to_lin(p.output_trim);
        let makeup = if p.gain_output >= 0.0 {
            db_to_lin(p.gain_output)
        } else {
            1.0
        };
        let g = self.gain * trim * makeup;
        *left *= g;
        *right *= g;

        self.peak_hold = self.peak_hold.max(lin_to_db((*left).abs().max((*right).abs())));
        lin_to_db(self.gain)
    }

    pub fn input_level_db(&self) -> f32 {
        lin_to_db(self.level)
    }

    pub fn output_level_db(&self) -> f32 {
        self.peak_hold
    }
}

pub fn process_stereo(
    left: &mut [f32],
    right: &mut [f32],
    rider: &mut Rider,
    p: &Params,
) {
    for (l, r) in left.iter_mut().zip(right.iter_mut()) {
        rider.process_frame(l, r, p);
    }
}
