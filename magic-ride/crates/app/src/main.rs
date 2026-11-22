//! Standalone macOS UI & live audio host for the reverse-engineered magic.RIDE DSP.
//!
//! Matches the MBM Audio magic.RIDE dark interface using pure egui vector rendering:
//! scrolling input/output/gain-rider graph, custom rotary knobs, gradient vertical meters,
//! A/B compare, undo/redo, all 48 factory presets, and an advanced drawer for all 15 parameters.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use egui::{
    Align2, Color32, CornerRadius, CursorIcon, FontId, Margin, Mesh, Pos2, Rect, Sense, Shape,
    Stroke, StrokeKind, Vec2,
};
use magic_ride_core::*;
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::HeapRb;
use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Color Palette (matched to magic.RIDE screenshot)
// ---------------------------------------------------------------------------
const BG_HEADER: Color32 = Color32::from_rgb(20, 22, 27);
const BG_BRAND: Color32 = Color32::from_rgb(15, 17, 22);
const BG_GRAPH_TOP: Color32 = Color32::from_rgb(27, 30, 37);
const BG_GRAPH_BOT: Color32 = Color32::from_rgb(31, 35, 42);
const BG_METERS: Color32 = Color32::from_rgb(18, 20, 25);
const BG_FOOTER: Color32 = Color32::from_rgb(18, 20, 25);
const BG_CONTROL: Color32 = Color32::from_rgb(29, 33, 41);
const BG_CONTROL_HOVER: Color32 = Color32::from_rgb(39, 44, 54);
const BORDER_SUBTLE: Color32 = Color32::from_rgb(39, 43, 52);
const BORDER_CTRL: Color32 = Color32::from_rgb(52, 57, 69);

const TEXT_PRIMARY: Color32 = Color32::from_rgb(232, 236, 242);
const TEXT_MUTED: Color32 = Color32::from_rgb(132, 140, 155);
const TEXT_DIM: Color32 = Color32::from_rgb(96, 103, 117);

const PURPLE_BRIGHT: Color32 = Color32::from_rgb(194, 152, 255);
const PURPLE_MID: Color32 = Color32::from_rgb(154, 106, 228);
const PURPLE_DEEP: Color32 = Color32::from_rgb(118, 76, 194);
const PURPLE_TARGET: Color32 = Color32::from_rgb(176, 110, 224);

const CYAN_BRIGHT: Color32 = Color32::from_rgb(66, 230, 255);
const CYAN_MID: Color32 = Color32::from_rgb(30, 182, 214);

const GRAPH_MIN_DB: f32 = -60.0;
const GRAPH_MAX_DB: f32 = 4.0;
const HISTORY_LEN: usize = 340;

#[derive(Clone, Copy)]
struct TracePoint {
    input_db: f32,
    output_db: f32,
    gain_db: f32,
}

impl Default for TracePoint {
    fn default() -> Self {
        Self {
            input_db: -60.0,
            output_db: -60.0,
            gain_db: 0.0,
        }
    }
}

#[derive(Default, Clone, Copy)]
struct Meters {
    input_db: f32,
    output_db: f32,
    gain_db: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum AutoMode {
    Off,
    Read,
    Write,
    Latch,
}

impl AutoMode {
    fn label(self) -> &'static str {
        match self {
            AutoMode::Off => "OFF",
            AutoMode::Read => "READ",
            AutoMode::Write => "WRITE",
            AutoMode::Latch => "LATCH",
        }
    }
    const ALL: [AutoMode; 4] = [AutoMode::Off, AutoMode::Read, AutoMode::Write, AutoMode::Latch];
}

struct AudioRuntime {
    _input: cpal::Stream,
    _output: cpal::Stream,
    sample_rate: u32,
    input_name: String,
    output_name: String,
}

struct App {
    plain: [f32; 15],
    use_lufs: bool,
    params: Arc<Mutex<Params>>,
    meters: Arc<Mutex<Meters>>,
    live_trace_rx: Arc<Mutex<VecDeque<TracePoint>>>,
    audio: Option<AudioRuntime>,
    error: Option<String>,

    // History & visualization
    history: VecDeque<TracePoint>,
    display_out_db: f32,
    display_peak_db: f32,
    display_gain_db: f32,
    peak_hold_timer: f32,

    // Demo simulator (drives real Rider DSP when live audio is off)
    demo_mode: bool,
    demo_rider: Rider,
    demo_time: f32,
    last_frame: Instant,

    // UI state
    preset: Option<usize>,
    preset_menu_open: bool,
    auto_mode: AutoMode,
    auto_menu_open: bool,
    auto_target_enabled: bool,
    settings_open: bool,
    help_open: bool,
    expanded_window: bool,

    // A/B & Undo/Redo
    slot_is_b: bool,
    slot_a: [f32; 15],
    slot_b: [f32; 15],
    undo_stack: Vec<[f32; 15]>,
    redo_stack: Vec<[f32; 15]>,
    drag_snapshot: Option<[f32; 15]>,
}

impl Default for App {
    fn default() -> Self {
        let mut plain = Params::default().plain();
        // Start close to the iconic screenshot defaults (-24 dB target, ~46 range, ~58 speed)
        plain[0] = -24.0; // targetLevel
        plain[1] = 46.0; // range
        plain[2] = 55.0; // speed
        let mut p = Params::from_plain(&plain);
        p.natural_mode = true;
        plain[10] = 1.0;

        let mut app = Self {
            params: Arc::new(Mutex::new(p)),
            meters: Arc::new(Mutex::new(Meters::default())),
            live_trace_rx: Arc::new(Mutex::new(VecDeque::with_capacity(128))),
            plain,
            use_lufs: false,
            audio: None,
            error: None,
            history: VecDeque::with_capacity(HISTORY_LEN + 8),
            display_out_db: -24.0,
            display_peak_db: -6.0,
            display_gain_db: 0.0,
            peak_hold_timer: 0.0,
            demo_mode: true,
            demo_rider: Rider::new(48000.0),
            demo_time: 0.0,
            last_frame: Instant::now(),
            preset: Some(0),
            preset_menu_open: false,
            auto_mode: AutoMode::Off,
            auto_menu_open: false,
            auto_target_enabled: false,
            settings_open: false,
            help_open: false,
            expanded_window: false,
            slot_is_b: false,
            slot_a: plain,
            slot_b: FACTORY_PRESETS[1].params.plain(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            drag_snapshot: None,
        };

        // Pre-fill history so the waveform graph looks alive immediately on open
        for _ in 0..HISTORY_LEN {
            app.step_demo(1.0 / 60.0);
        }
        app
    }
}

impl App {
    fn current_params(&self) -> Params {
        let mut p = Params::from_plain(&self.plain);
        p.use_lufs = self.use_lufs;
        p
    }

    fn push_params(&mut self) {
        let p = self.current_params();
        if let Ok(mut shared) = self.params.lock() {
            *shared = p;
        }
        if self.slot_is_b {
            self.slot_b = self.plain;
        } else {
            self.slot_a = self.plain;
        }
    }

    fn record_undo(&mut self, prev: [f32; 15]) {
        if prev != self.plain {
            self.undo_stack.push(prev);
            if self.undo_stack.len() > 64 {
                self.undo_stack.remove(0);
            }
            self.redo_stack.clear();
        }
    }

    fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.redo_stack.push(self.plain);
            self.plain = prev;
            self.push_params();
        }
    }

    fn redo(&mut self) {
        if let Some(next) = self.redo_stack.pop() {
            self.undo_stack.push(self.plain);
            self.plain = next;
            self.push_params();
        }
    }

    fn apply_preset(&mut self, index: usize) {
        let prev = self.plain;
        let fp = &FACTORY_PRESETS[index];
        self.preset = Some(index);
        self.plain = fp.params.plain();
        self.use_lufs = fp.params.use_lufs;
        self.record_undo(prev);
        self.push_params();
    }

    fn step_demo(&mut self, dt: f32) {
        let dt = dt.clamp(0.002, 0.05);
        self.demo_time += dt;
        let t = self.demo_time;

        // Expressive synthetic vocal phrase envelope (in dBFS) with syllables, breaths & pauses
        let phrase = (t * 0.72).sin() * 0.5 + 0.5;
        let syllable = ((t * 4.3).sin() * 0.55 + (t * 7.1 + 0.8).sin() * 0.45).abs();
        let micro = (t * 17.3).sin() * 2.4 + (t * 29.1).cos() * 1.4;
        let dip = if ((t * 0.41).sin()) < -0.58 { -22.0 } else { 0.0 };
        let swell = (t * 0.27 + 1.1).sin() * 9.5;
        let input_db = (-22.0 + swell + phrase * 12.0 + syllable * 8.5 + micro + dip)
            .clamp(-58.0, -1.5);

        let amp = 10.0f32.powf(input_db / 20.0);
        let p = self.current_params();

        // Feed a short audio block (~128 frames) through the actual reconstructed Rider DSP
        let mut gain_db = 0.0f32;
        let mut out_sq = 0.0f32;
        let block = 128;
        for s in 0..block {
            let phase = (t * 220.0 + s as f32 * 0.028) * TAU;
            let mut l = amp * phase.sin();
            let mut r = l;
            gain_db = self.demo_rider.process_frame(&mut l, &mut r, &p);
            out_sq += l * l;
        }
        let out_rms = (out_sq / block as f32).sqrt().max(1e-6);
        let output_db = (20.0 * out_rms.log10() + 3.0).clamp(-60.0, 0.0);

        // Visually smooth gain curve slightly so it has the clean vocal-rider ribbon feel
        let prev_gain = self.history.back().map(|x| x.gain_db).unwrap_or(0.0);
        let smooth_gain = prev_gain * 0.65 + gain_db * 0.35;

        self.push_trace(TracePoint {
            input_db,
            output_db,
            gain_db: smooth_gain,
        });

        self.update_meter_ballistics(output_db, smooth_gain, dt);

        if self.auto_target_enabled && input_db > -45.0 {
            let desired = input_db.clamp(-36.0, -6.0);
            self.plain[0] += (desired - self.plain[0]) * (dt * 0.9).min(1.0);
            self.push_params();
        }
    }

    fn push_trace(&mut self, pt: TracePoint) {
        if self.history.len() >= HISTORY_LEN {
            self.history.pop_front();
        }
        self.history.push_back(pt);
    }

    fn update_meter_ballistics(&mut self, out_db: f32, gain_db: f32, dt: f32) {
        let smooth = (dt * 14.0).clamp(0.05, 0.5);
        self.display_out_db += (out_db - self.display_out_db) * smooth;
        self.display_gain_db += (gain_db - self.display_gain_db) * smooth;

        if out_db >= self.display_peak_db {
            self.display_peak_db = out_db;
            self.peak_hold_timer = 1.4;
        } else if self.peak_hold_timer > 0.0 {
            self.peak_hold_timer -= dt;
        } else {
            self.display_peak_db = (self.display_peak_db - dt * 14.0).max(self.display_out_db);
        }
    }

    fn start_audio(&mut self) -> Result<AudioRuntime, String> {
        let host = cpal::default_host();
        let in_dev = host
            .default_input_device()
            .ok_or("No default input device found")?;
        let out_dev = host
            .default_output_device()
            .ok_or("No default output device found")?;
        let in_cfg = in_dev.default_input_config().map_err(|e| e.to_string())?;
        let out_cfg = out_dev.default_output_config().map_err(|e| e.to_string())?;
        let in_channels = in_cfg.channels() as usize;
        let out_channels = out_cfg.channels() as usize;
        let sr_in = in_cfg.sample_rate();
        let sr_out = out_cfg.sample_rate();
        if sr_in != sr_out {
            return Err(format!(
                "Input ({sr_in} Hz) and output ({sr_out} Hz) sample rates differ; match them in Audio MIDI Setup"
            ));
        }

        let capacity = (sr_in as usize) * 4;
        let ring = HeapRb::<f32>::new(capacity);
        let (mut producer, mut consumer) = ring.split();
        let params_in = Arc::clone(&self.params);
        let meters_in = Arc::clone(&self.meters);
        let meters_out = Arc::clone(&self.meters);
        let trace_tx = Arc::clone(&self.live_trace_rx);

        let input_stream = in_dev
            .build_input_stream(
                in_cfg.into(),
                move |data: &[f32], _| {
                    let frames = data.len() / in_channels.max(1);
                    let mut sq = 0.0f32;
                    for i in 0..frames {
                        let base = i * in_channels;
                        let l = data[base];
                        let r = if in_channels > 1 { data[base + 1] } else { l };
                        sq += 0.5 * (l * l + r * r);
                        let _ = producer.try_push(l);
                        let _ = producer.try_push(r);
                    }
                    if frames > 0 {
                        let rms = (sq / frames as f32).sqrt().max(1e-6);
                        if let Ok(mut m) = meters_in.try_lock() {
                            m.input_db = (20.0 * rms.log10()).clamp(-60.0, 0.0);
                        }
                    }
                },
                |err| eprintln!("input stream error: {err}"),
                None,
            )
            .map_err(|e| e.to_string())?;

        let mut rider = Rider::new(sr_out as f32);
        let mut last = self.current_params();
        let mut trace_accum = 0usize;
        let trace_stride = (sr_out as usize / 60).max(128);
        let mut in_sq_acc = 0.0f32;
        let mut out_sq_acc = 0.0f32;

        let output_stream = out_dev
            .build_output_stream(
                out_cfg.into(),
                move |data: &mut [f32], _| {
                    let p = match params_in.try_lock() {
                        Ok(guard) => {
                            last = *guard;
                            last
                        }
                        Err(_) => last,
                    };
                    let frames = data.len() / out_channels.max(1);
                    let mut gr = 0.0f32;
                    let mut out_peak = 0.0f32;

                    for i in 0..frames {
                        let mut l = consumer.try_pop().unwrap_or(0.0);
                        let mut r = consumer.try_pop().unwrap_or(0.0);
                        in_sq_acc += 0.5 * (l * l + r * r);
                        gr = rider.process_frame(&mut l, &mut r, &p);
                        out_sq_acc += 0.5 * (l * l + r * r);
                        out_peak = out_peak.max(l.abs()).max(r.abs());

                        let base = i * out_channels;
                        for ch in 0..out_channels {
                            data[base + ch] = if ch % 2 == 0 { l } else { r };
                        }

                        trace_accum += 1;
                        if trace_accum >= trace_stride {
                            let inv = 1.0 / trace_accum as f32;
                            let in_db = (20.0 * (in_sq_acc * inv).sqrt().max(1e-6).log10())
                                .clamp(-60.0, 0.0);
                            let out_db = (20.0 * (out_sq_acc * inv).sqrt().max(1e-6).log10())
                                .clamp(-60.0, 0.0);
                            if let Ok(mut q) = trace_tx.try_lock() {
                                if q.len() >= 128 {
                                    q.pop_front();
                                }
                                q.push_back(TracePoint {
                                    input_db: in_db,
                                    output_db: out_db,
                                    gain_db: gr,
                                });
                            }
                            trace_accum = 0;
                            in_sq_acc = 0.0;
                            out_sq_acc = 0.0;
                        }
                    }

                    if let Ok(mut m) = meters_out.try_lock() {
                        m.gain_db = gr;
                        m.output_db = if out_peak > 1e-6 {
                            (20.0 * out_peak.log10()).clamp(-60.0, 0.0)
                        } else {
                            -60.0
                        };
                    }
                },
                |err| eprintln!("output stream error: {err}"),
                None,
            )
            .map_err(|e| e.to_string())?;

        input_stream.play().map_err(|e| e.to_string())?;
        output_stream.play().map_err(|e| e.to_string())?;

        Ok(AudioRuntime {
            _input: input_stream,
            _output: output_stream,
            sample_rate: sr_in,
            input_name: in_dev.to_string(),
            output_name: out_dev.to_string(),
        })
    }

    fn toggle_live_audio(&mut self) {
        if self.audio.is_some() {
            self.audio = None;
            self.demo_mode = true;
            self.error = None;
        } else {
            match self.start_audio() {
                Ok(rt) => {
                    self.audio = Some(rt);
                    self.demo_mode = false;
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(e);
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // UI Sections
    // -----------------------------------------------------------------------

    fn draw_header(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG_HEADER);

        // Brand block on the left (slightly darker recessed bay)
        let brand_w = 398.0f32.min(rect.width() * 0.44);
        let brand_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + brand_w, rect.bottom()));
        painter.rect_filled(brand_rect, CornerRadius::ZERO, BG_BRAND);
        painter.line_segment(
            [
                Pos2::new(brand_rect.right(), brand_rect.top()),
                Pos2::new(brand_rect.right(), brand_rect.bottom()),
            ],
            Stroke::new(1.0, BORDER_SUBTLE),
        );
        // Bottom hairline across header
        painter.line_segment(
            [rect.left_bottom(), rect.right_bottom()],
            Stroke::new(1.0, BORDER_SUBTLE),
        );

        // MBM Audio logo icon
        let cy = rect.center().y;
        let icon_rect = Rect::from_center_size(Pos2::new(rect.left() + 42.0, cy), Vec2::new(34.0, 28.0));
        painter.rect(
            icon_rect,
            CornerRadius::same(4),
            Color32::from_rgb(138, 144, 155),
            Stroke::NONE,
            StrokeKind::Inside,
        );
        // Inner waveform cutout
        let wave_pts = [
            Pos2::new(icon_rect.left() + 4.0, cy + 2.0),
            Pos2::new(icon_rect.left() + 8.5, cy + 2.0),
            Pos2::new(icon_rect.left() + 12.0, cy - 8.5),
            Pos2::new(icon_rect.left() + 16.5, cy + 8.5),
            Pos2::new(icon_rect.left() + 21.5, cy - 6.0),
            Pos2::new(icon_rect.left() + 25.0, cy + 4.0),
            Pos2::new(icon_rect.right() - 4.0, cy + 1.0),
        ];
        painter.add(Shape::line(
            wave_pts.to_vec(),
            Stroke::new(2.3, BG_BRAND),
        ));

        // MBM / AUDIO stacked wordmark
        painter.text(
            Pos2::new(icon_rect.right() + 8.0, cy - 6.5),
            Align2::LEFT_CENTER,
            "MBM",
            FontId::proportional(17.5),
            Color32::from_rgb(154, 160, 171),
        );
        painter.text(
            Pos2::new(icon_rect.right() + 8.0, cy + 8.0),
            Align2::LEFT_CENTER,
            "AUDIO",
            FontId::proportional(12.0),
            Color32::from_rgb(138, 144, 155),
        );

        // Vertical divider
        let div_x = icon_rect.right() + 72.0;
        painter.line_segment(
            [Pos2::new(div_x, rect.top() + 14.0), Pos2::new(div_x, rect.bottom() - 14.0)],
            Stroke::new(1.0, Color32::from_rgb(56, 60, 70)),
        );

        // magic.RIDE title + subtitle
        let title_x = div_x + 18.0;
        let galley_magic = painter.layout_no_wrap(
            "magic.".to_string(),
            FontId::proportional(24.0),
            TEXT_PRIMARY,
        );
        let magic_w = galley_magic.size().x;
        painter.galley(Pos2::new(title_x, cy - 16.0), galley_magic, TEXT_PRIMARY);

        let galley_ride = painter.layout_no_wrap(
            "RIDE".to_string(),
            FontId::proportional(24.0),
            PURPLE_MID,
        );
        let ride_w = galley_ride.size().x;
        painter.galley(
            Pos2::new(title_x + magic_w + 1.0, cy - 16.0),
            galley_ride,
            PURPLE_MID,
        );

        painter.text(
            Pos2::new(title_x + magic_w + ride_w, cy + 15.0),
            Align2::RIGHT_CENTER,
            "Precision Vocal Leveling",
            FontId::proportional(9.5),
            TEXT_DIM,
        );

        // --- Right-side header controls ---
        let right = rect.right();

        // Help (?) button
        let help_rect = Rect::from_center_size(Pos2::new(right - 26.0, cy), Vec2::new(26.0, 26.0));
        let help_resp = ui.interact(help_rect, ui.id().with("hdr_help"), Sense::click());
        let help_col = if self.help_open || help_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        painter.text(
            help_rect.center(),
            Align2::CENTER_CENTER,
            "?",
            FontId::proportional(14.0),
            help_col,
        );
        if help_resp.clicked() {
            self.help_open = !self.help_open;
        }

        // Settings gear icon button
        let gear_rect = Rect::from_center_size(Pos2::new(right - 64.0, cy), Vec2::new(26.0, 26.0));
        let gear_resp = ui.interact(gear_rect, ui.id().with("hdr_gear"), Sense::click());
        let gear_col = if self.settings_open {
            PURPLE_BRIGHT
        } else if gear_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        draw_gear_icon(&painter, gear_rect.center(), 7.5, gear_col);
        if gear_resp.clicked() {
            self.settings_open = !self.settings_open;
        }

        // Preset selector:  <  [ Preset          v ]  >
        let next_rect = Rect::from_center_size(Pos2::new(right - 112.0, cy), Vec2::new(20.0, 24.0));
        let box_w = 162.0;
        let preset_box = Rect::from_center_size(
            Pos2::new(next_rect.left() - 10.0 - box_w * 0.5, cy),
            Vec2::new(box_w, 26.0),
        );
        let prev_rect = Rect::from_center_size(
            Pos2::new(preset_box.left() - 18.0, cy),
            Vec2::new(20.0, 24.0),
        );

        let prev_resp = ui.interact(prev_rect, ui.id().with("preset_prev"), Sense::click());
        draw_triangle(
            &painter,
            prev_rect.center(),
            4.5,
            TriDir::Left,
            if prev_resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED },
        );
        if prev_resp.clicked() {
            let idx = self
                .preset
                .map(|i| if i == 0 { FACTORY_PRESETS.len() - 1 } else { i - 1 })
                .unwrap_or(0);
            self.apply_preset(idx);
        }

        let next_resp = ui.interact(next_rect, ui.id().with("preset_next"), Sense::click());
        draw_triangle(
            &painter,
            next_rect.center(),
            4.5,
            TriDir::Right,
            if next_resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED },
        );
        if next_resp.clicked() {
            let idx = self
                .preset
                .map(|i| (i + 1) % FACTORY_PRESETS.len())
                .unwrap_or(0);
            self.apply_preset(idx);
        }

        let box_resp = ui.interact(preset_box, ui.id().with("preset_box"), Sense::click());
        painter.rect(
            preset_box,
            CornerRadius::same(4),
            if box_resp.hovered() || self.preset_menu_open {
                BG_CONTROL_HOVER
            } else {
                BG_CONTROL
            },
            Stroke::new(1.0, BORDER_CTRL),
            StrokeKind::Inside,
        );
        let preset_label = self
            .preset
            .map(|i| FACTORY_PRESETS[i].name)
            .unwrap_or("Preset");
        painter.text(
            Pos2::new(preset_box.left() + 10.0, cy),
            Align2::LEFT_CENTER,
            truncate_str(preset_label, 18),
            FontId::proportional(12.0),
            if self.preset.is_some() {
                Color32::from_rgb(196, 202, 212)
            } else {
                TEXT_MUTED
            },
        );
        draw_triangle(
            &painter,
            Pos2::new(preset_box.right() - 12.0, cy + 0.5),
            4.0,
            TriDir::Down,
            TEXT_MUTED,
        );
        if box_resp.clicked() {
            self.preset_menu_open = !self.preset_menu_open;
        }

        // A | B toggle pill
        let ab_rect = Rect::from_center_size(
            Pos2::new(prev_rect.left() - 48.0, cy),
            Vec2::new(48.0, 24.0),
        );
        painter.rect(
            ab_rect,
            CornerRadius::same(4),
            BG_CONTROL,
            Stroke::new(1.0, BORDER_CTRL),
            StrokeKind::Inside,
        );
        let a_rect = Rect::from_min_max(ab_rect.min, Pos2::new(ab_rect.center().x, ab_rect.bottom()));
        let b_rect = Rect::from_min_max(Pos2::new(ab_rect.center().x, ab_rect.top()), ab_rect.max);

        let a_resp = ui.interact(a_rect, ui.id().with("slot_a"), Sense::click());
        let b_resp = ui.interact(b_rect, ui.id().with("slot_b"), Sense::click());

        if !self.slot_is_b {
            painter.rect_filled(a_rect.shrink(1.5), CornerRadius::same(3), Color32::from_rgb(44, 48, 58));
        } else {
            painter.rect_filled(b_rect.shrink(1.5), CornerRadius::same(3), Color32::from_rgb(44, 48, 58));
        }
        painter.text(
            a_rect.center(),
            Align2::CENTER_CENTER,
            "A",
            FontId::proportional(11.5),
            if !self.slot_is_b { TEXT_PRIMARY } else { TEXT_DIM },
        );
        painter.text(
            b_rect.center(),
            Align2::CENTER_CENTER,
            "B",
            FontId::proportional(11.5),
            if self.slot_is_b { TEXT_PRIMARY } else { TEXT_DIM },
        );

        if a_resp.clicked() && self.slot_is_b {
            self.slot_b = self.plain;
            self.slot_is_b = false;
            self.plain = self.slot_a;
            self.push_params();
        }
        if b_resp.clicked() && !self.slot_is_b {
            self.slot_a = self.plain;
            self.slot_is_b = true;
            self.plain = self.slot_b;
            self.push_params();
        }

        // Undo / Redo curved arrows
        let redo_rect = Rect::from_center_size(
            Pos2::new(ab_rect.left() - 26.0, cy),
            Vec2::new(22.0, 22.0),
        );
        let undo_rect = Rect::from_center_size(
            Pos2::new(redo_rect.left() - 18.0, cy),
            Vec2::new(22.0, 22.0),
        );

        let undo_resp = ui.interact(undo_rect, ui.id().with("btn_undo"), Sense::click());
        let redo_resp = ui.interact(redo_rect, ui.id().with("btn_redo"), Sense::click());

        let undo_col = if self.undo_stack.is_empty() {
            Color32::from_rgb(72, 77, 88)
        } else if undo_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        let redo_col = if self.redo_stack.is_empty() {
            Color32::from_rgb(72, 77, 88)
        } else if redo_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };

        draw_undo_icon(&painter, undo_rect.center(), false, undo_col);
        draw_undo_icon(&painter, redo_rect.center(), true, redo_col);

        if undo_resp.clicked() {
            self.undo();
        }
        if redo_resp.clicked() {
            self.redo();
        }

        // Subtle LIVE / DEMO audio toggle badge between brand bay and undo/redo
        let space_left = undo_rect.left() - brand_rect.right();
        if space_left > 86.0 {
            let live_rect = Rect::from_center_size(
                Pos2::new(brand_rect.right() + 48.0, cy),
                Vec2::new(66.0, 22.0),
            );
            let live_resp = ui.interact(live_rect, ui.id().with("btn_live"), Sense::click());
            let is_live = self.audio.is_some();
            let fill = if is_live {
                Color32::from_rgba_unmultiplied(32, 168, 120, 42)
            } else if live_resp.hovered() {
                BG_CONTROL_HOVER
            } else {
                BG_CONTROL
            };
            let stroke_col = if is_live {
                Color32::from_rgb(56, 206, 138)
            } else {
                BORDER_CTRL
            };
            painter.rect(
                live_rect,
                CornerRadius::same(11),
                fill,
                Stroke::new(1.0, stroke_col),
                StrokeKind::Inside,
            );
            let dot_col = if is_live {
                Color32::from_rgb(66, 232, 146)
            } else {
                CYAN_MID
            };
            painter.circle_filled(Pos2::new(live_rect.left() + 12.0, cy), 3.2, dot_col);
            painter.text(
                Pos2::new(live_rect.left() + 21.0, cy),
                Align2::LEFT_CENTER,
                if is_live { "LIVE" } else { "DEMO" },
                FontId::proportional(10.0),
                if is_live { TEXT_PRIMARY } else { TEXT_MUTED },
            );
            if live_resp.clicked() {
                self.toggle_live_audio();
            }
            live_resp.on_hover_text(if is_live {
                "Stop live microphone audio & return to demo signal"
            } else {
                "Start live audio (default input → magic.RIDE DSP → default output)"
            });
        }

        // Preset dropdown popup
        if self.preset_menu_open {
            let popup_pos = Pos2::new(preset_box.left() - 40.0, preset_box.bottom() + 4.0);
            let mut close_menu = false;
            let area_resp = egui::Area::new(egui::Id::new("preset_popup_area"))
                .fixed_pos(popup_pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(Color32::from_rgb(24, 27, 34))
                        .stroke(Stroke::new(1.0, BORDER_CTRL))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::same(6))
                        .show(ui, |ui| {
                            ui.set_min_width(240.0);
                            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                                let mut last_cat = "";
                                for (i, fp) in FACTORY_PRESETS.iter().enumerate() {
                                    if fp.category != last_cat {
                                        if !last_cat.is_empty() {
                                            ui.add_space(4.0);
                                        }
                                        ui.label(
                                            egui::RichText::new(fp.category.to_uppercase())
                                                .size(10.0)
                                                .color(PURPLE_BRIGHT),
                                        );
                                        last_cat = fp.category;
                                    }
                                    let selected = self.preset == Some(i);
                                    let label = egui::RichText::new(fp.name)
                                        .size(12.0)
                                        .color(if selected { CYAN_BRIGHT } else { TEXT_PRIMARY });
                                    if ui.selectable_label(selected, label).clicked() {
                                        self.apply_preset(i);
                                        close_menu = true;
                                    }
                                }
                            });
                        });
                });

            if close_menu
                || (ui.input(|i| i.pointer.any_click())
                    && !box_resp.hovered()
                    && !area_resp.response.hovered())
            {
                self.preset_menu_open = false;
            }
        }
    }

    fn draw_graph_and_knobs(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);

        // Background vertical gradient
        paint_vertical_gradient(&painter, rect, BG_GRAPH_TOP, BG_GRAPH_BOT);

        let graph_top = rect.top() + 34.0;
        let graph_bot = rect.bottom() - 12.0;
        let graph_left = rect.left();
        let graph_right = rect.right() - 28.0;
        let graph_rect = Rect::from_min_max(
            Pos2::new(graph_left, graph_top),
            Pos2::new(graph_right, graph_bot),
        );

        let db_to_y = |db: f32| -> f32 {
            let norm = ((db - GRAPH_MIN_DB) / (GRAPH_MAX_DB - GRAPH_MIN_DB)).clamp(0.0, 1.0);
            graph_rect.bottom() - norm * graph_rect.height()
        };
        let y_to_db = |y: f32| -> f32 {
            let norm = ((graph_rect.bottom() - y) / graph_rect.height()).clamp(0.0, 1.0);
            GRAPH_MIN_DB + norm * (GRAPH_MAX_DB - GRAPH_MIN_DB)
        };

        // Subtle horizontal dB reference gridlines & right-side scale labels
        for &db in &[0.0f32, -6.0, -12.0, -24.0, -36.0, -48.0] {
            let y = db_to_y(db);
            let line_col = if db == 0.0 {
                Color32::from_rgba_unmultiplied(110, 118, 132, 42)
            } else {
                Color32::from_rgba_unmultiplied(110, 118, 132, 22)
            };
            painter.line_segment(
                [Pos2::new(graph_left, y), Pos2::new(graph_right, y)],
                Stroke::new(1.0, line_col),
            );
            painter.text(
                Pos2::new(rect.right() - 6.0, y),
                Align2::RIGHT_CENTER,
                format!("{:.0}", db),
                FontId::proportional(10.0),
                TEXT_DIM,
            );
        }

        // Subtle purple vertical tint in regions where gain reduction (cut) happens
        let n = self.history.len();
        if n >= 2 {
            let step_x = graph_rect.width() / (HISTORY_LEN - 1) as f32;
            let start_x = graph_rect.right() - (n - 1) as f32 * step_x;

            // Vertical subtle cut highlight bands
            for (i, pt) in self.history.iter().enumerate() {
                if pt.gain_db < -1.2 {
                    let x0 = start_x + i as f32 * step_x;
                    let intensity = ((-pt.gain_db) / 14.0).clamp(0.0, 1.0);
                    let band = Rect::from_min_max(
                        Pos2::new(x0, graph_top),
                        Pos2::new(x0 + step_x + 0.5, graph_bot),
                    );
                    painter.rect_filled(
                        band,
                        CornerRadius::ZERO,
                        Color32::from_rgba_unmultiplied(104, 62, 148, (intensity * 36.0) as u8),
                    );
                }
            }

            // Translucent fill meshes between input_db and output_db
            let mut boost_mesh = Mesh::default();
            let mut cut_mesh = Mesh::default();
            let boost_col = Color32::from_rgba_unmultiplied(16, 152, 184, 76);
            let cut_col = Color32::from_rgba_unmultiplied(128, 72, 178, 72);

            let mut in_pts = Vec::with_capacity(n);
            let mut out_pts = Vec::with_capacity(n);

            for i in 0..n {
                let x = start_x + i as f32 * step_x;
                let pt = self.history[i];
                let yi = db_to_y(pt.input_db);
                let yo = db_to_y(pt.output_db);
                in_pts.push(Pos2::new(x, yi));
                out_pts.push(Pos2::new(x, yo));

                if i > 0 {
                    let prev_in = in_pts[i - 1];
                    let prev_out = out_pts[i - 1];
                    let cur_in = in_pts[i];
                    let cur_out = out_pts[i];

                    let is_boost = pt.output_db >= pt.input_db;
                    let (mesh, col) = if is_boost {
                        (&mut boost_mesh, boost_col)
                    } else {
                        (&mut cut_mesh, cut_col)
                    };
                    let base = mesh.vertices.len() as u32;
                    mesh.colored_vertex(prev_in, col);
                    mesh.colored_vertex(cur_in, col);
                    mesh.colored_vertex(cur_out, col);
                    mesh.colored_vertex(prev_out, col);
                    mesh.add_triangle(base, base + 1, base + 2);
                    mesh.add_triangle(base, base + 2, base + 3);
                }
            }

            if !boost_mesh.indices.is_empty() {
                painter.add(Shape::mesh(boost_mesh));
            }
            if !cut_mesh.indices.is_empty() {
                painter.add(Shape::mesh(cut_mesh));
            }

            // Input waveform trace (crisp silver-grey)
            painter.add(Shape::line(
                in_pts,
                Stroke::new(1.25, Color32::from_rgba_unmultiplied(204, 210, 220, 175)),
            ));

            // Output waveform trace (teal when boosting, muted purple when cutting)
            for i in 1..n {
                let is_boost = self.history[i].output_db >= self.history[i].input_db;
                let col = if is_boost {
                    Color32::from_rgba_unmultiplied(36, 174, 206, 205)
                } else {
                    Color32::from_rgba_unmultiplied(154, 96, 204, 195)
                };
                painter.line_segment([out_pts[i - 1], out_pts[i]], Stroke::new(1.3, col));
            }
        }

        // --- Range dashed ceiling/floor + Target horizontal purple line ---
        let target_db = self.plain[0];
        let visual_range_db = (self.plain[1] * 0.24).clamp(1.0, 24.0);
        let y_target = db_to_y(target_db);
        let y_upper = db_to_y((target_db + visual_range_db).min(0.0));
        let y_lower = db_to_y((target_db - visual_range_db).max(GRAPH_MIN_DB + 2.0));

        draw_dashed_hline(
            &painter,
            graph_left,
            graph_right,
            y_upper,
            6.0,
            5.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 158, 174, 95)),
        );
        draw_dashed_hline(
            &painter,
            graph_left,
            graph_right,
            y_lower,
            6.0,
            5.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 158, 174, 95)),
        );

        // Left-side dB annotations matching screenshot (+11 dB / -25 dB / -11 dB)
        painter.text(
            Pos2::new(graph_left + 10.0, y_upper - 10.0),
            Align2::LEFT_CENTER,
            format!("+{:.0} dB", visual_range_db),
            FontId::proportional(10.5),
            Color32::from_rgba_unmultiplied(165, 172, 184, 160),
        );
        painter.text(
            Pos2::new(graph_left + 10.0, y_target - 11.0),
            Align2::LEFT_CENTER,
            format!("{:.0} dB", target_db),
            FontId::proportional(11.0),
            PURPLE_BRIGHT,
        );
        painter.text(
            Pos2::new(graph_left + 10.0, y_lower - 10.0),
            Align2::LEFT_CENTER,
            format!("-{:.0} dB", visual_range_db),
            FontId::proportional(10.5),
            Color32::from_rgba_unmultiplied(165, 172, 184, 160),
        );

        // Draggable Target Level horizontal purple line
        let target_hit = Rect::from_min_max(
            Pos2::new(graph_left, y_target - 7.0),
            Pos2::new(graph_right, y_target + 7.0),
        );
        let target_resp = ui
            .interact(target_hit, ui.id().with("target_line_drag"), Sense::drag())
            .on_hover_cursor(CursorIcon::ResizeVertical);

        if target_resp.drag_started() {
            self.drag_snapshot = Some(self.plain);
        }
        if target_resp.dragged() {
            if let Some(pos) = target_resp.interact_pointer_pos() {
                let (min, max, _) = PARAM_RANGES[0];
                self.plain[0] = y_to_db(pos.y).clamp(min, max);
                self.push_params();
            }
        }
        if target_resp.drag_stopped() {
            if let Some(prev) = self.drag_snapshot.take() {
                self.record_undo(prev);
            }
        }

        let target_line_col = if target_resp.hovered() || target_resp.dragged() {
            PURPLE_BRIGHT
        } else {
            PURPLE_TARGET
        };
        painter.line_segment(
            [Pos2::new(graph_left, y_target), Pos2::new(graph_right, y_target)],
            Stroke::new(2.0, target_line_col),
        );

        // --- Luminous Neon Gain-Rider Ribbon Curve ---
        if n >= 2 {
            let step_x = graph_rect.width() / (HISTORY_LEN - 1) as f32;
            let start_x = graph_rect.right() - (n - 1) as f32 * step_x;
            let rider_pts: Vec<(Pos2, f32)> = self
                .history
                .iter()
                .enumerate()
                .map(|(i, pt)| {
                    let x = start_x + i as f32 * step_x;
                    // Plot gain ride relative to target line
                    let y = db_to_y((target_db + pt.gain_db * 0.85).clamp(GRAPH_MIN_DB + 2.0, 0.0));
                    (Pos2::new(x, y), pt.gain_db)
                })
                .collect();

            // 3-pass glow (outer halo -> medium bloom -> bright core)
            for &(width, alpha) in &[(9.5f32, 26u8), (4.6, 84), (2.1, 245)] {
                for i in 1..rider_pts.len() {
                    let (p0, g0) = rider_pts[i - 1];
                    let (p1, g1) = rider_pts[i];
                    let g = (g0 + g1) * 0.5;
                    let base_col = if g >= 0.0 {
                        let t = (g / 8.0).clamp(0.0, 1.0);
                        lerp_color(Color32::from_rgb(168, 224, 255), CYAN_BRIGHT, t)
                    } else {
                        let t = (-g / 8.0).clamp(0.0, 1.0);
                        lerp_color(Color32::from_rgb(184, 194, 248), PURPLE_BRIGHT, t)
                    };
                    let col = Color32::from_rgba_unmultiplied(
                        base_col.r(),
                        base_col.g(),
                        base_col.b(),
                        alpha,
                    );
                    painter.line_segment([p0, p1], Stroke::new(width, col));
                }
            }
        }

        // --- Three Overlaid Rotary Knobs (RANGE, TARGET, SPEED) ---
        let center_x = graph_rect.center().x + 8.0;
        let base_y = rect.bottom() - 88.0;
        let spacing = (graph_rect.width() * 0.215).clamp(142.0, 198.0);

        // 1. RANGE (param index 1)
        let range_pos = Pos2::new(center_x - spacing, base_y - 4.0);
        let range_val_str = format!("±{:.1} dB", (self.plain[1] * 0.24).clamp(0.0, 24.0));
        self.knob(
            ui,
            &painter,
            "knob_range",
            range_pos,
            37.0,
            1,
            "RANGE",
            &range_val_str,
            false,
        );

        // 2. TARGET (param index 0 — larger hero knob)
        let target_pos = Pos2::new(center_x, base_y + 4.0);
        let target_val_str = format!("{:.1} dB", self.plain[0]);
        self.knob(
            ui,
            &painter,
            "knob_target",
            target_pos,
            47.0,
            0,
            "TARGET",
            &target_val_str,
            true,
        );

        // 3. SPEED (param index 2) + turtle/rabbit icons
        let speed_pos = Pos2::new(center_x + spacing, base_y - 4.0);
        let speed_val_str = format!("{:.0}%", self.plain[2]);
        self.knob(
            ui,
            &painter,
            "knob_speed",
            speed_pos,
            37.0,
            2,
            "SPEED",
            &speed_val_str,
            false,
        );
        draw_turtle_icon(
            &painter,
            Pos2::new(speed_pos.x - 42.0, speed_pos.y + 33.0),
            Color32::from_rgb(92, 97, 108),
        );
        draw_rabbit_icon(
            &painter,
            Pos2::new(speed_pos.x + 42.0, speed_pos.y + 32.0),
            Color32::from_rgb(92, 97, 108),
        );

        // Optional non-intrusive error banner at top of graph if live audio failed
        if let Some(err) = &self.error {
            let banner = Rect::from_min_max(
                Pos2::new(rect.left() + 14.0, rect.top() + 8.0),
                Pos2::new(rect.right() - 14.0, rect.top() + 30.0),
            );
            painter.rect(
                banner,
                CornerRadius::same(4),
                Color32::from_rgba_unmultiplied(160, 42, 54, 210),
                Stroke::new(1.0, Color32::from_rgb(230, 86, 98)),
                StrokeKind::Inside,
            );
            painter.text(
                banner.center(),
                Align2::CENTER_CENTER,
                err,
                FontId::proportional(11.0),
                TEXT_PRIMARY,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn knob(
        &mut self,
        ui: &mut egui::Ui,
        painter: &egui::Painter,
        id_salt: &str,
        center: Pos2,
        radius: f32,
        param_idx: usize,
        label: &str,
        readout: &str,
        is_hero: bool,
    ) {
        let hit_rect = Rect::from_center_size(center, Vec2::splat(radius * 2.25));
        let resp = ui
            .interact(hit_rect, ui.id().with(id_salt), Sense::click_and_drag())
            .on_hover_cursor(CursorIcon::ResizeVertical);

        let (min, max, default_val) = PARAM_RANGES[param_idx];

        if resp.drag_started() {
            self.drag_snapshot = Some(self.plain);
        }
        if resp.dragged() {
            let fine = ui.input(|i| i.modifiers.shift);
            let sens = if fine { 0.0012 } else { 0.0045 };
            let delta_norm = -resp.drag_delta().y * sens;
            let cur_norm = Params::normalize(param_idx, self.plain[param_idx]);
            let next_norm = (cur_norm + delta_norm).clamp(0.0, 1.0);
            self.plain[param_idx] = Params::denormalize(param_idx, next_norm);
            self.push_params();
        }
        if resp.drag_stopped() {
            if let Some(prev) = self.drag_snapshot.take() {
                self.record_undo(prev);
            }
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.01 {
                let prev = self.plain;
                let cur_norm = Params::normalize(param_idx, self.plain[param_idx]);
                let next_norm = (cur_norm + scroll * 0.0015).clamp(0.0, 1.0);
                self.plain[param_idx] = Params::denormalize(param_idx, next_norm);
                self.record_undo(prev);
                self.push_params();
            }
        }
        if resp.double_clicked() {
            let prev = self.plain;
            self.plain[param_idx] = default_val;
            self.record_undo(prev);
            self.push_params();
        }

        let norm = ((self.plain[param_idx] - min) / (max - min)).clamp(0.0, 1.0);

        // Soft drop shadow
        painter.circle_filled(
            center + Vec2::new(0.0, 5.0),
            radius + 4.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 105),
        );

        // Outer bezel ring
        painter.circle_filled(center, radius + 2.0, Color32::from_rgb(16, 18, 23));

        // 270-degree sweep: 135 deg (bottom-left) clockwise to 405 deg (bottom-right)
        let start_angle = 0.75 * PI; // 135° (screen +y is down, so 135° is bottom-left)
        let sweep_total = 1.5 * PI; // 270°
        let val_angle = start_angle + norm * sweep_total;

        // Background track arc
        let arc_r = radius - 2.0;
        let arc_w = if is_hero { 5.2 } else { 4.2 };
        draw_arc(
            painter,
            center,
            arc_r,
            start_angle,
            start_angle + sweep_total,
            48,
            arc_w,
            Color32::from_rgb(38, 41, 50),
            Color32::from_rgb(38, 41, 50),
        );

        // Active purple-lavender gradient arc
        if norm > 0.005 {
            draw_arc(
                painter,
                center,
                arc_r,
                start_angle,
                val_angle,
                44,
                arc_w,
                PURPLE_DEEP,
                PURPLE_BRIGHT,
            );
        }

        // Inner matte dark knob cap
        let inner_r = radius - 6.5;
        painter.circle_filled(center, inner_r, Color32::from_rgb(28, 30, 38));
        // Subtle top-down sheen on cap
        painter.circle_filled(
            center + Vec2::new(0.0, -inner_r * 0.16),
            inner_r * 0.84,
            Color32::from_rgba_unmultiplied(54, 52, 68, 46),
        );
        painter.circle_stroke(
            center,
            inner_r,
            Stroke::new(
                1.0,
                if resp.hovered() || resp.dragged() {
                    Color32::from_rgba_unmultiplied(190, 150, 255, 110)
                } else {
                    Color32::from_rgb(46, 49, 60)
                },
            ),
        );

        // Needle pointer inside cap
        let dir = Vec2::new(val_angle.cos(), val_angle.sin());
        let p_start = center + dir * (inner_r * 0.22);
        let p_end = center + dir * (inner_r * 0.86);
        painter.line_segment(
            [p_start, p_end],
            Stroke::new(
                if is_hero { 2.0 } else { 1.7 },
                if resp.dragged() { TEXT_PRIMARY } else { PURPLE_BRIGHT },
            ),
        );

        // Label below knob
        painter.text(
            Pos2::new(center.x, center.y + radius + 18.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(if is_hero { 11.5 } else { 11.0 }),
            Color32::from_rgb(206, 212, 222),
        );

        // Floating readout pill on hover or active drag
        if resp.hovered() || resp.dragged() {
            let pill = Rect::from_center_size(
                Pos2::new(center.x, center.y - radius - 16.0),
                Vec2::new(72.0, 18.0),
            );
            painter.rect(
                pill,
                CornerRadius::same(9),
                Color32::from_rgba_unmultiplied(16, 18, 24, 232),
                Stroke::new(1.0, PURPLE_MID),
                StrokeKind::Inside,
            );
            painter.text(
                pill.center(),
                Align2::CENTER_CENTER,
                readout,
                FontId::proportional(10.5),
                TEXT_PRIMARY,
            );
        }
    }

    fn draw_meters(&self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG_METERS);
        painter.line_segment(
            [rect.left_top(), rect.left_bottom()],
            Stroke::new(1.0, BORDER_SUBTLE),
        );

        // Top readouts (Output peak in coral/red, Gain ride in cyan/lavender)
        let peak_txt = if self.display_peak_db >= -0.2 {
            "0".to_string()
        } else {
            format!("{:.0}", self.display_peak_db.clamp(-60.0, 0.0))
        };
        let gain_txt = if self.display_gain_db >= 0.05 {
            format!("+{:.1}", self.display_gain_db)
        } else {
            format!("{:.1}", self.display_gain_db)
        };

        let left_x = rect.left() + 18.0;
        let right_x = rect.right() - 20.0;

        painter.text(
            Pos2::new(left_x, rect.top() + 14.0),
            Align2::CENTER_CENTER,
            peak_txt,
            FontId::proportional(10.5),
            Color32::from_rgb(236, 94, 96),
        );
        painter.text(
            Pos2::new(right_x, rect.top() + 14.0),
            Align2::CENTER_CENTER,
            gain_txt,
            FontId::proportional(10.5),
            if self.display_gain_db >= 0.0 {
                CYAN_BRIGHT
            } else {
                PURPLE_BRIGHT
            },
        );

        let bar_top = rect.top() + 34.0;
        let bar_bot = rect.bottom() - 14.0;
        let bar_w = 13.0;

        // 1. Output level gradient bar (green -> yellow -> orange -> red)
        let out_track = Rect::from_min_max(
            Pos2::new(left_x - bar_w * 0.5, bar_top),
            Pos2::new(left_x + bar_w * 0.5, bar_bot),
        );
        painter.rect_filled(out_track, CornerRadius::same(3), Color32::from_rgb(25, 28, 35));

        let out_norm = ((self.display_out_db - (-60.0)) / 60.0).clamp(0.0, 1.0);
        if out_norm > 0.005 {
            let fill_top = out_track.bottom() - out_norm * out_track.height();
            let fill_rect = Rect::from_min_max(
                Pos2::new(out_track.left(), fill_top),
                out_track.max,
            );
            paint_meter_gradient(&painter, out_track, fill_rect);
        }

        // Peak hold red tick
        let peak_norm = ((self.display_peak_db - (-60.0)) / 60.0).clamp(0.0, 1.0);
        let peak_y = out_track.bottom() - peak_norm * out_track.height();
        painter.line_segment(
            [
                Pos2::new(out_track.left() - 1.0, peak_y),
                Pos2::new(out_track.right() + 1.0, peak_y),
            ],
            Stroke::new(2.5, Color32::from_rgb(238, 86, 88)),
        );

        // 2. Bipolar Gain Ride meter (center = 0 dB; up = cyan boost, down = purple cut)
        let gr_track = Rect::from_min_max(
            Pos2::new(right_x - bar_w * 0.5, bar_top),
            Pos2::new(right_x + bar_w * 0.5, bar_bot),
        );
        painter.rect_filled(gr_track, CornerRadius::same(3), Color32::from_rgb(25, 28, 35));

        let mid_y = gr_track.center().y;
        painter.line_segment(
            [Pos2::new(gr_track.left(), mid_y), Pos2::new(gr_track.right(), mid_y)],
            Stroke::new(1.0, Color32::from_rgb(56, 62, 74)),
        );

        let gr_norm = (self.display_gain_db / 14.0).clamp(-1.0, 1.0);
        let half_h = gr_track.height() * 0.48;
        if gr_norm > 0.01 {
            let y_top = mid_y - gr_norm * half_h;
            let r = Rect::from_min_max(
                Pos2::new(gr_track.left(), y_top),
                Pos2::new(gr_track.right(), mid_y),
            );
            paint_vertical_gradient(
                &painter,
                r,
                CYAN_BRIGHT,
                Color32::from_rgb(16, 118, 152),
            );
        } else if gr_norm < -0.01 {
            let y_bot = mid_y + (-gr_norm) * half_h;
            let r = Rect::from_min_max(
                Pos2::new(gr_track.left(), mid_y),
                Pos2::new(gr_track.right(), y_bot),
            );
            paint_vertical_gradient(
                &painter,
                r,
                Color32::from_rgb(112, 68, 178),
                PURPLE_BRIGHT,
            );
        }
    }

    fn draw_footer(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, BG_FOOTER);
        painter.line_segment(
            [rect.left_top(), rect.right_top()],
            Stroke::new(1.0, BORDER_SUBTLE),
        );

        let cy = rect.center().y;

        // Version label on far left
        painter.text(
            Pos2::new(rect.left() + 18.0, cy),
            Align2::LEFT_CENTER,
            format!("v{}", VERSION),
            FontId::proportional(10.5),
            TEXT_DIM,
        );

        // Center toggle group: NATURAL | SMART SILENCE | AUTO-TARGET
        let cx = rect.center().x - 10.0;

        // 1. NATURAL
        let nat_active = self.plain[10] >= 0.5;
        let nat_rect = Rect::from_center_size(Pos2::new(cx - 128.0, cy), Vec2::new(92.0, 24.0));
        let nat_resp = ui.interact(nat_rect, ui.id().with("tog_natural"), Sense::click());
        let nat_col = if nat_active {
            Color32::from_rgb(174, 212, 186)
        } else if nat_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        draw_leaf_icon(&painter, Pos2::new(nat_rect.left() + 12.0, cy), nat_col);
        painter.text(
            Pos2::new(nat_rect.left() + 26.0, cy),
            Align2::LEFT_CENTER,
            "NATURAL",
            FontId::proportional(10.0),
            nat_col,
        );
        if nat_resp.clicked() {
            let prev = self.plain;
            self.plain[10] = if nat_active { 0.0 } else { 1.0 };
            self.record_undo(prev);
            self.push_params();
        }

        // 2. SMART SILENCE
        let ss_active = self.plain[11] >= 0.5;
        let ss_rect = Rect::from_center_size(Pos2::new(cx + 8.0, cy), Vec2::new(124.0, 24.0));
        let ss_resp = ui.interact(ss_rect, ui.id().with("tog_smart_silence"), Sense::click());
        let ss_col = if ss_active {
            PURPLE_BRIGHT
        } else if ss_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        draw_silence_icon(&painter, Pos2::new(ss_rect.left() + 14.0, cy), ss_col);
        painter.text(
            Pos2::new(ss_rect.left() + 29.0, cy),
            Align2::LEFT_CENTER,
            "SMART SILENCE",
            FontId::proportional(10.0),
            ss_col,
        );
        if ss_resp.clicked() {
            let prev = self.plain;
            self.plain[11] = if ss_active { 0.0 } else { 1.0 };
            self.record_undo(prev);
            self.push_params();
        }

        // 3. AUTO-TARGET
        let at_rect = Rect::from_center_size(Pos2::new(cx + 152.0, cy), Vec2::new(112.0, 24.0));
        let at_resp = ui.interact(at_rect, ui.id().with("tog_auto_target"), Sense::click());
        let at_col = if self.auto_target_enabled {
            CYAN_BRIGHT
        } else if at_resp.hovered() {
            TEXT_PRIMARY
        } else {
            TEXT_MUTED
        };
        draw_auto_target_icon(&painter, Pos2::new(at_rect.left() + 13.0, cy), at_col);
        painter.text(
            Pos2::new(at_rect.left() + 27.0, cy),
            Align2::LEFT_CENTER,
            "AUTO-TARGET",
            FontId::proportional(10.0),
            at_col,
        );
        if at_resp.clicked() {
            self.auto_target_enabled = !self.auto_target_enabled;
        }

        // Right group: AUTO [ OFF v ]  [resize]
        let resize_rect = Rect::from_center_size(
            Pos2::new(rect.right() - 22.0, cy),
            Vec2::new(24.0, 24.0),
        );
        let resize_resp = ui.interact(resize_rect, ui.id().with("btn_resize"), Sense::click());
        draw_expand_icon(
            &painter,
            resize_rect.center(),
            if resize_resp.hovered() { TEXT_PRIMARY } else { TEXT_MUTED },
        );
        if resize_resp.clicked() {
            self.expanded_window = !self.expanded_window;
            let size = if self.expanded_window {
                Vec2::new(1200.0, 780.0)
            } else {
                Vec2::new(980.0, 640.0)
            };
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
        }

        let auto_box = Rect::from_center_size(
            Pos2::new(resize_rect.left() - 56.0, cy),
            Vec2::new(84.0, 22.0),
        );
        painter.text(
            Pos2::new(auto_box.left() - 10.0, cy),
            Align2::RIGHT_CENTER,
            "AUTO",
            FontId::proportional(10.5),
            TEXT_MUTED,
        );

        let auto_resp = ui.interact(auto_box, ui.id().with("auto_dropdown"), Sense::click());
        painter.rect(
            auto_box,
            CornerRadius::same(4),
            if auto_resp.hovered() || self.auto_menu_open {
                BG_CONTROL_HOVER
            } else {
                BG_CONTROL
            },
            Stroke::new(1.0, BORDER_CTRL),
            StrokeKind::Inside,
        );
        painter.text(
            Pos2::new(auto_box.left() + 12.0, cy),
            Align2::LEFT_CENTER,
            self.auto_mode.label(),
            FontId::proportional(10.5),
            if self.auto_mode == AutoMode::Off {
                TEXT_PRIMARY
            } else {
                CYAN_BRIGHT
            },
        );
        draw_triangle(
            &painter,
            Pos2::new(auto_box.right() - 11.0, cy + 0.5),
            3.5,
            TriDir::Down,
            TEXT_MUTED,
        );
        if auto_resp.clicked() {
            self.auto_menu_open = !self.auto_menu_open;
        }

        if self.auto_menu_open {
            let popup_pos = Pos2::new(auto_box.left(), auto_box.top() - 96.0);
            let mut close = false;
            let area_resp = egui::Area::new(egui::Id::new("auto_mode_popup"))
                .fixed_pos(popup_pos)
                .order(egui::Order::Foreground)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(Color32::from_rgb(24, 27, 34))
                        .stroke(Stroke::new(1.0, BORDER_CTRL))
                        .corner_radius(CornerRadius::same(5))
                        .inner_margin(Margin::same(4))
                        .show(ui, |ui| {
                            ui.set_min_width(auto_box.width() - 8.0);
                            for mode in AutoMode::ALL {
                                let sel = self.auto_mode == mode;
                                if ui
                                    .selectable_label(
                                        sel,
                                        egui::RichText::new(mode.label())
                                            .size(11.0)
                                            .color(if sel { CYAN_BRIGHT } else { TEXT_PRIMARY }),
                                    )
                                    .clicked()
                                {
                                    self.auto_mode = mode;
                                    close = true;
                                }
                            }
                        });
                });
            if close
                || (ui.input(|i| i.pointer.any_click())
                    && !auto_resp.hovered()
                    && !area_resp.response.hovered())
            {
                self.auto_menu_open = false;
            }
        }
    }

    fn draw_settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let mut open = self.settings_open;
        egui::Window::new("Advanced DSP & Audio Settings")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(380.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.audio.is_none() {
                        if ui.button("▶ Start Live Audio").clicked() {
                            self.toggle_live_audio();
                        }
                    } else if ui.button("■ Stop Live Audio").clicked() {
                        self.toggle_live_audio();
                    }
                    if ui.button("Reset Defaults").clicked() {
                        let prev = self.plain;
                        self.plain = Params::default().plain();
                        self.record_undo(prev);
                        self.push_params();
                    }
                });
                if let Some(rt) = &self.audio {
                    ui.small(format!(
                        "{} Hz · IN: {} · OUT: {}",
                        rt.sample_rate, rt.input_name, rt.output_name
                    ));
                } else {
                    ui.small("Running synthetic vocal demo through reconstructed Rider DSP.");
                }
                ui.separator();

                ui.checkbox(&mut self.use_lufs, "Use LUFS Target Flag");
                ui.add_space(4.0);

                // Remaining 12 recovered DSP parameters
                for &i in &[3usize, 4, 5, 6, 7, 8, 9, 12, 13, 14] {
                    let (min, max, _) = PARAM_RANGES[i];
                    let unit = PARAM_UNITS[i];
                    let mut v = self.plain[i];
                    let slider = egui::Slider::new(&mut v, min..=max)
                        .text(PARAM_LABELS[i])
                        .custom_formatter(move |val, _| {
                            if unit.is_empty() {
                                format!("{val:.2}")
                            } else {
                                format!("{val:.2} {unit}")
                            }
                        });
                    if ui.add(slider).changed() {
                        self.plain[i] = v;
                        self.push_params();
                    }
                }
            });
        self.settings_open = open;
    }

    fn draw_help_window(&mut self, ctx: &egui::Context) {
        if !self.help_open {
            return;
        }
        let mut open = self.help_open;
        egui::Window::new("magic.RIDE — Quick Guide")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(340.0)
            .show(ctx, |ui| {
                ui.label("• Drag RANGE, TARGET, or SPEED knobs vertically (hold Shift for fine control).");
                ui.label("• Double-click any knob to reset it to its default value.");
                ui.label("• Drag the horizontal purple TARGET line directly on the waveform graph.");
                ui.label("• Click DEMO / LIVE in the top bar to switch between synthetic vocal preview and live macOS audio.");
                ui.label("• Click the gear icon (⚙) to access all 15 recovered DSP parameters (Attack, Release, Hold, Breath Reduction, Noise Floor, Output Trim, etc.).");
                ui.separator();
                ui.small(format!(
                    "MBM Audio magic.RIDE v{} · 48 factory presets · {} state keys",
                    VERSION,
                    STATE_KEYS.len()
                ));
            });
        self.help_open = open;
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().clamp(0.001, 0.05);
        self.last_frame = now;

        // Advance either live audio queue or synthetic vocal demo
        if self.audio.is_some() {
            let mut drained = Vec::new();
            if let Ok(mut q) = self.live_trace_rx.try_lock() {
                while let Some(pt) = q.pop_front() {
                    drained.push(pt);
                }
            }
            for pt in drained {
                self.push_trace(pt);
            }
            let m = *self.meters.lock().unwrap_or_else(|e| e.into_inner());
            self.update_meter_ballistics(m.output_db, m.gain_db, dt);
        } else if self.demo_mode {
            self.step_demo(dt);
        }

        // Request smooth ~60 fps animation
        ui.ctx().request_repaint_after(Duration::from_millis(16));

        // Full-bleed layout: Header (56px) | Main (Graph + 68px Meter Strip) | Footer (36px)
        let full = ui.available_rect_before_wrap();
        let header_h = 56.0;
        let footer_h = 36.0;
        let meter_w = 66.0;

        let header_rect = Rect::from_min_max(full.min, Pos2::new(full.right(), full.top() + header_h));
        let footer_rect = Rect::from_min_max(
            Pos2::new(full.left(), full.bottom() - footer_h),
            full.max,
        );
        let main_rect = Rect::from_min_max(
            Pos2::new(full.left(), header_rect.bottom()),
            Pos2::new(full.right(), footer_rect.top()),
        );
        let graph_rect = Rect::from_min_max(
            main_rect.min,
            Pos2::new((main_rect.right() - meter_w).max(main_rect.left() + 100.0), main_rect.bottom()),
        );
        let meter_rect = Rect::from_min_max(
            Pos2::new(graph_rect.right(), main_rect.top()),
            main_rect.max,
        );

        self.draw_graph_and_knobs(ui, graph_rect);
        self.draw_meters(ui, meter_rect);
        self.draw_header(ui, header_rect);
        self.draw_footer(ui, footer_rect);

        self.draw_settings_window(ui.ctx());
        self.draw_help_window(ui.ctx());
    }
}

