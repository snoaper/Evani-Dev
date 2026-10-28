//! The capture sequence of a look check, as pure helpers (docs/LIVE_API.md
//! 15.3): which `capture.frame` params each shot uses, the stats of a
//! moment's two shots, the staged guard files of a new baseline, and the
//! hashes a bake approval is bound to. The bridge sends the commands.
//!
//! Capture version 1 ([`crate::guardset::CAPTURE_VERSION`]): for every
//! moment, in `moments.json` order, one shot with the look as it is and one
//! with bloom off (`values: {"fx.bloom.enabled": false}`), both at the
//! moments' tier and width; the page turns adaptive quality off for them.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::limits::{Limits, Moment, Moments};
use crate::png;
use crate::stats::{self, Stats};

/// The param the bloom-off shot turns off.
pub const BLOOM_SWITCH: &str = "fx.bloom.enabled";

/// The `capture.frame` params of one shot.
pub fn capture_params(moments: &Moments, m: &Moment, bloom_off: bool) -> Value {
    let mut p = m.capture.clone();
    if !p.is_object() {
        p = json!({});
    }
    p["width"] = json!(moments.width);
    p["tier"] = json!(moments.tier);
    if bloom_off {
        p["values"] = json!({ BLOOM_SWITCH: false });
    }
    p
}

/// One moment's two captures (PNG bytes as the page sent them).
#[derive(Debug, Clone)]
pub struct Shot {
    pub moment: String,
    pub on: Vec<u8>,
    pub off: Vec<u8>,
}

impl Shot {
    /// From two `capture.frame` results (`png_base64`).
    pub fn from_results(moment: &str, on: &Value, off: &Value) -> Result<Shot, String> {
        let png_of = |r: &Value, which: &str| {
            r["png_base64"]
                .as_str()
                .ok_or_else(|| format!("moment '{moment}': the {which} capture has no image"))
                .and_then(|s| png::base64_decode(s).map_err(|e| format!("moment '{moment}': {e}")))
        };
        Ok(Shot { moment: moment.into(), on: png_of(on, "bloom-on")?, off: png_of(off, "bloom-off")? })
    }

    pub fn stats(&self) -> Result<Stats, String> {
        let on = png::decode(&self.on).map_err(|e| format!("moment '{}': {e}", self.moment))?;
        let off = png::decode(&self.off).map_err(|e| format!("moment '{}': {e}", self.moment))?;
        if (on.w, on.h) != (off.w, off.h) {
            return Err(format!("moment '{}': the bloom-off capture is a different size", self.moment));
        }
        stats::compute(&on, Some(&off)).map_err(|e| format!("moment '{}': {e}", self.moment))
    }
}

/// Stats per moment.
pub fn stats_of(shots: &[Shot]) -> Result<BTreeMap<String, Stats>, String> {
    shots.iter().map(|s| Ok((s.moment.clone(), s.stats()?))).collect()
}

/// Hash of a capture set: `evani-look-captures/1`, then
/// `<moment> <sha256 on> <sha256 off>` per moment in order.
pub fn captures_hash(shots: &[Shot]) -> String {
    let mut text = String::from("evani-look-captures/1\n");
    for s in shots {
        text.push_str(&format!("{} {} {}\n", s.moment, crate::sha256_hex(&s.on), crate::sha256_hex(&s.off)));
    }
    crate::sha256_hex(text.as_bytes())
}

/// The guard files of a new baseline (relative to the guard folder).
pub fn staged_files(moments: &Moments, limits: &Limits, shots: &[Shot]) -> Vec<(String, Vec<u8>)> {
    let mut files = vec![
        ("moments.json".to_string(), moments.file_text().into_bytes()),
        ("limits.json".to_string(), limits.file_text().into_bytes()),
    ];
    for s in shots {
        files.push((format!("baseline/{}.png", s.moment), s.on.clone()));
        files.push((format!("baseline/{}.nobloom.png", s.moment), s.off.clone()));
    }
    files
}

