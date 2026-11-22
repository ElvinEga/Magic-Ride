# magic.RIDE 1.3.1 — Reverse-engineering Analysis

**Target:** `magic.RIDE 1.3.1 Windows Setup.exe` (MBM Audio)
**Goal:** reconstruct the audio plugin as a native macOS VST3 + CLAP in Rust, including factory/user presets.
**Date:** 2026-11-22

---

## 1. Scope and result summary

| Deliverable | Status |
|---|---|
| Extract installer payload | Done — 5 files, all SHA-256 verified against the installer's own data entries |
| Identify format and plugin architecture | Done — Inno Setup 6.7.0 Unicode; VST3 + AAX x86-64 PE DLLs |
| Recover parameter model | Done (IDs and labels observed; ranges inferred) |
| Recover factory presets | Done — 48 presets with names, categories and full value structs |
| Recover user preset format | Done (XML element/attribute keys observed) |
| Recover DSP | Partially — behaviour deduced from parameter descriptions; not bit-exact |
| macOS Rust VST3 + CLAP | Built — `magic-ride/` workspace, release `cdylib`s and bundle script |
| Bit-exact audio parity | **Not achieved / unknown** — original DSP source not present in artifact |

Every claim below is tagged **[observed]**, **[inferred]**, or **[unknown]** and tied to an evidence ID.

---

## 2. Evidence ledger

| ID | Type | Summary |
|---|---|---|
| `E-INSTALLER` | observed | PE32 x86 installer, 9,340,728 bytes, SHA-256 `10e855e9cfb509cab93dbd780cec716fee0767ef99521be9ca9cf8e7db59bfe3`. REA evidence `ev_4e6fe5fbe3d2fe179b1c556bd093c5f9424515945525751e88e0cf2d3552e178`; inventory `ev_d9f8a8e032e70010599b06b5351c7c08bb2a7748c15cd0057d8d4a878f73178f`. |
| `E-INNO` | observed | Setup loader resource 11111 at file offset `0xd9524`, magic `rDlPtS…`, revision 2. Header data at `0x7c07f2`; compressed payload at `0xda400`; compressed setup.exe at `0x7c5a0b`. |
| `E-VERSION` | observed | Setup data string `Inno Setup Setup Data (6.7.0)`; messages `Inno Setup Messages (6.5.0) (u)`. |
| `E-FILES` | observed | 6 file entries / 5 data entries; extracted names and SHA-256 digests matched the embedded `SHA256Sum` values exactly. |
| `E-VST3` | observed | `magic.RIDE.vst3`: PE32+ DLL, x86-64, 8,425,472 bytes, SHA-256 `9548fec114d2f60cce399e9abfc4ae69f83a18997736059e53d2cce71bd6d135`. PE resources: only icon/cursor (REA evidence `ev_06b92bef4528bf1d81c8c6dbb5eb43009e93c89306093c1ae1be309059990af6`). |
| `E-PARAM-IDS` | observed | ASCII parameter IDs in `.rdata` at file offsets `0x5f7768` onward: `targetLevel`, `range`, `speed`, `cutRange`, `boostRange`, `attack`, `gainOutput`, `hold`, `release`, `transientPreservation`, `breathReduction`, `smartSilence`, `naturalMode`, `noiseFloor`, `outputTrim`. |
| `E-STATE-KEYS` | observed | Additional state keys: `attackMs`, `releaseMs`, `holdMs`, `useLufs`, `automationMode`, `lookAheadMode`, `rangeLocked`, `vocalFocusEnabled`, `sidechainEnabled`, `sidechainFollow`, `sidechainAmount`, `presetIndex`, `scrollSpeed`, `uiScalePercent`, `windowSizeIndex`, `VocalRiderState`. |
| `E-LABELS` | observed | UI strings at `0x606fb0`–`0x607640`: `Target Level`, `Speed`, `Range (legacy)`, `Boost Range`, `Cut Range`, `Attack`, `Release`, `Hold`, `Breath Reduction`, `Natural Mode`, `Smart Silence`, `Output Trim`, `Noise Floor`, `Gain Output`, plus descriptions such as “How quickly gain responds”, “Reduces gain on breaths”, “Time to hold gain before release”, “Preserves vocal attacks”. |
| `E-PARAM-CREATION` | observed | Tiny thunks at RVA `0x1ae4`–`0x1da2` construct one parameter per ID using a shared helper. |
| `E-PRESET-TABLE` | observed | Function at RVA `0x1d4930–0x1d653a` builds 48 consecutive 0x50-byte structs on the stack; each begins with two `juce::String` constructions (category, name) from the string blob at `0x5f7838` onward, followed by immediate float/int writes. |
| `E-PRESET-NAMES` | observed | 48 preset names in five categories: Lead Vocals (9 + 7 later), Backgrounds (8), Speaking (9), Rescue (6), Mattie's Favorites (5). |
| `E-USER-PRESET-KEYS` | observed | XML/user-preset keys at `0x5f8484`–`0x5f86d0`: `name`, `UserPreset`, `speed`, `targetLevel`, `attackMs`, `range`, `holdMs`, `releaseMs`, `smartSilence`, `naturalMode`, `breathReduction`, `useLufs`, `noiseFloor`, `transientPreservation`, `outputTrim`, `lookAheadMode`, `boostRange`, `rangeLocked`, `cutRange`; file extension `.xml`, folder `User Presets`, illegal-character set `\/:*?"<>|`. |
| `E-NO-PRESET-FILES` | observed | Installer installs only the VST3, AAX, `moduleinfo.json`, an icon and `desktop.ini`; no `.xml`/preset files. Factory presets are compiled into the DLL. |
| `E-AAX` | observed | AAX binary is a second ~8 MB PE DLL with the same parameter/preset strings, indicating a shared codebase. |
| `E-PARITY` | unknown | No original source, no runtime audio comparison (no Windows host). DSP equivalence cannot be asserted. |