// ---------------------------------------------------------------------------
// Custom Vector Drawing Helpers
// ---------------------------------------------------------------------------

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
        (a.a() as f32 + (b.a() as f32 - a.a() as f32) * t) as u8,
    )
}

fn paint_vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(Shape::mesh(mesh));
}

fn paint_meter_gradient(painter: &egui::Painter, full_track: Rect, fill: Rect) {
    // 4-stop vertical meter: green (bottom) -> yellow-green -> warm orange -> coral-red (top)
    let stops: [(f32, Color32); 4] = [
        (0.0, Color32::from_rgb(16, 178, 86)),
        (0.55, Color32::from_rgb(86, 206, 76)),
        (0.80, Color32::from_rgb(242, 182, 54)),
        (1.0, Color32::from_rgb(244, 114, 52)),
    ];
    let sample_col = |y: f32| -> Color32 {
        let t = ((full_track.bottom() - y) / full_track.height()).clamp(0.0, 1.0);
        for w in stops.windows(2) {
            if t >= w[0].0 && t <= w[1].0 {
                let local = (t - w[0].0) / (w[1].0 - w[0].0);
                return lerp_color(w[0].1, w[1].1, local);
            }
        }
        stops[3].1
    };

    let slices = 12;
    let dy = fill.height() / slices as f32;
    for i in 0..slices {
        let y0 = fill.top() + i as f32 * dy;
        let y1 = (y0 + dy + 0.5).min(fill.bottom());
        let r = Rect::from_min_max(Pos2::new(fill.left(), y0), Pos2::new(fill.right(), y1));
        paint_vertical_gradient(painter, r, sample_col(y0), sample_col(y1));
    }
}

