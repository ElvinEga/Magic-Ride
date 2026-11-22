//! Minimal but functional CLAP wrapper for the reconstructed magic.RIDE.
//! Build on macOS with: cargo build --release -p magic-ride-clap

use clap_sys::entry::clap_plugin_entry as ClapEntry;
use clap_sys::ext::audio_ports::{
    clap_audio_port_info, clap_plugin_audio_ports, CLAP_EXT_AUDIO_PORTS, CLAP_PORT_STEREO,
};
use clap_sys::ext::params::{
    clap_param_info, clap_plugin_params, CLAP_EXT_PARAMS, CLAP_PARAM_IS_AUTOMATABLE,
};
use clap_sys::ext::state::{clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::process::*;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use magic_ride_core::*;
use std::ffi::{c_char, c_void, CStr};
use std::ptr;

const fn cstr(s: &[u8]) -> &CStr {
    unsafe { CStr::from_bytes_with_nul_unchecked(s) }
}

struct SyncPtrs([*const c_char; 2]);
unsafe impl Sync for SyncPtrs {}
static FEATURES: SyncPtrs = SyncPtrs([cstr(b"audio-effect\0").as_ptr(), ptr::null()]);

static DESC: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: cstr(b"com.mbmaudio.magicride.re\0").as_ptr(),
    name: cstr(b"magic.RIDE\0").as_ptr(),
    vendor: cstr(b"MBM Audio (RE reconstruction)\0").as_ptr(),
    url: cstr(b"https://mbmaudio.com\0").as_ptr(),
    manual_url: ptr::null(),
    support_url: ptr::null(),
    version: cstr(b"1.3.1\0").as_ptr(),
    description: cstr(b"Vocal gain rider (reverse-engineered)\0").as_ptr(),
    features: FEATURES.0.as_ptr(),
};

#[repr(C)]
struct Plugin {
    clap: clap_plugin,
    params: Params,
    rider: Rider,
    sample_rate: f32,
}

impl Plugin {
    fn from_clap(p: *const clap_plugin) -> &'static mut Self {
        unsafe { &mut *((*p).plugin_data as *mut Plugin) }
    }
}

unsafe extern "C" fn init(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    if !p.is_null() && !(*p).plugin_data.is_null() {
        drop(Box::from_raw((*p).plugin_data as *mut Plugin));
    }
}
unsafe extern "C" fn activate(p: *const clap_plugin, sample_rate: f64, _min: u32, _max: u32) -> bool {
    let plug = Plugin::from_clap(p);
    plug.sample_rate = sample_rate as f32;
    plug.rider.set_sample_rate(sample_rate as f32);
    true
}
unsafe extern "C" fn deactivate(_p: *const clap_plugin) {}
unsafe extern "C" fn start_processing(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn stop_processing(_p: *const clap_plugin) {}
unsafe extern "C" fn reset(p: *const clap_plugin) {
    Plugin::from_clap(p).rider.reset();
}

unsafe extern "C" fn process(p: *const clap_plugin, data: *const clap_process) -> clap_process_status {
    let plug = Plugin::from_clap(p);
    let d = &*data;

    // Parameter events.
    if !d.in_events.is_null() {
        let list = &*d.in_events;
        if let (Some(size), Some(get)) = (list.size, list.get) {
            for i in 0..size(list as *const _) {
                let ev = get(list as *const _, i) as *const clap_sys::events::clap_event_param_value;
                if ev.is_null() {
                    continue;
                }
                let e = &*ev;
                if e.header.type_ == clap_sys::events::CLAP_EVENT_PARAM_VALUE {
                    let id = e.param_id as usize;
                    if id < PARAM_IDS.len() {
                        let mut plain = plug.params.plain();
                        plain[id] = Params::denormalize(id, e.value as f32);
                        plug.params = Params::from_plain(&plain);
                    }
                }
            }
        }
    }

    if d.audio_inputs_count < 1 || d.audio_outputs_count < 1 {
        return CLAP_PROCESS_CONTINUE;
    }
    let inp = &*d.audio_inputs;
    let out = &mut *d.audio_outputs;
    if inp.channel_count < 2 || out.channel_count < 2 || inp.data32.is_null() || out.data32.is_null() {
        return CLAP_PROCESS_CONTINUE;
    }
    let ib = std::slice::from_raw_parts(inp.data32 as *const *mut f32, 2);
    let ob = std::slice::from_raw_parts_mut(out.data32, 2);
    let l = std::slice::from_raw_parts(ib[0], d.frames_count as usize);
    let r = std::slice::from_raw_parts(ib[1], d.frames_count as usize);
    let ol = std::slice::from_raw_parts_mut(ob[0], d.frames_count as usize);
    let or = std::slice::from_raw_parts_mut(ob[1], d.frames_count as usize);
    ol.copy_from_slice(l);
    or.copy_from_slice(r);
    magic_ride_core::dsp::process_stereo(ol, or, &mut plug.rider, &plug.params);
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn get_extension(p: *const clap_plugin, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return ptr::null();
    }
    let ext = CStr::from_ptr(id);
    if ext == CLAP_EXT_PARAMS {
        &PARAMS_EXT as *const _ as *const c_void
    } else if ext == CLAP_EXT_AUDIO_PORTS {
        &PORTS_EXT as *const _ as *const c_void
    } else if ext == CLAP_EXT_STATE {
        &STATE_EXT as *const _ as *const c_void
    } else {
        let _ = p;
        ptr::null()
    }
}
unsafe extern "C" fn on_main_thread(_p: *const clap_plugin) {}

static PARAMS_EXT: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(params_get_info),
    get_value: Some(params_get_value),
    value_to_text: Some(params_value_to_text),
    text_to_value: Some(params_text_to_value),
    flush: Some(params_flush),
};

