
// ---- Ocean comparison rig (tools/nature/compare.mjs on branch ocean-compare) ----
// compare.mjs appends this test to a throwaway checkout's
// crates/evani-material-nature/src/sea/tests.rs (never to a working copy)
// and runs it once: with EVANI_CMP_VIEWS set to the shared views file
// (tools/nature/compare-views.json) and EVANI_SEA_DUMP to a folder, it writes
// the sea shader and `scenes.json` in the GPU harness's own format
// (tools/nature/sea-shots.mjs): one shot per view and moment, the uniform
// words of every quality level packed by the engine's own `uniforms`, from
// the Ocean item's catalogue preset plus the view's overrides. It uses the
// tests module's helpers (reg, frame, uniforms, Mirror); a checkout whose
// tests already hold `dump_compare_views` is used as it is.
#[test]
fn dump_compare_views() {
    let Ok(views) = std::env::var("EVANI_CMP_VIEWS") else { return };
    let Ok(dir) = std::env::var("EVANI_SEA_DUMP") else { return };
    let dir = std::path::Path::new(&dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("sea.wgsl"), SHADERS[FULL].source).unwrap();
    let cfg: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&views).unwrap()).unwrap();
    let lib: serde_json::Value = serde_json::from_str(include_str!("../../../../catalog/procedural/procedural.json")).unwrap();
    let ocean = lib["items"].as_array().unwrap().iter().find(|i| i["id"] == "ocean").unwrap();
    let (base, p) = reg();
    // A catalogue value onto the registry; false when the name is not a
    // registered sea or nature value (the studio's sky.* values are not).
    let put = |reg: &mut ParamRegistry, n: &str, v: &serde_json::Value| -> bool {
        let Some(id) = reg.id(n) else { return false };
        let val = match v {
            serde_json::Value::Array(a) => ParamValue::Color([0, 1, 2].map(|i| a[i].as_f64().unwrap() as f32)),
            x => ParamValue::F32(x.as_f64().unwrap() as f32),
        };
        reg.set(id, val).unwrap();
        true
    };
    let num = |v: &serde_json::Value| v.as_f64().unwrap() as f32;
    let moments: Vec<f64> = cfg["moments"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let size = [num(&cfg["size"][0]), num(&cfg["size"][1])];
    let mut out = Vec::new();
    let mut applied = serde_json::Map::new();
    for v in cfg["views"].as_array().unwrap() {
        let id = v["id"].as_str().unwrap();
        let mut r = base.clone();
        let mut used = serde_json::Map::new();
        for (n, x) in ocean["base"].as_object().unwrap() {
            if put(&mut r, n, x) {
                used.insert(n.clone(), x.clone());
            }
        }
        if let Some(pr) = v["ours"]["preset"].as_str() {
            let pr = ocean["presets"].as_array().unwrap().iter().find(|x| x["id"] == pr).unwrap_or_else(|| panic!("no Ocean preset {pr}"));
            for (n, x) in pr["params"].as_object().unwrap() {
                if put(&mut r, n, x) {
                    used.insert(n.clone(), x.clone());
                }
            }
        }
        if let Some(set) = v["ours"]["set"].as_object() {
            for (n, x) in set {
                assert!(put(&mut r, n, x), "view {id}: {n} is not a sea or nature value");
                used.insert(n.clone(), x.clone());
            }
        }
        applied.insert(id.to_string(), serde_json::Value::Object(used));
        let s = p.settings(&r);
        let cam = &v["camera"];
        let eye = [num(&cam["eye"][0]), num(&cam["eye"][1]), num(&cam["eye"][2])];
        let (yaw, pitch) = (num(&cam["yaw_deg"]).to_radians(), num(&cam["pitch_deg"]).to_radians());
        let d = [yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos()];
        let target = [eye[0] + d[0] * 60.0, eye[1] + d[1] * 60.0, eye[2] + d[2] * 60.0];
        let fov = num(&cam["fov_deg"]);
        let (el, az) = (num(&v["sun"]["elevation_deg"]).to_radians(), num(&v["sun"]["azimuth_deg"]).to_radians());
        let sun = norm([az.cos() * el.cos(), el.sin(), az.sin() * el.cos()]);
        let sun_col = v["ours"]["sun_col"].as_array().map(|a| [num(&a[0]), num(&a[1]), num(&a[2])]);
        let bed = v["ours"]["bed"].as_str().unwrap_or("deep");
        for (k, t0) in moments.iter().enumerate() {
            // The camera never inside a wave: the next moment (1.3 s steps)
            // where no level's sea stands within a metre of the eye, as the
            // storm harness does.
            let clear = |t: f64| (1..4).all(|l| Mirror::new(&uniforms(&s, &frame(l, eye, target, t))).height([eye[0], eye[2]], 0.05) < eye[1] - 1.0);
            let time = (0..20).map(|i| t0 + 1.3 * f64::from(i)).find(|t| clear(*t)).unwrap_or(*t0);
            let mut levels = Vec::new();
            for level in 0..4 {
                let mut fr = frame(level, eye, target, time);
                fr.camera.fov_y_deg = fov;
                fr.sun = sun;
                if let Some(c) = sun_col {
                    fr.sun_col = c;
                }
                fr.size = size;
                levels.push(uniforms(&s, &fr).to_vec());
            }
            out.push(serde_json::json!({"name": format!("{id}-m{k}"), "look": id, "bed": bed, "time": time, "levels": levels}));
        }
    }
    std::fs::write(dir.join("scenes.json"), serde_json::to_string(&serde_json::json!({"shots": out, "worst": [], "applied": applied})).unwrap()).unwrap();
}