fn draw_arc(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    start: f32,
    end: f32,
    segments: usize,
    width: f32,
    col_start: Color32,
    col_end: Color32,
) {
    if segments == 0 || (end - start).abs() < 1e-4 {
        return;
    }
    let mut prev = center + Vec2::new(start.cos(), start.sin()) * radius;
    for i in 1..=segments {
        let t = i as f32 / segments as f32;
        let a = start + (end - start) * t;
        let cur = center + Vec2::new(a.cos(), a.sin()) * radius;
        painter.line_segment([prev, cur], Stroke::new(width, lerp_color(col_start, col_end, t)));
        prev = cur;
    }
}

fn draw_dashed_hline(
    painter: &egui::Painter,
    x0: f32,
    x1: f32,
    y: f32,
    dash: f32,
    gap: f32,
    stroke: Stroke,
) {
    let mut x = x0;
    while x < x1 {
        let end = (x + dash).min(x1);
        painter.line_segment([Pos2::new(x, y), Pos2::new(end, y)], stroke);
        x += dash + gap;
    }
}

enum TriDir {
    Left,
    Right,
    Down,
}

fn draw_triangle(painter: &egui::Painter, c: Pos2, r: f32, dir: TriDir, col: Color32) {
    let pts = match dir {
        TriDir::Left => vec![
            Pos2::new(c.x - r, c.y),
            Pos2::new(c.x + r * 0.7, c.y - r * 0.9),
            Pos2::new(c.x + r * 0.7, c.y + r * 0.9),
        ],
        TriDir::Right => vec![
            Pos2::new(c.x + r, c.y),
            Pos2::new(c.x - r * 0.7, c.y - r * 0.9),
            Pos2::new(c.x - r * 0.7, c.y + r * 0.9),
        ],
        TriDir::Down => vec![
            Pos2::new(c.x, c.y + r * 0.75),
            Pos2::new(c.x - r, c.y - r * 0.55),
            Pos2::new(c.x + r, c.y - r * 0.55),
        ],
    };
    painter.add(Shape::convex_polygon(pts, col, Stroke::NONE));
}