/// The hash of a bake bundle's content: what would be written, not when
/// or from which transactions (`title`, `dry_run`, `from_tx`, `to_tx` and
/// `notes` are left out).
pub fn bundle_hash(bundle: &Value) -> String {
    let mut b = bundle.clone();
    if let Some(o) = b.as_object_mut() {
        for k in ["title", "dry_run", "from_tx", "to_tx", "notes"] {
            o.remove(k);
        }
    }
    crate::sha256_hex(canonical_json(&b).as_bytes())
}

/// JSON with object keys sorted (whatever map type serde_json was built
/// with), no spaces.
pub fn canonical_json(v: &Value) -> String {
    match v {
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let parts: Vec<String> =
                keys.iter().map(|k| format!("{}:{}", Value::String((*k).clone()), canonical_json(&o[*k]))).collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(canonical_json).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moments() -> Moments {
        Moments::parse(
            r#"{"format":"evani-look-moments/1","game":"sandbox","tier":"medium","width":160,
                "moments":[{"name":"start","capture":{"sim_time":1,"camera":"path"}},{"name":"drift","capture":{"moment":"drift"}}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn capture_params_add_tier_width_and_bloom_off() {
        let m = moments();
        assert_eq!(
            capture_params(&m, &m.moments[0], false),
            json!({"sim_time":1,"camera":"path","width":160,"tier":"medium"})
        );
        assert_eq!(
            capture_params(&m, &m.moments[1], true),
            json!({"moment":"drift","width":160,"tier":"medium","values":{"fx.bloom.enabled":false}})
        );
    }

    #[test]
    fn bundle_hash_ignores_when_but_not_what() {
        let a = json!({"title":"A","dry_run":false,"from_tx":1,"to_tx":4,"look_crate":"evani-look-basic",
                       "params":[{"name":"look.basic.saturation","value":1.2}],"notes":["x"]});
        let mut b = json!({"params":[{"value":1.2,"name":"look.basic.saturation"}],"look_crate":"evani-look-basic",
                       "title":"B","dry_run":true,"from_tx":7,"to_tx":9,"notes":[]});
        assert_eq!(bundle_hash(&a), bundle_hash(&b));
        b["params"][0]["value"] = json!(1.3);
        assert_ne!(bundle_hash(&a), bundle_hash(&b));
    }

    #[test]
    fn shots_give_stats_files_and_a_hash() {
        let img = |c: [u8; 3]| {
            let rgb = evani_live::img::Rgb { w: 8, h: 4, px: c.iter().copied().cycle().take(8 * 4 * 3).collect() };
            evani_live::img::encode_png(&rgb)
        };
        let on = json!({ "png_base64": evani_live::img::base64(&img([200, 200, 200])) });
        let off = json!({ "png_base64": evani_live::img::base64(&img([100, 100, 100])) });
        let s = Shot::from_results("start", &on, &off).unwrap();
        let st = s.stats().unwrap();
        assert_eq!(st.bloom, Some(1.0), "every pixel is brighter with bloom on");
        assert!(Shot::from_results("start", &json!({}), &off).unwrap_err().contains("bloom-on"));
        let m = moments();
        let shots = vec![s.clone(), Shot { moment: "drift".into(), ..s.clone() }];
        let base = stats_of(&shots).unwrap();
        let limits = Limits::generate("evani-look-basic", base, None);
        let files = staged_files(&m, &limits, &shots);
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            ["moments.json", "limits.json", "baseline/start.png", "baseline/start.nobloom.png", "baseline/drift.png", "baseline/drift.nobloom.png"]
        );
        assert!(crate::guardset::hash_files("evani-look-basic", &files).is_ok());
        let h = captures_hash(&shots);
        let mut other = shots.clone();
        other[1].off = img([90, 90, 90]);
        assert_ne!(h, captures_hash(&other));
    }
}
