//! VST3 wrapper for the reverse-engineered magic.RIDE.
//!
//! Modelled on the `vst3-rs` gain example (MIT/Apache-2.0), with the plugin's
//! recovered 15-parameter APVTS layout and the reconstructed DSP core.
//! Build on macOS with: cargo build --release -p magic-ride-vst3

#![allow(non_upper_case_globals, non_camel_case_types, non_snake_case)]

use magic_ride_core::*;
use std::cell::Cell;
use std::ffi::{c_char, c_void, CString};
use std::slice;
use vst3::{uid, Class, ComRef, ComWrapper, Steinberg::Vst::*, Steinberg::*};

fn copy_cstring(src: &str, dst: &mut [c_char]) {
    let bytes = CString::new(src).unwrap_or_default();
    let b = bytes.as_bytes_with_nul();
    for (s, d) in b.iter().zip(dst.iter_mut()) {
        *d = *s as c_char;
    }
    if b.len() > dst.len() {
        if let Some(last) = dst.last_mut() {
            *last = 0;
        }
    }
}
fn copy_wstring(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    for (s, d) in src.encode_utf16().zip(dst.iter_mut()) {
        *d = s as TChar;
        n += 1;
    }
    if n < dst.len() {
        dst[n] = 0;
    } else if let Some(last) = dst.last_mut() {
        *last = 0;
    }
}

struct MagicProcessor {
    params: Cell<Params>,
    rider: Cell<Rider>,
}

impl Class for MagicProcessor {
    type Interfaces = (IComponent, IAudioProcessor, IProcessContextRequirements);
}
impl MagicProcessor {
    const CID: TUID = uid(0x6D616769, 0x52564944, 0x454D4147, 0x31333031);
    fn new() -> Self {
        Self {
            params: Cell::new(Params::default()),
            rider: Cell::new(Rider::new(48000.0)),
        }
    }
}

impl IPluginBaseTrait for MagicProcessor {
    unsafe fn initialize(&self, _context: *mut FUnknown) -> tresult { kResultOk }
    unsafe fn terminate(&self) -> tresult { kResultOk }
}

impl IComponentTrait for MagicProcessor {
    unsafe fn getControllerClassId(&self, class_id: *mut TUID) -> tresult {
        *class_id = MagicController::CID;
        kResultOk
    }
    unsafe fn setIoMode(&self, _mode: IoMode) -> tresult { kResultOk }
    unsafe fn getBusCount(&self, media_type: MediaType, dir: BusDirection) -> i32 {
        match media_type as BusDirections {
            MediaTypes_::kAudio => match dir as BusDirections {
                BusDirections_::kInput | BusDirections_::kOutput => 1,
                _ => 0,
            },
            _ => 0,
        }
    }
    unsafe fn getBusInfo(&self, media_type: MediaType, dir: BusDirection, index: i32, bus: *mut BusInfo) -> tresult {
        if media_type as i32 != MediaTypes_::kAudio as i32 || index != 0 {
            return kInvalidArgument;
        }
        let b = &mut *bus;
        b.mediaType = MediaTypes_::kAudio as MediaType;
        b.direction = dir;
        b.channelCount = 2;
        copy_wstring(if dir as i32 == BusDirections_::kInput as i32 { "Input" } else { "Output" }, &mut b.name);
        b.busType = BusTypes_::kMain as BusType;
        b.flags = BusInfo_::BusFlags_::kDefaultActive as u32;
        kResultOk
    }
    unsafe fn getRoutingInfo(&self, _a: *mut RoutingInfo, _b: *mut RoutingInfo) -> tresult { kNotImplemented }
    unsafe fn activateBus(&self, _t: MediaType, _d: BusDirection, _i: i32, _s: TBool) -> tresult { kResultOk }
    unsafe fn setActive(&self, _state: TBool) -> tresult { kResultOk }
    unsafe fn setState(&self, _state: *mut IBStream) -> tresult { kResultOk }
    unsafe fn getState(&self, _state: *mut IBStream) -> tresult { kResultOk }
}

impl IAudioProcessorTrait for MagicProcessor {
    unsafe fn setBusArrangements(&self, inputs: *mut SpeakerArrangement, n_in: i32, outputs: *mut SpeakerArrangement, n_out: i32) -> tresult {
        if n_in != 1 || n_out != 1 { return kResultFalse; }
        if *inputs != SpeakerArr::kStereo || *outputs != SpeakerArr::kStereo { return kResultFalse; }
        kResultTrue
    }
    unsafe fn getBusArrangement(&self, dir: BusDirection, index: i32, arr: *mut SpeakerArrangement) -> tresult {
        if index != 0 { return kInvalidArgument; }
        if dir as i32 == BusDirections_::kInput as i32 || dir as i32 == BusDirections_::kOutput as i32 {
            *arr = SpeakerArr::kStereo;
            kResultOk
        } else { kInvalidArgument }
    }
    unsafe fn canProcessSampleSize(&self, size: i32) -> tresult {
        match size as SymbolicSampleSizes {
            SymbolicSampleSizes_::kSample32 => kResultOk as i32,
            _ => kNotImplemented as i32,
        }
    }
    unsafe fn getLatencySamples(&self) -> u32 { 0 }
    unsafe fn setupProcessing(&self, _setup: *mut ProcessSetup) -> tresult { kResultOk }
    unsafe fn setProcessing(&self, _state: TBool) -> tresult { kResultOk }