unsafe extern "C" fn params_count(_p: *const clap_plugin) -> u32 {
    PARAM_IDS.len() as u32
}
unsafe extern "C" fn params_get_info(
    _p: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    if index as usize >= PARAM_IDS.len() || info.is_null() {
        return false;
    }
    let i = index as usize;
    let out = &mut *info;
    out.id = i as u32;
    out.flags = CLAP_PARAM_IS_AUTOMATABLE;
    out.cookie = ptr::null_mut();
    let name = PARAM_LABELS[i].as_bytes();
    for (dst, src) in out.name.iter_mut().zip(name.iter()) {
        *dst = *src as c_char;
    }
    let unit = PARAM_UNITS[i].as_bytes();
    for (dst, src) in out.module.iter_mut().zip(unit.iter()) {
        *dst = *src as c_char;
    }
    out.min_value = PARAM_RANGES[i].0 as f64;
    out.max_value = PARAM_RANGES[i].1 as f64;
    out.default_value = PARAM_RANGES[i].2 as f64;
    true
}
unsafe extern "C" fn params_get_value(
    p: *const clap_plugin,
    id: clap_sys::id::clap_id,
    out: *mut f64,
) -> bool {
    let plug = Plugin::from_clap(p);
    if id as usize >= PARAM_IDS.len() || out.is_null() {
        return false;
    }
    let plain = plug.params.plain();
    *out = Params::normalize(id as usize, plain[id as usize]) as f64;
    true
}
unsafe extern "C" fn params_value_to_text(
    _p: *const clap_plugin,
    id: clap_sys::id::clap_id,
    value: f64,
    buf: *mut c_char,
    cap: u32,
) -> bool {
    if id as usize >= PARAM_IDS.len() || buf.is_null() || cap == 0 {
        return false;
    }
    let plain = Params::denormalize(id as usize, value as f32);
    let unit = PARAM_UNITS[id as usize];
    let s = if unit.is_empty() {
        format!("{:.2}", plain)
    } else {
        format!("{:.2} {}", plain, unit)
    };
    let bytes = s.as_bytes();
    let n = bytes.len().min(cap as usize - 1);
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), buf as *mut u8, n);
        *buf.add(n) = 0;
    }
    true
}
unsafe extern "C" fn params_text_to_value(
    _p: *const clap_plugin,
    id: clap_sys::id::clap_id,
    text: *const c_char,
    out: *mut f64,
) -> bool {
    if id as usize >= PARAM_IDS.len() || text.is_null() || out.is_null() {
        return false;
    }
    let s = match CStr::from_ptr(text).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let v: f32 = match s.trim().trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.' && c != '-') .parse() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let (min, max, _) = PARAM_RANGES[id as usize];
    *out = Params::normalize(id as usize, v.clamp(min, max)) as f64;
    true
}
unsafe extern "C" fn params_flush(
    _p: *const clap_plugin,
    _in_: *const clap_sys::events::clap_input_events,
    _out: *const clap_sys::events::clap_output_events,
) {
}