fn draw_undo_icon(painter: &egui::Painter, c: Pos2, mirror: bool, col: Color32) {
    let s = if mirror { -1.0 } else { 1.0 };
    let pts = vec![
        Pos2::new(c.x - 5.0 * s, c.y - 1.5),
        Pos2::new(c.x + 2.5 * s, c.y - 1.5),
        Pos2::new(c.x + 5.0 * s, c.y + 0.5),
        Pos2::new(c.x + 5.0 * s, c.y + 3.0),
        Pos2::new(c.x + 2.5 * s, c.y + 4.5),
        Pos2::new(c.x - 1.5 * s, c.y + 4.5),
    ];
    painter.add(Shape::line(pts, Stroke::new(1.4, col)));
    painter.line_segment(
        [
            Pos2::new(c.x - 5.0 * s, c.y - 1.5),
            Pos2::new(c.x - 2.0 * s, c.y - 4.2),
        ],
        Stroke::new(1.4, col),
    );
    painter.line_segment(
        [
            Pos2::new(c.x - 5.0 * s, c.y - 1.5),
            Pos2::new(c.x - 2.0 * s, c.y + 1.2),
        ],
        Stroke::new(1.4, col),
    );
}

fn draw_gear_icon(painter: &egui::Painter, c: Pos2, r: f32, col: Color32) {
    painter.circle_stroke(c, r * 0.62, Stroke::new(2.2, col));
    for i in 0..8 {
        let a = (i as f32) * TAU / 8.0;
        let dir = Vec2::new(a.cos(), a.sin());
        painter.line_segment([c + dir * (r * 0.72), c + dir * (r * 1.08)], Stroke::new(2.2, col));
    }
}

