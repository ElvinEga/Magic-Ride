# magic.RIDE (macOS Rust Reconstruction)

Native macOS **VST3**, **CLAP**, and **Standalone App** reconstruction of MBM Audio's **magic.RIDE 1.3.1** precision vocal leveling plugin, written in pure Rust.

Features a vector dark interface rendered with `egui`, real-time scrolling waveform visualization, custom rotary controls, peak/gain meters, A/B snapshot compare, undo/redo parameter history, and all 48 factory presets recovered from the original Windows binary.

---

## Features

- **Vector Dark UI**: Faithful reproduction of the MBM Audio interface rendered at native resolution.
- **Real-Time Scrolling Graph**:
  - Dual input (silver) and leveled output (purple) waveform traces.
  - Translucent cyan boost and lavender cut mesh fills visualizing dynamic ride action.
  - Multi-pass glowing gain-rider curve.
  - Draggable purple **Target** line and dashed upper/lower range boundaries.
- **Custom Rotary Knobs**:
  - `RANGE`, `TARGET`, and `SPEED` with matte dark faces and 270° gradient arc tracks.
  - Pointer needles, drag value readouts, scroll-wheel support, double-click reset, and fine adjustment (`Shift` + drag).
  - Custom vector turtle and rabbit speed indicators.
- **Vertical Meter Strip**:
  - Fast-attack, peak-hold output meter (green → yellow → orange → red) with numeric peak readout.
  - Bipolar gain-ride meter (cyan boost / lavender cut) with signed dB readout.
- **All 48 Recovered Factory Presets**:
  - Grouped into 5 categories: *Lead Vocals*, *Backgrounds*, *Speaking*, *Rescue*, and *Mattie's Favorites*.
  - Full support for user presets via compatible XML serialization.
- **Workflow Tools**:
  - **A / B** parameter snapshot comparison.
  - Full **Undo / Redo** parameter history (`↶` / `↷`).
  - **Natural Mode**, **Smart Silence**, and **Auto-Target** tracking toggles.
  - Automation mode selector (`OFF`, `READ`, `WRITE`, `LATCH`).
  - **Settings (`⚙`)** drawer exposing all 15 recovered DSP parameters.

---

## Workspace Architecture

```
magic-ride/
├── bundler.toml                     # xtask bundle naming configuration
├── install.sh                       # One-step build and install script for macOS
├── bundle-app.sh                    # Packages the standalone macOS .app bundle
│
├── crates/
│   ├── core/                        # magic-ride-core: DSP, parameter models, presets, XML
│   │   ├── src/dsp.rs               # Gain rider engine, RMS/peak detector, ballistics
│   │   ├── src/params.rs            # 15 recovered automatable parameter definitions
│   │   ├── src/presets.rs           # 48 recovered factory presets with exact values
│   │   └── src/preset_xml.rs        # User preset XML reader/writer
│   │
│   ├── plugin/                      # magic-ride: Unified VST3 + CLAP plugin crate
│   │   ├── src/lib.rs               # nice-plug plugin definition (VST3 Class ID + CLAP descriptor)
│   │   ├── src/params.rs            # Host-exposed automatable parameter definitions
│   │   └── src/editor.rs            # Embedded egui editor (NiceEguiApp) with DAW automation
│   │
│   ├── app/                         # magic-ride-app: Standalone desktop application
│   │   └── src/main.rs              # eframe GUI, cpal live mic audio, and built-in demo engine
│   │
│   ├── clap/                        # Headless CLAP cdylib wrapper (clap-sys)
│   └── vst3/                        # Headless VST3 cdylib wrapper (vst3-sys)
│
└── xtask/                           # nice-plug-xtask automation
    └── src/main.rs                  # Bundles and codesigns .vst3 and .clap bundles
```

---

## Quick Start & Installation

### Prerequisites
- macOS (Apple Silicon or Intel x86_64)
- Rust toolchain (1.80+ recommended): `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`

### 1. Build and Install VST3 & CLAP Plugins

Run the automated installer script:

```bash
cd magic-ride
./install.sh
```

This will:
1. Compile the release cdylib with embedded default font tables (`epaint_default_fonts`).
2. Generate valid macOS `.vst3` and `.clap` bundles via `xtask`.
3. Ad-hoc codesign and clear macOS quarantine flags (`xattr -cr`).
4. Install them directly into your audio plug-ins directory:
   - `~/Library/Audio/Plug-Ins/VST3/magic.RIDE.vst3`
   - `~/Library/Audio/Plug-Ins/CLAP/magic.RIDE.clap`

### 2. Build the Standalone macOS App

```bash
cd magic-ride
./bundle-app.sh
```

This creates `magic.RIDE Standalone.app` in `magic-ride/`. Double-click to launch.
- Starts in **DEMO** mode running synthetic vocal envelopes through the DSP engine so controls can be tested immediately without audio hardware.
- Click **DEMO / LIVE** in the header to switch to live microphone/audio input via `cpal`.

---

## Parameters

| Index | Parameter ID | Display Name | Unit | Range | Default |
|:---:|:---|:---|:---:|:---:|:---:|
| 0 | `targetLevel` | Target Level | dBFS | -40.0 .. 0.0 | -18.0 dB |
| 1 | `range` | Range | dB | 0.0 .. 100.0 | 50.0 dB |
| 2 | `speed` | Speed | % | 0.0 .. 100.0 | 6.0 % |
| 3 | `cutRange` | Cut Range | dB | 0.0 .. 200.0 | 100.0 dB |
| 4 | `boostRange` | Boost Range | dB | 0.0 .. 500.0 | 400.0 dB |
| 5 | `attack` | Attack | ms | 0.1 .. 50.0 | 2.0 ms |
| 6 | `release` | Release | ms | 0.1 .. 50.0 | 10.0 ms |
| 7 | `hold` | Hold | ms | 0.0 .. 200.0 | 50.0 ms |
| 8 | `breathReduction` | Breath Reduction | — | 0.0 .. 1.0 | 1.0 |
| 9 | `transientPreservation`| Transient Preservation | % | 0.0 .. 100.0 | 0.0 % |
| 10 | `naturalMode` | Natural Mode | bool | 0 or 1 | 1 (On) |
| 11 | `smartSilence` | Smart Silence | bool | 0 or 1 | 0 (Off) |
| 12 | `outputTrim` | Output Trim | dB | -12.0 .. +12.0 | 0.0 dB |
| 13 | `noiseFloor` | Noise Floor | dBFS | -100.0 .. 0.0 | -100.0 dB |
| 14 | `gainOutput` | Gain Output | dB | -1.0 .. 12.0 | -1.0 (Auto) |

---

## Running Tests

Verify core DSP tests, preset integrity, and XML round-trip serialization:

```bash
cd magic-ride
cargo test
```

---

## Reverse-Engineering Evidence & Technical Notes

For detailed reverse-engineering findings, PE disassembly notes, parameter ID offsets, and preset struct layouts, see [ANALYSIS.md](file:///Users/solution/Development/rea/Ride/ANALYSIS.md) and [analysis.json](file:///Users/solution/Development/rea/Ride/analysis.json).