static PORTS_EXT: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(ports_count),
    get: Some(ports_get),
};
unsafe extern "C" fn ports_count(_p: *const clap_plugin, _is_input: bool) -> u32 {
    1
}
unsafe extern "C" fn ports_get(
    _p: *const clap_plugin,
    index: u32,
    is_input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    if index != 0 || info.is_null() {
        return false;
    }
    let out = &mut *info;
    out.id = 0;
    let name: &[u8] = if is_input { b"Input\0" } else { b"Output\0" };
    for (dst, src) in out.name.iter_mut().zip(name.iter()) {
        *dst = *src as c_char;
    }
    out.flags = clap_sys::ext::audio_ports::CLAP_AUDIO_PORT_IS_MAIN;
    out.channel_count = 2;
    out.port_type = CLAP_PORT_STEREO.as_ptr();
    out.in_place_pair = 0;
    true
}

static STATE_EXT: clap_plugin_state = clap_plugin_state {
    save: Some(state_save),
    load: Some(state_load),
};
unsafe extern "C" fn state_save(p: *const clap_plugin, stream: *const clap_ostream) -> bool {
    let plug = Plugin::from_clap(p);
    let s = preset_xml::write_user_preset(&preset_xml::UserPreset {
        name: "Current State".to_string(),
        params: plug.params,
    });
    let f = &*stream;
    match f.write {
        Some(write) => write(stream as *const _, s.as_ptr() as *const c_void, s.len() as u64) >= 0,
        None => false,
    }
}
unsafe extern "C" fn state_load(p: *const clap_plugin, stream: *const clap_istream) -> bool {
    let plug = Plugin::from_clap(p);
    let f = &*stream;
    let Some(read) = f.read else { return false };
    let mut buf = vec![0u8; 8192];
    let n = read(stream as *const _, buf.as_mut_ptr() as *mut c_void, buf.len() as u64);
    if n <= 0 {
        return false;
    }
    let s = String::from_utf8_lossy(&buf[..n as usize]);
    match preset_xml::parse_user_preset(&s) {
        Ok(preset) => {
            plug.params = preset.params;
            plug.rider.reset();
            true
        }
        Err(_) => false,
    }
}

unsafe extern "C" fn create_plugin(
    _factory: *const clap_plugin_factory,
    _host: *const clap_sys::host::clap_host,
    _id: *const c_char,
) -> *const clap_plugin {
    let plug = Box::new(Plugin {
        clap: clap_plugin {
            desc: &DESC,
            plugin_data: ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start_processing),
            stop_processing: Some(stop_processing),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(on_main_thread),
        },
        params: Params::default(),
        rider: Rider::new(48000.0),
        sample_rate: 48000.0,
    });
    let ptr = Box::into_raw(plug);
    (*ptr).clap.plugin_data = ptr as *mut c_void;
    &(*ptr).clap as *const clap_plugin
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_count),
    get_plugin_descriptor: Some(factory_descriptor),
    create_plugin: Some(create_plugin),
};
unsafe extern "C" fn factory_count(_f: *const clap_plugin_factory) -> u32 {
    1
}
unsafe extern "C" fn factory_descriptor(
    _f: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    if index == 0 { &DESC } else { ptr::null() }
}

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}
unsafe extern "C" fn entry_deinit() {}
unsafe extern "C" fn entry_get_factory(id: *const c_char) -> *const c_void {
    if id.is_null() {
        return ptr::null();
    }
    if CStr::from_ptr(id) == CLAP_PLUGIN_FACTORY_ID {
        &FACTORY as *const _ as *const c_void
    } else {
        ptr::null()
    }
}

#[no_mangle]
pub static clap_entry: ClapEntry = ClapEntry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_get_factory),
};
