use magic_ride_core::*;

#[test]
fn factory_presets_are_complete_and_sane() {
    assert_eq!(FACTORY_PRESETS.len(), 48);
    for p in FACTORY_PRESETS {
        assert!(!p.name.is_empty());
        assert!(!p.category.is_empty());
        assert!(p.params.target_level >= -40.0 && p.params.target_level <= 0.0);
        assert!(p.params.boost_range > 0.0);
        assert!(p.params.cut_range > 0.0);
        assert!(p.params.noise_floor <= -30.0);
    }
}

#[test]
fn preset_xml_roundtrip() {
    let preset = preset_xml::UserPreset {
        name: "My Vocal".to_string(),
        params: FACTORY_PRESETS[3].params,
    };
    let xml = preset_xml::write_user_preset(&preset);
    let parsed = preset_xml::parse_user_preset(&xml).unwrap();
    assert_eq!(parsed.name, "My Vocal");
    assert_eq!(parsed.params.target_level, preset.params.target_level);
    assert_eq!(parsed.params.attack_ms, preset.params.attack_ms);
    assert_eq!(parsed.params.smart_silence, preset.params.smart_silence);
}

#[test]
fn rider_reduces_loud_input_and_passes_silence() {
    let p = Params {
        target_level: -18.0,
        range: 30.0,
        noise_floor: -100.0,
        natural_mode: false,
        ..Params::default()
    };
    let mut rider = Rider::new(48000.0);
    let mut l = vec![0.5f32; 4800];
    let mut r = vec![0.5f32; 4800];
    process_stereo(&mut l, &mut r, &mut rider, &p);
    let out: f32 = l.iter().skip(4000).map(|x| x * x).sum::<f32>() / 800.0;
    assert!(out.sqrt() < 0.5, "gain reduction expected, got {out}");

    let mut ls = vec![0.0f32; 100];
    let mut rs = vec![0.0f32; 100];
    process_stereo(&mut ls, &mut rs, &mut rider, &p);
    assert!(ls.iter().all(|x| *x == 0.0));
}