---

## 3. Installer and extraction

### 3.1 Format
The installer is an **Inno Setup 6.7.0 Unicode** setup. Standard `innoextract` (1.9) cannot read it, because 6.7 changed:

1. The loader offset table (revision 2) now uses **64-bit offsets** and a `TotalSize` field:
   `ID[12], Version(u32)=2, TotalSize(i64), OffsetEXE(i64), UncompressedSizeEXE(u32), CRCEXE(i32), Offset0(i64), Offset1(i64), Reserved(u32), TableCRC(i32)` (upstream `Shared.Struct.pas`).
2. `setup-0.bin` begins with a **setup encryption header**: `SetupID[64]`, CRC32(u32), `TSetupEncryptionHeader`(49 bytes), then the compressed block(s).
3. A compressed block header changed from `CRC32 + u32 size + u8 flag` to `CRC32 + i64 size + bool compressed` (13 bytes).
4. The header/file-entry/language/data-entry records gained fields (notably `NumISSigKeyEntries`, `CloseApplicationsFilterExcludes`, `SevenZipLibraryName`, 6 `UsePrevious*` strings, `BaseFilename`, 4 download strings, `TSetupFileVerification`, and the simplified `TSetupFileLocationEntry`).

I patched a source build of `innoextract` to understand version `6.7.0`, then used it to enumerate and extract. **All five extracted SHA-256 digests matched the digests embedded in the installer**, so the extraction is verified.

### 3.2 Installed files

| Destination | Size | SHA-256 (first 16) |
|---|---|---|
| `Common Files/VST3/magic.RIDE.vst3/Contents/Resources/moduleinfo.json` | 1,082 | `5d6e1ae46a6b551f` |
| `Common Files/VST3/magic.RIDE.vst3/Contents/x86_64-win/magic.RIDE.vst3` | 8,425,472 | `9548fec114d2f60c` |
| `Common Files/Avid/.../magic.RIDE.aaxplugin/desktop.ini` | 80 | `34b7fb2daafdf887` |
| `Common Files/Avid/.../magic.RIDE.aaxplugin/Plugin.ico` | 363,247 | `6cd99e8cc4d7c8bd` |
| `Common Files/Avid/.../magic.RIDE.aaxplugin/Contents/x64/magic.RIDE.aaxplugin` | 8,397,120 | `bf5c9ef282ac126d` |

**[Observed]** No CLAP binary and no standalone app are shipped. **[Inferred]** The VST3 and AAX are the same codebase in different wrappers; this reconstruction targets VST3 + CLAP as requested.

---

## 4. Plugin architecture