fn draw_leaf_icon(painter: &egui::Painter, c: Pos2, col: Color32) {
    let pts = vec![
        Pos2::new(c.x - 5.0, c.y + 5.0),
        Pos2::new(c.x - 5.5, c.y - 1.5),
        Pos2::new(c.x - 1.0, c.y - 5.5),
        Pos2::new(c.x + 5.5, c.y - 5.5),
        Pos2::new(c.x + 5.0, c.y + 0.5),
        Pos2::new(c.x + 0.5, c.y + 4.5),
        Pos2::new(c.x - 5.0, c.y + 5.0),
    ];
    painter.add(Shape::line(pts, Stroke::new(1.3, col)));
    painter.line_segment(
        [Pos2::new(c.x - 5.0, c.y + 5.0), Pos2::new(c.x + 3.0, c.y - 3.0)],
        Stroke::new(1.1, col),
    );
}

fn draw_silence_icon(painter: &egui::Painter, c: Pos2, col: Color32) {
    let pts = vec![
        Pos2::new(c.x - 7.5, c.y - 3.5),
        Pos2::new(c.x - 3.5, c.y - 3.5),
        Pos2::new(c.x - 1.5, c.y + 4.0),
        Pos2::new(c.x + 1.5, c.y + 4.0),
        Pos2::new(c.x + 3.5, c.y - 3.5),
        Pos2::new(c.x + 7.5, c.y - 3.5),
    ];
    painter.add(Shape::line(pts, Stroke::new(1.3, col)));
}

