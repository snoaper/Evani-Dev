//! The layer BRDF WGSL validates on its own (naga), and its key formulas,
//! mirrored here line for line, behave as the papers say.

use crate::BRDF_WGSL;

#[test]
fn brdf_wgsl_validates() {
    let m = naga::front::wgsl::parse_str(BRDF_WGSL)
        .map_err(|e| e.emit_to_string(BRDF_WGSL))
        .unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&m)
    .unwrap();
    for f in [
        "ly_d_charlie",
        "ly_v_neubelt",
        "ly_sheen_env",
        "ly_coat_direct",
        "ly_aniso_dv",
        "ly_aniso_kajiya",
        "ly_aniso_bent",
        "ly_iridescence",
    ] {
        assert!(BRDF_WGSL.contains(&format!("fn {f}(")), "{f}");
    }
    assert!(!BRDF_WGSL.contains("@group"), "pure functions, no bindings");
}

const PI: f32 = std::f32::consts::PI;

fn d_charlie(rough: f32, ndh: f32) -> f32 {
    let r = rough.max(0.07);
    let inv = 1.0 / (r * r);
    let sin2 = (1.0 - ndh * ndh).max(0.0078125);
    (2.0 + inv) * sin2.powf(inv * 0.5) / (2.0 * PI)
}

/// The Charlie lobe peaks at grazing half vectors (the velvet rim) and
/// integrates to about 1 over the hemisphere of half vectors (D * N.H).
#[test]
fn charlie_is_a_grazing_lobe_and_normalised() {
    for rough in [0.3, 0.6, 1.0] {
        assert!(
            d_charlie(rough, 0.1) > d_charlie(rough, 0.95),
            "rough {rough}"
        );
        let n = 2000;
        let mut sum = 0.0;
        for i in 0..n {
            let t = (i as f32 + 0.5) / n as f32 * PI * 0.5;
            sum += d_charlie(rough, t.cos()) * t.cos() * t.sin() * 2.0 * PI * (PI * 0.5 / n as f32);
        }
        assert!((sum - 1.0).abs() < 0.08, "rough {rough}: {sum}");
    }
}

fn schlick1(f0: f32, c: f32) -> f32 {
    f0 + (1.0 - f0) * (1.0 - c).powi(5)
}

fn sensitivity(opd: f32, shift: [f32; 3]) -> [f32; 3] {
    let phase = 2.0 * PI * opd * 1.0e-9;
    let val = [5.4856e-13f32, 4.4201e-13, 5.2481e-13];
    let pos = [1.6810e+06f32, 1.7953e+06, 2.2084e+06];
    let vr = [4.3278e+09f32, 9.3046e+09, 6.6121e+09];
    let mut xyz = [0.0f32; 3];
    for c in 0..3 {
        xyz[c] = val[c]
            * (2.0 * PI * vr[c]).sqrt()
            * (pos[c] * phase + shift[c]).cos()
            * (-(phase * phase) * vr[c]).exp();
    }
    xyz[0] += 9.7470e-14
        * (2.0 * PI * 4.5282e+09f32).sqrt()
        * (2.2399e+06 * phase + shift[0]).cos()
        * (-4.5282e+09 * phase * phase).exp();
    let xyz = xyz.map(|x| x / 1.0685e-7);
    [
        3.2404542 * xyz[0] - 1.5371385 * xyz[1] - 0.4985314 * xyz[2],
        -0.969_266 * xyz[0] + 1.8760108 * xyz[1] + 0.0415560 * xyz[2],
        0.0556434 * xyz[0] - 0.2040259 * xyz[1] + 1.0572252 * xyz[2],
    ]
}

/// `ly_iridescence`, line for line, for one grey base reflectance.
fn iridescence(eta2: f32, cos1: f32, thickness: f32, f0: f32) -> [f32; 3] {
    let t = (thickness / 30.0).clamp(0.0, 1.0);
    let ior = 1.0 + (eta2 - 1.0) * t * t * (3.0 - 2.0 * t);
    let cos2sq = 1.0 - (1.0 / (ior * ior)) * (1.0 - cos1 * cos1);
    let cos2 = cos2sq.sqrt();
    let r0 = ((ior - 1.0) / (ior + 1.0)).powi(2);
    let r12 = schlick1(r0, cos1);
    let t121 = 1.0 - r12;
    let phi21 = PI - if ior < 1.0 { PI } else { 0.0 };
    let sf0 = f0.clamp(0.0, 0.9999).sqrt();
    let base_ior = (1.0 + sf0) / (1.0 - sf0);
    let r1 = ((base_ior - ior) / (base_ior + ior)).powi(2);
    let r23 = schlick1(r1, cos2);
    let phi23 = if base_ior < ior { PI } else { 0.0 };
    let opd = 2.0 * ior * thickness * cos2;
    let phi = phi21 + phi23;
    let r123 = (r12 * r23).clamp(1e-5, 0.9999);
    let rr = r123.sqrt();
    let rs = t121 * t121 * r23 / (1.0 - r123);
    let s1 = sensitivity(opd, [phi; 3]);
    let s2 = sensitivity(2.0 * opd, [2.0 * phi; 3]);
    let c1 = (rs - t121) * rr;
    let c2 = c1 * rr;
    [0, 1, 2].map(|c| (r12 + rs + c1 * 2.0 * s1[c] + c2 * 2.0 * s2[c]).max(0.0))
}

/// A film far thicker than light's coherence length reflects grey (the
/// interference terms die out, leaving the two interfaces' sum); a soap
/// film of a few hundred nanometres colours the reflection, and the
/// colour changes with the viewing angle.
#[test]
fn iridescence_colours_only_with_a_thin_film() {
    let spread = |x: [f32; 3]| {
        x.iter().cloned().fold(f32::MIN, f32::max) - x.iter().cloned().fold(f32::MAX, f32::min)
    };
    for c in [0.3f32, 0.7, 1.0] {
        let thick = iridescence(1.33, c, 5000.0, 0.04);
        assert!(spread(thick) < 0.01, "cos {c}: {thick:?}");
    }
    let a = iridescence(1.33, 1.0, 380.0, 0.04);
    let b = iridescence(1.33, 0.4, 380.0, 0.04);
    assert!(spread(a) > 0.01, "a film colours the reflection: {a:?}");
    assert!(
        a.iter().zip(&b).any(|(x, y)| (x - y).abs() > 0.01),
        "the colour shifts with angle: {a:?} {b:?}"
    );
    assert!(
        a.iter().all(|&x| x <= 1.0),
        "never brighter than a mirror: {a:?}"
    );
}
