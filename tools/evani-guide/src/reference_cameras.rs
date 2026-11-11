//! Per-asset reference cameras and studios in `catalog/procedural/references.json`
//! (each asset's optional `"camera"` and `"studio"`, written by
//! tools/procedural/gen_library.py from the asset's `realism.camera` and
//! `realism.studio` in library.json).
//!
//! The Evani.commons server shoots an object-judged item's view 0 from this
//! camera (missing values take its default: yaw 0, pitch 20, distance 1, sun
//! 135/40) and draws its hidden views round it; Asset Studio's server views
//! (tools/studio/server-views.mjs) do the same. Evani.App's seed script refuses
//! a value outside these ranges, so `check` refuses it first.

use serde_json::Value;

/// `(key, min, max)`: the values a camera may set and their allowed ranges
/// (Evani.App's commons-seed-evani-dev.mjs, 2026-11-11; since the per-object
/// studio, also `cam_aim_y`, the aim above the centre in bounds radii, and
/// `cam_lens_deg`, the vertical lens).
pub const CAMERA_RANGES: [(&str, f64, f64); 7] = [
    ("cam_yaw_deg", -360.0, 360.0),
    ("cam_pitch_deg", -10.0, 85.0),
    ("cam_dist_scale", 0.3, 3.0),
    ("sun_yaw_deg", -360.0, 360.0),
    ("sun_pitch_deg", 5.0, 89.0),
    ("cam_aim_y", -1.0, 1.0),
    ("cam_lens_deg", 10.0, 90.0),
];

/// `(key, min, max)`: an object's studio, as its reference picture shows it
/// (8-bit sRGB, measured with tools/studio/studio-measure.py): the backdrop's
/// grey, the lit floor's grey in front of the object, and their red minus
/// blue. commons-render's grey studio sweep draws them
/// (`kit.proc_field.studio_backdrop`, `studio_floor`, `studio_warmth`).
pub const STUDIO_RANGES: [(&str, f64, f64); 3] = [("backdrop_grey", 40.0, 230.0), ("floor_grey", 40.0, 230.0), ("warmth", -40.0, 40.0)];

/// Problems with one asset's `studio` value (empty when it is sound): an
/// object of known keys in range, with both greys (a studio sets the two).
pub fn studio_problems(asset: &str, st: &Value) -> Vec<String> {
    let mut out = ranged(asset, "studio", st, &STUDIO_RANGES);
    if let Some(obj) = st.as_object() {
        for need in ["backdrop_grey", "floor_grey"] {
            if !obj.is_empty() && !obj.contains_key(need) {
                out.push(format!("references.json `{asset}`.studio has no `{need}` (a studio sets both greys)"));
            }
        }
    }
    out
}

/// Problems with an object of numbers held to `ranges`.
fn ranged(asset: &str, what: &str, v: &Value, ranges: &[(&str, f64, f64)]) -> Vec<String> {
    let Some(obj) = v.as_object() else {
        return vec![format!("references.json `{asset}`.{what} is not an object")];
    };
    let mut out = Vec::new();
    if obj.is_empty() {
        out.push(format!("references.json `{asset}`.{what} is empty (leave it out for the server's default)"));
    }
    for (k, v) in obj {
        let Some(&(_, lo, hi)) = ranges.iter().find(|(n, _, _)| n == k) else {
            let names: Vec<&str> = ranges.iter().map(|r| r.0).collect();
            out.push(format!("references.json `{asset}`.{what} has unknown key `{k}` (allowed: {})", names.join(", ")));
            continue;
        };
        match v.as_f64() {
            Some(x) if x.is_finite() && (lo..=hi).contains(&x) => {}
            Some(x) => out.push(format!("references.json `{asset}`.{what}.{k} = {x} is outside {lo}..{hi}")),
            None => out.push(format!("references.json `{asset}`.{what}.{k} is not a number")),
        }
    }
    out
}

/// Problems with one asset's `camera` value (empty when it is sound).
pub fn camera_problems(asset: &str, cam: &Value) -> Vec<String> {
    ranged(asset, "camera", cam, &CAMERA_RANGES)
}