fn draw_auto_target_icon(painter: &egui::Painter, c: Pos2, col: Color32) {
    painter.circle_stroke(c, 5.8, Stroke::new(1.2, col));
    painter.text(
        c + Vec2::new(0.0, 0.5),
        Align2::CENTER_CENTER,
        "A",
        FontId::proportional(8.5),
        col,
    );
    // Tiny sparkle accent at top-right
    let sp = Pos2::new(c.x + 6.5, c.y - 5.5);
    painter.line_segment([sp + Vec2::new(-2.0, 0.0), sp + Vec2::new(2.0, 0.0)], Stroke::new(1.1, col));
    painter.line_segment([sp + Vec2::new(0.0, -2.0), sp + Vec2::new(0.0, 2.0)], Stroke::new(1.1, col));
}

fn draw_expand_icon(painter: &egui::Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.4, col);
    painter.line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(5.0, 5.0)], s);
    // Top-left arrow head
    painter.line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(-1.0, -5.0)], s);
    painter.line_segment([c + Vec2::new(-5.0, -5.0), c + Vec2::new(-5.0, -1.0)], s);
    // Bottom-right arrow head
    painter.line_segment([c + Vec2::new(5.0, 5.0), c + Vec2::new(1.0, 5.0)], s);
    painter.line_segment([c + Vec2::new(5.0, 5.0), c + Vec2::new(5.0, 1.0)], s);
}

