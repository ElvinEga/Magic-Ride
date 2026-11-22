//! Standalone macOS test host for the reverse-engineered magic.RIDE DSP.
//!
//! Live audio: default input device -> reconstructed gain rider -> default output.
//! Includes the 48 recovered factory presets and the recovered parameter set.

use eframe::egui;
use magic_ride_core::*;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
use cpal::traits::{DeviceTrait, HostTrait};
use std::sync::{Arc, Mutex};

#[derive(Default, Clone, Copy)]
struct Meters {
    input_db: f32,
    output_db: f32,
    gain_db: f32,
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
    params: Arc<Mutex<Params>>,
    meters: Arc<Mutex<Meters>>,
    audio: Option<AudioRuntime>,
    preset: usize,
    error: Option<String>,
}

impl Default for App {
    fn default() -> Self {
        let plain = Params::default().plain();
        Self {
            params: Arc::new(Mutex::new(Params::from_plain(&plain))),
            meters: Arc::new(Mutex::new(Meters::default())),
            plain,
            audio: None,
            preset: 0,
            error: None,
        }
    }
}

impl App {
    fn push_params(&mut self) {
        let p = Params::from_plain(&self.plain);
        if let Ok(mut shared) = self.params.lock() {
            *shared = p;
        }
    }

    fn start_audio(&mut self) -> Result<AudioRuntime, String> {
        let host = cpal::default_host();
        let in_dev = host.default_input_device().ok_or("No default input device found")?;
        let out_dev = host.default_output_device().ok_or("No default output device found")?;
        let in_cfg = in_dev.default_input_config().map_err(|e| e.to_string())?;
        let out_cfg = out_dev.default_output_config().map_err(|e| e.to_string())?;
        let in_channels = in_cfg.channels() as usize;
        let out_channels = out_cfg.channels() as usize;
        let sr_in = in_cfg.sample_rate();
        let sr_out = out_cfg.sample_rate();
        if sr_in != sr_out {
            return Err(format!(
                "Input ({sr_in} Hz) and output ({sr_out} Hz) sample rates differ; set them equal in macOS Audio MIDI Setup"
            ));
        }

        let capacity = (sr_in as usize) * 4;
        let ring = HeapRb::<f32>::new(capacity);
        let (mut producer, mut consumer) = ring.split();
        let params_in = Arc::clone(&self.params);
        let meters_in = Arc::clone(&self.meters);
        let meters_out = Arc::clone(&self.meters);

        let input_stream = in_dev
            .build_input_stream(
                in_cfg.into(),
                move |data: &[f32], _| {
                    let frames = data.len() / in_channels.max(1);
                    for i in 0..frames {
                        let base = i * in_channels;
                        let l = data[base];
                        let r = if in_channels > 1 { data[base + 1] } else { l };
                        let _ = producer.try_push(l);
                        let _ = producer.try_push(r);
                    }
                    if let Ok(mut m) = meters_in.lock() {
                        let mut peak = 0.0f32;
                        for &s in data.iter().take(frames * in_channels) {
                            peak = peak.max(s.abs());
                        }
                        if peak > 0.0 {
                            let mut x = *m;
                            x.input_db = 20.0 * peak.log10();
                            *m = x;
                        }
                    }
                },
                |err| eprintln!("input stream error: {err}"),
                None,
            )
            .map_err(|e| e.to_string())?;

        let mut rider = Rider::new(sr_out as f32);
        let mut last = Params::default();
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
                        gr = rider.process_frame(&mut l, &mut r, &p);
                        out_peak = out_peak.max(l.abs()).max(r.abs());
                        let base = i * out_channels;
                        for ch in 0..out_channels {
                            data[base + ch] = if ch % 2 == 0 { l } else { r };
                        }
                    }
                    if let Ok(mut m) = meters_out.lock() {
                        m.gain_db = gr;
                        if out_peak > 0.0 {
                            m.output_db = 20.0 * out_peak.log10();
                        }
                    }
                },
                |err| eprintln!("output stream error: {err}"),
                None,
            )
            .map_err(|e| e.to_string())?;

        Ok(AudioRuntime {
            _input: input_stream,
            _output: output_stream,
            sample_rate: sr_in,
            input_name: in_dev.to_string(),
            output_name: out_dev.to_string(),
        })
    }
}