    unsafe fn process(&self, data: *mut ProcessData) -> tresult {
        let pd = &*data;
        let mut params = self.params.get();

        if let Some(changes) = ComRef::from_raw(pd.inputParameterChanges) {
            let count = changes.getParameterCount();
            for i in 0..count {
                if let Some(q) = ComRef::from_raw(changes.getParameterData(i)) {
                    let id = q.getParameterId() as usize;
                    let n = q.getPointCount();
                    if n > 0 && id < PARAM_IDS.len() {
                        let mut off = 0i32;
                        let mut value = 0.0f64;
                        if q.getPoint(n - 1, &mut off, &mut value) == kResultTrue {
                            let mut plain = params.plain();
                            plain[id] = Params::denormalize(id, value as f32);
                            params = Params::from_plain(&plain);
                        }
                    }
                }
            }
        }

        let n = pd.numSamples as usize;
        if pd.numInputs != 1 || pd.numOutputs != 1 || n == 0 { return kResultOk; }
        let ins = slice::from_raw_parts(pd.inputs, 1);
        let outs = slice::from_raw_parts(pd.outputs, 1);
        if ins[0].numChannels < 2 || outs[0].numChannels < 2 { return kResultOk; }
        let ib = slice::from_raw_parts(ins[0].__field0.channelBuffers32, 2);
        let ob = slice::from_raw_parts(outs[0].__field0.channelBuffers32, 2);
        let il = slice::from_raw_parts(ib[0], n);
        let ir = slice::from_raw_parts(ib[1], n);
        let ol = slice::from_raw_parts_mut(ob[0], n);
        let or = slice::from_raw_parts_mut(ob[1], n);
        ol.copy_from_slice(il);
        or.copy_from_slice(ir);
        self.params.set(params);
        let mut rider = self.rider.take();
        magic_ride_core::dsp::process_stereo(ol, or, &mut rider, &params);
        self.rider.set(rider);
        kResultOk
    }
    unsafe fn getTailSamples(&self) -> u32 { 0 }
}

impl IProcessContextRequirementsTrait for MagicProcessor {
    unsafe fn getProcessContextRequirements(&self) -> u32 { 0 }
}

