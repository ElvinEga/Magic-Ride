//! User preset reader/writer matching magic.RIDE 1.3.1's XML layout.
//!
//! The original writes user presets as `*.xml` inside a "User Presets" folder.
//! The element/attribute keys were recovered as ASCII strings in the plugin
//! (evidence E-USER-PRESET-KEYS):
//!   name, UserPreset, speed, targetLevel, attackMs, range, holdMs,
//!   releaseMs, smartSilence, naturalMode, breathReduction, useLufs,
//!   noiseFloor, transientPreservation, outputTrim, lookAheadMode,
//!   boostRange, rangeLocked, cutRange

use crate::params::Params;

/// Minimal attribute lookup: finds `key="value"` anywhere in `xml`.
fn attr(xml: &str, key: &str) -> Option<f32> {
    // Written format: <PARAM name="speed" value="6"/>
    let pat = format!("name=\"{}\"", key);
    if let Some(i) = xml.find(&pat) {
        let rest = &xml[i + pat.len()..];
        if let Some(vpos) = rest.find("value=\"") {
            let start = vpos + 7;
            if let Some(q) = rest[start..].find('"') {
                if let Ok(v) = rest[start..start + q].trim().parse::<f32>() {
                    return Some(v);
                }
            }
        }
    }
    // Also accept key="value" and <key>value</key>
    let pat2 = format!("{}\"", key);
    let mut pos = 0;
    while let Some(i) = xml[pos..].find(&pat2) {
        let start = pos + i + pat2.len();
        let rest = &xml[start..];
        if let Some(q) = rest.find('"') {
            if let Ok(v) = rest[..q].trim().parse::<f32>() {
                return Some(v);
            }
        }
        pos = start;
    }
    let open = format!("<{}>", key);
    if let Some(i) = xml.find(&open) {
        let start = i + open.len();
        let close = format!("</{}>", key);
        if let Some(j) = xml[start..].find(&close) {
            return xml[start..start + j].trim().parse::<f32>().ok();
        }
    }
    None
}

fn attr_bool(xml: &str, key: &str) -> Option<bool> {
    attr(xml, key).map(|v| v >= 0.5)
}

fn name(xml: &str) -> Option<String> {
    for key in ["name=", "presetName="] {
        if let Some(i) = xml.find(key) {
            let start = i + key.len();
            if let Some(q1) = xml[start..].find('"') {
                let a = start + q1 + 1;
                if let Some(q2) = xml[a..].find('"') {
                    return Some(xml[a..a + q2].to_string());
                }
            }
        }
    }
    None
}

pub struct UserPreset {
    pub name: String,
    pub params: Params,
}

pub fn parse_user_preset(xml: &str) -> Result<UserPreset, &'static str> {
    let mut p = Params::default();
    macro_rules! get {
        ($key:literal, $field:ident) => {
            if let Some(v) = attr(xml, $key) { p.$field = v; }
        };
    }
    get!("speed", speed);
    get!("targetLevel", target_level);
    get!("attackMs", attack_ms);
    get!("range", range);
    get!("holdMs", hold_ms);
    get!("releaseMs", release_ms);
    get!("noiseFloor", noise_floor);
    get!("transientPreservation", transient_preservation);
    get!("outputTrim", output_trim);
    get!("boostRange", boost_range);
    get!("cutRange", cut_range);
    get!("breathReduction", breath_reduction);
    if let Some(v) = attr_bool(xml, "smartSilence") { p.smart_silence = v; }
    if let Some(v) = attr_bool(xml, "naturalMode") { p.natural_mode = v; }
    if let Some(v) = attr_bool(xml, "useLufs") { p.use_lufs = v; }
    // `lookAheadMode` and `rangeLocked` are UI/state flags; accepted but not
    // used by the audio path in this reconstruction.
    let name = name(xml).unwrap_or_else(|| "User".to_string());
    Ok(UserPreset { name, params: p })
}

pub fn write_user_preset(preset: &UserPreset) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <UserPreset name=\"{}\">\n  \
           <PARAM name=\"speed\" value=\"{}\"/>\n  \
           <PARAM name=\"targetLevel\" value=\"{}\"/>\n  \
           <PARAM name=\"range\" value=\"{}\"/>\n  \
           <PARAM name=\"attackMs\" value=\"{}\"/>\n  \
           <PARAM name=\"holdMs\" value=\"{}\"/>\n  \
           <PARAM name=\"releaseMs\" value=\"{}\"/>\n  \
           <PARAM name=\"noiseFloor\" value=\"{}\"/>\n  \
           <PARAM name=\"transientPreservation\" value=\"{}\"/>\n  \
           <PARAM name=\"outputTrim\" value=\"{}\"/>\n  \
           <PARAM name=\"boostRange\" value=\"{}\"/>\n  \
           <PARAM name=\"cutRange\" value=\"{}\"/>\n  \
           <PARAM name=\"breathReduction\" value=\"{}\"/>\n  \
           <PARAM name=\"smartSilence\" value=\"{}\"/>\n  \
           <PARAM name=\"naturalMode\" value=\"{}\"/>\n  \
           <PARAM name=\"useLufs\" value=\"{}\"/>\n\
         </UserPreset>\n",
        preset.name.replace('"', "&quot;"),
        preset.params.speed,
        preset.params.target_level,
        preset.params.range,
        preset.params.attack_ms,
        preset.params.hold_ms,
        preset.params.release_ms,
        preset.params.noise_floor,
        preset.params.transient_preservation,
        preset.params.output_trim,
        preset.params.boost_range,
        preset.params.cut_range,
        preset.params.breath_reduction,
        if preset.params.smart_silence { 1 } else { 0 },
        if preset.params.natural_mode { 1 } else { 0 },
        if preset.params.use_lufs { 1 } else { 0 },
    )
}