- JUCE-based audio processor (JUCE strings, `juce::String`/UUID artifacts and `juce::AudioProcessorValueTreeState` ID strings throughout). **[observed]**
- 2-in/2-out audio processor with a **sidechain input** (`Sidechain`, `sidechainEnabled`, `sidechainFollow`, `sidechainAmount`). **[observed from UI/state strings]**
- Meters named `Input`, `Output`, `GainReduction`, `GAIN_METER` and controls `Comp:GainReduction*` suggest JUCE-style meter parameters. **[observed]**
- Feature is a **vocal gain rider**: target level, boost/cut range, speed, attack/hold/release, breath reduction, noise floor, transient preservation, natural mode, smart silence, optional LUFS mode. **[observed from parameter descriptions, E-LABELS]**
- AAX and VST3 are x86-64 PE DLLs; **macOS binaries were not present** and therefore no Mach-O comparison was possible. **[observed / unknown]**

### Parameter model

15 automatable parameters (plain ranges are **[inferred]** from preset value distribution and the UI units):

| # | ID | Label | Unit | Recovered range | Default |
|---|---|---|---|---|---|
| 0 | `targetLevel` | Target Level | dB | -40…0 | -18 |
| 1 | `range` | Range (legacy) | dB | 0…100 | 50 |
| 2 | `speed` | Speed | % | 0…100 | 6 |
| 3 | `cutRange` | Cut Range | dB | 0…200 | 100 |
| 4 | `boostRange` | Boost Range | dB | 0…500 | 400 |
| 5 | `attack` | Attack | ms | 0.1…50 | 2 |
| 6 | `release` | Release | ms | 0.1…50 | 10 |
| 7 | `hold` | Hold | ms | 0…200 | 50 |
| 8 | `breathReduction` | Breath Reduction | — | 0…1 | 1 |
| 9 | `transientPreservation` | Transient Preservation | % | 0…100 | 0 |
| 10 | `naturalMode` | Natural Mode | bool | 0…1 | 1 |
| 11 | `smartSilence` | Smart Silence | bool | 0…1 | 0 |
| 12 | `outputTrim` | Output Trim | dB | -12…12 | 0 |
| 13 | `noiseFloor` | Noise Floor | dB | -100…0 | -100 |
| 14 | `gainOutput` | Gain Output | dB | -1(auto)…12 | -1 |

Non-automatable state keys are listed in `E-STATE-KEYS`; they are preserved in the Rust state model but not exposed as host parameters.

---

## 5. Presets

### 5.1 Factory presets — recovered
**48 presets** were recovered from the stackinitialiser function at RVA `0x1d4930`. Each 0x50-byte record is laid out as:

```
+0x00  juce::String category
+0x08  juce::String name
+0x10  f32 targetLevel
+0x14  f32 range (legacy)
+0x18  f32 speed
+0x1c  f32 cutRange
+0x20  f32 boostRange
+0x28  u16 flags  (bit0 = naturalMode, bit8 = smartSilence)  [inferred]
+0x2a  u8  useLufs                                        [inferred]
+0x2c  f32 attack (ms)
+0x30  f32 hold (ms)
+0x34  f32 noiseFloor (dBFS)
+0x38  u64 transientPreservation/outputTrim (zero in all presets) [inferred]
+0x40  f32 release override (ms; -1 = automatic)           [inferred]
+0x44  f32 gainOutput (dB; -1 = automatic)                 [inferred]
+0x48  u8  breathReduction                                 [inferred]
```

Field-to-parameter names for the floats at `+0x10…+0x20`, `+0x2c`, `+0x30`, `+0x34` are **[observed by value semantics]** (ranges and units match the recovered labels); the packed flags and the `+0x38/+0x40/+0x44/+0x48` mapping are **[inferred]** because those fields are zero or -1 in most presets and no decompiler was available for the consuming function.

The complete decoded table is in `magic-ride/crates/core/src/presets.rs`, e.g.:

| Category | Name | Target | Range | Speed | Cut | Boost | Attack | Hold | Noise floor |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|
| Lead Vocals | Gentle & Natural | -18 | 30 | 6 | 100 | 400 | 2 | 50 | -100 |
| Lead Vocals | Tight Control | -16 | 55 | 8 | 40 | 150 | 2 | 30 | -100 |
| Lead Vocals | Aggressive Mix | -14 | 75 | 12 | 15 | 60 | 2 | 70 | -100 |
| Backgrounds | Backing Vocals | -22 | 35 | 5 | 80 | 350 | 3 | 50 | -45 |
| Speaking | Podcast | -18 | 50 | 9 | 50 | 200 | 4 | 50 | -48 |
| Rescue | Quiet Vocal Recovery | -16 | 60 | 12 | 80 | 300 | 4 | 50 | -45 |
| Mattie's Favorites | Natural LUFS | -18 | 60 | 7 | 40 | 180 | 2 | 50 | -100 |

### 5.2 User presets
**[observed]** User presets are XML files named from the preset name (illegal characters `\/:*?"<>|` replaced with `_`) with extension `.xml`, stored under `User Presets`. Keys are listed in `E-USER-PRESET-KEYS`. The Rust core implements a compatible reader/writer in `preset_xml.rs`.

---

## 6. DSP reconstruction

**[Observed]** from UI text: gain responds to target level, has maximum adjustment, hold before release, separate attack/release speeds, breath reduction, noise floor, transient preservation, natural mode, smart silence and optional LUFS target.

**[Inferred reconstruction]** implemented in `crates/core/src/dsp.rs`:
1. Stereo RMS/peak level detector (peak weighting when transient preservation is on).
2. Gate below the noise floor; breath attenuation of up to -6 dB.
3. Desired gain = target − detected level, clamped by `range`, `cutRange` and `boostRange`.
4. Hold counter, then one-pole attack/release ballistics scaled by `speed`.
5. Optional natural-mode extra smoothing.
6. Output trim and `gainOutput` makeup; meters reported as dB.

**[Unknown]** Exact detector weighting, ballistics constants, LUFS integration, sidechain behaviour, look-ahead delay and transient filter topology. No Windows host was available for runtime probing, and Ghidra was not configured, so a decompilation of the audio callback was not performed.

---

## 7. Rust reconstruction

```
magic-ride/
  Cargo.toml
  bundle.sh                       # builds macOS .vst3 / .clap bundles
  bundle-app.sh                   # builds the double-clickable standalone .app
  crates/core/                    # parameters, 48 presets, XML state, DSP
  crates/clap/                    # CLAP cdylib (clap-sys 0.5)
  crates/vst3/                    # VST3 cdylib (vst3 0.3 COM bindings)
  crates/app/                     # egui + cpal standalone test host
  ANALYSIS.md / analysis.json
```

Build:
```bash
cd magic-ride
cargo test --workspace
cargo build --release --workspace
./bundle.sh
./bundle-app.sh
```

Outputs:
- `bundles/magic.RIDE.vst3` and `bundles/magic.RIDE.clap`
- `magic.RIDE Standalone.app` — a native macOS application that runs the DSP live on the default input/output device, exposes all 15 recovered parameters, all 48 factory presets, and input/output/gain meters.

The standalone app requests microphone access via `NSMicrophoneUsageDescription`; grant it on first launch. It requires the input and output devices to share a sample rate (set this in Audio MIDI Setup if they differ).

Both wrappers:
- expose the 15 recovered parameters with the original IDs;
- accept factory preset values directly (`Params` structs);
- load/save the recovered user-preset XML as plugin state;
- process stereo float audio through the reconstructed rider.

**Limitations of the wrappers:** no custom GUI (the original is a JUCE UI), no sidechain bus exposed yet, no AAX target, parameter `stepCount` and automation flags are set generically. These are deliberate scope decisions, not evidence of original behaviour.

---

## 8. Findings checklist

| Question | Answer | Status |
|---|---|---|
| What is in the installer? | VST3 + AAX DLLs, icon, ini, JSON | Answered |
| Is there a macOS plugin inside? | No | Answered |
| What are the plugin parameters? | 15 IDs + 15 state keys | Answered (ranges inferred) |
| Are there presets? | 48 embedded factory presets + XML user presets | Answered |
| Can preset values be recovered? | Yes, all 48 structs | Answered |
| Is the DSP bit-exact? | Not established | Unresolved |
| Does sidechain/LUFS/look-ahead match? | Not established | Unresolved |

REA sessions were opened for the installer and the VST3 DLL; both were closed after analysis.