struct MagicController {
    values: Cell<[f64; 15]>,
}
impl Class for MagicController {
    type Interfaces = (IEditController,);
}
impl MagicController {
    const CID: TUID = uid(0x434F4E54, 0x524F4C4C, 0x45524D41, 0x47495244);
    fn new() -> Self {
        let mut values = [0.0f64; 15];
        let p = Params::default().plain();
        for (i, v) in p.iter().enumerate() {
            values[i] = Params::normalize(i, *v) as f64;
        }
        Self { values: Cell::new(values) }
    }
}
impl IPluginBaseTrait for MagicController {
    unsafe fn initialize(&self, _c: *mut FUnknown) -> tresult { kResultOk }
    unsafe fn terminate(&self) -> tresult { kResultOk }
}
impl IEditControllerTrait for MagicController {
    unsafe fn setComponentState(&self, _s: *mut IBStream) -> tresult { kNotImplemented }
    unsafe fn setState(&self, _s: *mut IBStream) -> tresult { kResultOk }
    unsafe fn getState(&self, _s: *mut IBStream) -> tresult { kResultOk }
    unsafe fn getParameterCount(&self) -> i32 { PARAM_IDS.len() as i32 }
    unsafe fn getParameterInfo(&self, index: i32, info: *mut ParameterInfo) -> tresult {
        let i = index as usize;
        if i >= PARAM_IDS.len() { return kInvalidArgument; }
        let out = &mut *info;
        out.id = i as u32;
        copy_wstring(PARAM_LABELS[i], &mut out.title);
        copy_wstring(PARAM_IDS[i], &mut out.shortTitle);
        copy_wstring(PARAM_UNITS[i], &mut out.units);
        out.stepCount = 0;
        out.defaultNormalizedValue = Params::normalize(i, PARAM_RANGES[i].2) as f64;
        out.unitId = 0;
        out.flags = ParameterInfo_::ParameterFlags_::kCanAutomate as i32;
        kResultOk
    }
    unsafe fn getParamStringByValue(&self, id: u32, norm: f64, string: *mut String128) -> tresult {
        let i = id as usize;
        if i >= PARAM_IDS.len() { return kInvalidArgument; }
        let plain = Params::denormalize(i, norm as f32);
        let unit = PARAM_UNITS[i];
        let s = if unit.is_empty() { format!("{:.2}", plain) } else { format!("{:.2} {}", plain, unit) };
        copy_wstring(&s, &mut *string);
        kResultOk
    }
    unsafe fn getParamValueByString(&self, id: u32, string: *mut TChar, value: *mut f64) -> tresult {
        let i = id as usize;
        if i >= PARAM_IDS.len() { return kInvalidArgument; }
        let mut n = 0usize;
        while *string.add(n) != 0 { n += 1; }
        let s = match String::from_utf16(slice::from_raw_parts(string as *const u16, n)) {
            Ok(s) => s,
            Err(_) => return kInvalidArgument,
        };
        match s.trim().parse::<f64>() {
            Ok(v) => {
                let (min, max, _) = PARAM_RANGES[i];
                *value = Params::normalize(i, v as f32) as f64;
                let _ = min; let _ = max;
                kResultOk
            }
            Err(_) => kInvalidArgument,
        }
    }
    unsafe fn normalizedParamToPlain(&self, id: u32, v: f64) -> f64 {
        Params::denormalize(id as usize, v as f32) as f64
    }
    unsafe fn plainParamToNormalized(&self, id: u32, v: f64) -> f64 {
        Params::normalize(id as usize, v as f32) as f64
    }
    unsafe fn getParamNormalized(&self, id: u32) -> f64 {
        *self.values.get().get(id as usize).unwrap_or(&0.0)
    }
    unsafe fn setParamNormalized(&self, id: u32, value: f64) -> tresult {
        let mut v = self.values.get();
        if id as usize >= v.len() { return kInvalidArgument; }
        v[id as usize] = value.clamp(0.0, 1.0);
        self.values.set(v);
        kResultOk
    }
    unsafe fn setComponentHandler(&self, _h: *mut IComponentHandler) -> tresult { kResultOk }
    unsafe fn createView(&self, _n: *const c_char) -> *mut IPlugView { std::ptr::null_mut() }
}

struct Factory;
impl Class for Factory { type Interfaces = (IPluginFactory,); }
impl IPluginFactoryTrait for Factory {
    unsafe fn getFactoryInfo(&self, info: *mut PFactoryInfo) -> tresult {
        let i = &mut *info;
        copy_cstring(VENDOR, &mut i.vendor);
        copy_cstring("https://mbmaudio.com", &mut i.url);
        copy_cstring("", &mut i.email);
        i.flags = PFactoryInfo_::FactoryFlags_::kUnicode as int32;
        kResultOk
    }
    unsafe fn countClasses(&self) -> i32 { 2 }
    unsafe fn getClassInfo(&self, index: i32, info: *mut PClassInfo) -> tresult {
        let i = &mut *info;
        match index {
            0 => {
                i.cid = MagicProcessor::CID;
                i.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
                copy_cstring("Audio Module Class", &mut i.category);
            }
            1 => {
                i.cid = MagicController::CID;
                i.cardinality = PClassInfo_::ClassCardinality_::kManyInstances as int32;
                copy_cstring("Component Controller Class", &mut i.category);
            }
            _ => return kInvalidArgument,
        }
        copy_cstring("magic.RIDE", &mut i.name);
        kResultOk
    }
    unsafe fn createInstance(&self, cid: FIDString, iid: FIDString, obj: *mut *mut c_void) -> tresult {
        let instance = match *(cid as *const TUID) {
            MagicProcessor::CID => Some(ComWrapper::new(MagicProcessor::new()).to_com_ptr::<FUnknown>().unwrap()),
            MagicController::CID => Some(ComWrapper::new(MagicController::new()).to_com_ptr::<FUnknown>().unwrap()),
            _ => None,
        };
        if let Some(instance) = instance {
            let p = instance.as_ptr();
            ((*(*p).vtbl).queryInterface)(p, iid as *mut TUID, obj)
        } else { kInvalidArgument }
    }
}

#[cfg(target_os = "macos")]
#[no_mangle]
extern "system" fn bundleEntry(_bundle_ref: *mut c_void) -> bool { true }
#[cfg(target_os = "macos")]
#[no_mangle]
extern "system" fn bundleExit() -> bool { true }
#[cfg(target_os = "windows")]
#[no_mangle]
extern "system" fn InitDll() -> bool { true }
#[cfg(target_os = "windows")]
#[no_mangle]
extern "system" fn ExitDll() -> bool { true }

#[no_mangle]
extern "system" fn GetPluginFactory() -> *mut IPluginFactory {
    ComWrapper::new(Factory).to_com_ptr::<IPluginFactory>().unwrap().into_raw()
}