fn draw_turtle_icon(painter: &egui::Painter, c: Pos2, col: Color32) {
    // Dome shell + head + legs
    let shell = vec![
        c + Vec2::new(-7.0, 2.0),
        c + Vec2::new(-5.5, -3.0),
        c + Vec2::new(-1.0, -4.5),
        c + Vec2::new(4.0, -3.0),
        c + Vec2::new(6.0, 2.0),
    ];
    painter.add(Shape::convex_polygon(shell, col, Stroke::NONE));
    painter.circle_filled(c + Vec2::new(8.2, 0.0), 2.3, col);
    painter.line_segment([c + Vec2::new(-4.5, 2.0), c + Vec2::new(-5.5, 5.0)], Stroke::new(1.8, col));
    painter.line_segment([c + Vec2::new(3.5, 2.0), c + Vec2::new(4.5, 5.0)], Stroke::new(1.8, col));
}

fn draw_rabbit_icon(painter: &egui::Painter, c: Pos2, col: Color32) {
    // Compact crouching rabbit silhouette with two ears
    let body = vec![
        c + Vec2::new(-6.0, 3.0),
        c + Vec2::new(-4.0, -2.5),
        c + Vec2::new(2.0, -2.5),
        c + Vec2::new(5.0, 1.5),
        c + Vec2::new(3.0, 4.0),
        c + Vec2::new(-4.5, 4.0),
    ];
    painter.add(Shape::convex_polygon(body, col, Stroke::NONE));
    painter.circle_filled(c + Vec2::new(4.2, -2.2), 2.8, col);
    // Ears
    painter.line_segment([c + Vec2::new(3.5, -4.5), c + Vec2::new(2.2, -10.0)], Stroke::new(1.7, col));
    painter.line_segment([c + Vec2::new(5.0, -4.5), c + Vec2::new(5.8, -9.8)], Stroke::new(1.7, col));
    // Tail
    painter.circle_filled(c + Vec2::new(-6.5, 1.5), 1.6, col);
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([980.0, 640.0])
            .with_min_inner_size([760.0, 500.0])
            .with_title("magic.RIDE — Precision Vocal Leveling"),
        ..Default::default()
    };
    eframe::run_native(
        "magic.RIDE",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BG_HEADER;
            visuals.window_fill = Color32::from_rgb(22, 25, 32);
            visuals.window_stroke = Stroke::new(1.0, BORDER_CTRL);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::<App>::default())
        }),
    )
}
