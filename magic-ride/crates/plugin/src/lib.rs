use nice_plug::editor::dpi::LogicalSize;
use nice_plug::prelude::*;
use nice_plug_egui::{create_egui_editor, EguiEditor, EguiEditorState, EguiNiceSettings, RepaintNotifier};
use std::sync::{Arc, Mutex};

mod editor;
mod params;

use editor::{MagicRideEditor, SharedTelemetry, TracePoint};
use params::MagicRideParams;

const WINDOW_W: u32 = 980;
const WINDOW_H: u32 = 640;

pub struct MagicRide {
    params: Arc<MagicRideParams>,
    editor_state: Arc<EguiEditorState>,
    shared: Arc<Mutex<SharedTelemetry>>,
    rider: magic_ride_core::Rider,
    repaint: RepaintNotifier,
}

impl Default for MagicRide {
    fn default() -> Self {
        Self {
            params: Arc::new(MagicRideParams::default()),
            editor_state: EguiEditorState::from_size(
                LogicalSize::new(WINDOW_W as f32, WINDOW_H as f32),
                1.0,
            ),
            shared: Arc::new(Mutex::new(SharedTelemetry::default())),
            rider: magic_ride_core::Rider::new(48000.0),
            repaint: RepaintNotifier::new(),
        }
    }
}

impl Plugin for MagicRide {
    const NAME: &'static str = "magic.RIDE";
    const VENDOR: &'static str = "MBM Audio";
    const URL: &'static str = "https://mbmaudio.com";
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

    type Editor = EguiEditor<MagicRideEditor>;
    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Self::Editor> {
        let app = MagicRideEditor::new(self.params.clone(), self.shared.clone());
        create_egui_editor(
            self.editor_state.clone(),
            self.repaint.clone(),
            EguiNiceSettings::new().with_tile("magic.RIDE"),
            app,
        )
    }

    fn activate(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl ActivateContext<Self>,
    ) -> bool {
        self.rider.set_sample_rate(buffer_config.sample_rate);
        true
    }

    fn reset(&mut self) {
        self.rider.reset();
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        _context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let p = self.params.to_core();
        let slices = buffer.as_slice();
        if slices.len() >= 2 {
            let (left, right) = slices.split_at_mut(1);
            let l = &mut left[0];
            let r = &mut right[0];
            let len = l.len().min(r.len());
            let mut gr = 0.0f32;
            let mut in_sq = 0.0f32;
            let mut out_sq = 0.0f32;

            for i in 0..len {
                in_sq += 0.5 * (l[i] * l[i] + r[i] * r[i]);
                gr = self.rider.process_frame(&mut l[i], &mut r[i], &p);
                out_sq += 0.5 * (l[i] * l[i] + r[i] * r[i]);
            }

            let inv = 1.0 / len.max(1) as f32;
            let in_db = (20.0 * (in_sq * inv).sqrt().max(1e-6).log10()).clamp(-60.0, 0.0);
            let out_db = (20.0 * (out_sq * inv).sqrt().max(1e-6).log10()).clamp(-60.0, 0.0);

            if let Ok(mut sh) = self.shared.try_lock() {
                sh.is_processing = true;
                sh.meters.input_db = in_db;
                sh.meters.output_db = out_db;
                sh.meters.gain_db = gr;
                if sh.live_trace_queue.len() >= 128 {
                    sh.live_trace_queue.pop_front();
                }
                sh.live_trace_queue.push_back(TracePoint {
                    input_db: in_db,
                    output_db: out_db,
                    gain_db: gr,
                });
            }
        }

        if self.editor_state.is_open() {
            self.repaint.request_repaint();
        }

        ProcessStatus::Normal
    }
}

impl ClapPlugin for MagicRide {
    const CLAP_ID: &'static str = "com.mbmaudio.magicride";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Precision Vocal Leveling");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] = &[
        ClapFeature::AudioEffect,
        ClapFeature::Stereo,
        ClapFeature::Utility,
        ClapFeature::Compressor,
    ];
}

impl Vst3Plugin for MagicRide {
    const VST3_CLASS_ID: [u8; 16] = [
        0xAB, 0xCD, 0xEF, 0x01, 0x91, 0x82, 0xFA, 0xEB, 0x4D, 0x62, 0x6D, 0x61, 0x56, 0x72, 0x6C, 0x72,
    ];
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Fx, Vst3SubCategory::Dynamics];
}

nice_export_clap!(MagicRide);
nice_export_vst3!(MagicRide);