fn meter(ui: &mut egui::Ui, label: &str, value_db: f32, min: f32, max: f32) {
    let norm = ((value_db - min) / (max - min)).clamp(0.0, 1.0);
    ui.horizontal(|ui| {
        ui.label(format!("{label:<14}"));
        ui.add(egui::ProgressBar::new(norm).text(format!("{value_db:6.1} dB")));
    });
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            ui.heading("magic.RIDE — reverse-engineered standalone");
            ui.small("MBM Audio · v1.3.1 · Rust reconstruction · 48 factory presets recovered");
            ui.separator();

            ui.horizontal(|ui| {
                if self.audio.is_none() {
                    if ui.button("▶  Start audio").clicked() {
                        match self.start_audio() {
                            Ok(rt) => {
                                self.error = None;
                                self.audio = Some(rt);
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                } else if ui.button("■  Stop audio").clicked() {
                    self.audio = None;
                }
                if ui.button("Reset DSP").clicked() {
                    if let Ok(mut p) = self.params.lock() {
                        *p = Params::from_plain(&self.plain);
                        drop(p);
                    }
                }
            });

            if let Some(e) = &self.error {
                ui.colored_label(egui::Color32::RED, e);
            }
            if let Some(rt) = &self.audio {
                ui.small(format!(
                    "Running · {} Hz · IN: {} · OUT: {}",
                    rt.sample_rate, rt.input_name, rt.output_name
                ));
            } else {
                ui.small("Audio stopped. Grant microphone access when prompted on first start.");
            }
            ui.separator();

            // Presets
            ui.label("Factory preset");
            let current = format!("{} — {}", FACTORY_PRESETS[self.preset].category, FACTORY_PRESETS[self.preset].name);
            egui::ComboBox::from_label("")
                .width(320.0)
                .selected_text(&current)
                .show_ui(ui, |ui| {
                    for (i, p) in FACTORY_PRESETS.iter().enumerate() {
                        let text = format!("{} — {}", p.category, p.name);
                        if ui.selectable_value(&mut self.preset, i, text).clicked() {
                            self.plain = p.params.plain();
                            self.push_params();
                        }
                    }
                });

            ui.separator();
            ui.label("Parameters");
            egui::Grid::new("params").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                for i in 0..15 {
                    let (min, max, _) = PARAM_RANGES[i];
                    let mut v = self.plain[i];
                    let unit = PARAM_UNITS[i];
                    let text = if unit.is_empty() {
                        format!("{v:.2}")
                    } else {
                        format!("{v:.2} {unit}")
                    };
                    if ui
                        .add(
                            egui::Slider::new(&mut v, min..=max)
                                .text(PARAM_LABELS[i])
                                .custom_formatter(move |value, _| {
                                    format!("{:.2}{}", value, if unit.is_empty() { String::new() } else { format!(" {unit}") })
                                }),
                        )
                        .changed()
                    {
                        self.plain[i] = v;
                        self.push_params();
                    }
                    let _ = text;
                    if i % 2 == 1 {
                        ui.end_row();
                    }
                }
            });

            ui.separator();
            ui.label("Meters");
            let m = *self.meters.lock().unwrap_or_else(|e| e.into_inner());
            meter(ui, "Input", m.input_db, -60.0, 0.0);
            meter(ui, "Output", m.output_db, -60.0, 0.0);
            meter(ui, "Gain change", m.gain_db, -40.0, 40.0);

            ui.separator();
        ui.small(format!(
            "State keys: {} · DSP is a behavioural reconstruction, not bit-exact.",
            STATE_KEYS.len()
        ));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([920.0, 820.0])
            .with_title("magic.RIDE — RE Standalone"),
        ..Default::default()
    };
    eframe::run_native(
        "magic.RIDE — RE Standalone",
        options,
        Box::new(|_cc| Ok(Box::<App>::default())),
    )
}