/// Every camera and studio problem in the text of references.json.
pub fn problems(text: &str) -> Vec<String> {
    let v: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => return vec![format!("catalog/procedural/references.json does not parse: {e}")],
    };
    let Some(assets) = v.get("assets").and_then(Value::as_object) else {
        return vec!["catalog/procedural/references.json has no `assets` object".into()];
    };
    let mut out: Vec<String> = assets.iter().filter_map(|(k, a)| a.get("camera").map(|c| camera_problems(k, c))).flatten().collect();
    out.extend(assets.iter().filter_map(|(k, a)| a.get("studio").map(|s| studio_problems(k, s))).flatten());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn refs(cam: Value) -> String {
        json!({ "assets": { "rocks-pebbles": { "reference": "proc-refs/rocks-pebbles.jpg", "camera": cam } } }).to_string()
    }

    #[test]
    fn no_camera_and_a_full_valid_camera_are_clean() {
        assert!(problems(&json!({ "assets": { "a": { "reference": "x.jpg" } } }).to_string()).is_empty());
        assert!(problems(&refs(json!({ "cam_yaw_deg": 0, "cam_pitch_deg": 14, "cam_dist_scale": 0.55 }))).is_empty());
        assert!(problems(&refs(json!({ "cam_yaw_deg": -360, "cam_pitch_deg": 85, "cam_dist_scale": 3, "sun_yaw_deg": 360, "sun_pitch_deg": 5 }))).is_empty());
        assert!(problems(&refs(json!({ "cam_pitch_deg": -10, "cam_dist_scale": 0.3, "sun_pitch_deg": 89 }))).is_empty());
    }

    #[test]
    fn out_of_range_values_are_refused() {
        for (k, bad) in [("cam_yaw_deg", 360.5), ("cam_yaw_deg", -361.0), ("cam_pitch_deg", -10.1), ("cam_pitch_deg", 86.0), ("cam_dist_scale", 0.29), ("cam_dist_scale", 3.01), ("sun_yaw_deg", 400.0), ("sun_pitch_deg", 4.9), ("sun_pitch_deg", 89.5)] {
            let p = problems(&refs(json!({ k: bad })));
            assert_eq!(p.len(), 1, "{k} = {bad}: {p:?}");
            assert!(p[0].contains("outside"), "{p:?}");
        }
    }

    #[test]
    fn wrong_shapes_are_refused() {
        assert!(problems(&refs(json!({ "cam_fov": 30 })))[0].contains("unknown key"));
        assert!(problems(&refs(json!({ "cam_pitch_deg": "14" })))[0].contains("not a number"));
        assert!(problems(&refs(json!([0, 14])))[0].contains("not an object"));
        assert!(problems(&refs(json!({})))[0].contains("empty"));
        assert!(!problems("{").is_empty());
    }

    #[test]
    fn the_new_camera_keys_are_held_to_their_ranges() {
        assert!(problems(&refs(json!({ "cam_aim_y": -1, "cam_lens_deg": 10 }))).is_empty());
        assert!(problems(&refs(json!({ "cam_aim_y": 1, "cam_lens_deg": 90 }))).is_empty());
        for (k, bad) in [("cam_aim_y", 1.01), ("cam_aim_y", -1.5), ("cam_lens_deg", 9.9), ("cam_lens_deg", 91.0)] {
            assert!(problems(&refs(json!({ k: bad })))[0].contains("outside"), "{k} = {bad}");
        }
    }

    fn studio(st: Value) -> String {
        json!({ "assets": { "trees-oak-trees": { "reference": "proc-refs/trees-oak-trees.jpg", "studio": st } } }).to_string()
    }

    #[test]
    fn a_studio_is_two_greys_and_an_optional_warmth_in_range() {
        assert!(problems(&studio(json!({ "backdrop_grey": 153.5, "floor_grey": 149 }))).is_empty());
        assert!(problems(&studio(json!({ "backdrop_grey": 40, "floor_grey": 230, "warmth": -40 }))).is_empty());
        assert!(problems(&studio(json!({ "backdrop_grey": 230, "floor_grey": 40, "warmth": 40 }))).is_empty());
        for (k, bad) in [("backdrop_grey", 39.0), ("backdrop_grey", 231.0), ("floor_grey", 255.0), ("warmth", 40.5), ("warmth", -41.0)] {
            let mut st = json!({ "backdrop_grey": 150, "floor_grey": 150 });
            st[k] = json!(bad);
            let p = problems(&studio(st));
            assert_eq!(p.len(), 1, "{k} = {bad}: {p:?}");
            assert!(p[0].contains("outside"), "{p:?}");
        }
        assert!(problems(&studio(json!({ "backdrop_grey": 150 })))[0].contains("no `floor_grey`"));
        assert!(problems(&studio(json!({ "backdrop_grey": 150, "floor_grey": 150, "key": 3 })))[0].contains("unknown key"));
        assert!(problems(&studio(json!({ "backdrop_grey": "150", "floor_grey": 150 })))[0].contains("not a number"));
        assert!(problems(&studio(json!("grey")))[0].contains("not an object"));
        assert!(problems(&studio(json!({})))[0].contains("empty"));
    }

    #[test]
    fn the_catalog_file_is_sound_and_pebbles_holds_its_camera() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/procedural/references.json");
        let text = std::fs::read_to_string(path).expect("references.json");
        assert_eq!(problems(&text), Vec::<String>::new());
        let v: Value = serde_json::from_str(&text).expect("json");
        assert_eq!(v["assets"]["rocks-pebbles"]["camera"], json!({ "cam_yaw_deg": 0, "cam_pitch_deg": 14, "cam_dist_scale": 0.57, "cam_aim_y": -0.09 }));
        assert_eq!(v["assets"]["rocks-pebbles"]["studio"], json!({ "backdrop_grey": 134.5, "floor_grey": 142.5, "warmth": 1.0 }));
        let studios = v["assets"].as_object().map_or(0, |a| a.values().filter(|x| x.get("studio").is_some()).count());
        assert_eq!(studios, 18, "every studio-object reference with a picture holds its studio");
    }
}
