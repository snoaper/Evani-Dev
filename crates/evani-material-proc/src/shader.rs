//! The kit-material WGSL, composed from its parts with the module's
//! material slots filled in: the surfaces (`shaders/material.wgsl`), the sky
//! (`shaders/sky.wgsl`, feature `sky` only) and the entry
//! (`shaders/entry.wgsl`).

/// The surfaces part (`$WORLD_SLOT`, `$DECAL_SLOT` and `$SKY_SLOT` become
/// `u32` literals).
pub const MATERIAL: &str = include_str!("../shaders/material.wgsl");
/// The sky dome part (three.js Sky.js, MIT; see the file's notice).
#[cfg(feature = "sky")]
pub const SKY: &str = include_str!("../shaders/sky.wgsl");
/// `kit_material` itself (`$SKY_CALL` becomes the sky colour call).
pub const ENTRY: &str = include_str!("../shaders/entry.wgsl");
/// The sky-only `kit_material` (`$SKY_SLOT` becomes the sky slot), for a
/// world with no boxes: see [`sky_wgsl`].
#[cfg(feature = "sky")]
pub const SKY_ENTRY: &str = include_str!("../shaders/sky_entry.wgsl");

/// Feature `sky-horizon`: the edits that turn the sky part's fixed horizon
/// numbers (the 0.03..0.27 cloud fade, the 0.1..0.4 puff detail, the layer
/// curve and the haze over the clouds) into reads of the horizon record
/// ([`crate::layout::SKY_HORIZON`]), the clouds' fixed look numbers (the
/// lobes' ball shading, the outline's soft band, the lobes' bite and the
/// clouds' brightness) into reads of the cloud look record
/// ([`crate::layout::SKY_CLOUD_LOOK`]) and the clear sky's colour into one
/// toned by the tone record ([`crate::layout::SKY_TONE`]); the clouds' lit
/// side and body shading read the light record
/// ([`crate::layout::SKY_CLOUD_LIGHT`]); with the cloud volume on (that
/// record's z) the cumulus march ([`SKY_CUMULUS`]) draws the clouds instead.
/// Each `(from, to)` matches exactly once; without the feature the sky part
/// ships as written.
#[cfg(feature = "sky-horizon")]
pub const HORIZON_EDITS: [(&str, &str); 15] = [
    // On the same line (the file's line endings differ between checkouts).
    (
        "let r6 = kit_data[6];",
        "let r6 = kit_data[6]; let hz = select(vec4<f32>(0.03, 0.27, 1.0, 1.0), kit_data[8], arrayLength(&kit_data) > 8u);",
    ),
    (
        "if (dir.y > 0.02 && coverage > 0.0) {",
        "if (dir.y > min(0.02, hz.x) && coverage > 0.0) {",
    ),
    (
        "let c = mix(0.35, 0.65, r6.x);",
        "let c = mix(0.35, 0.65, r6.x) * hz.z;",
    ),
    (
        "let horizon_fade = smoothstep(0.03, 0.27, dir.y);",
        "let horizon_fade = smoothstep(hz.x, hz.y, dir.y);",
    ),
    (
        "let detail = smoothstep(0.1, 0.4, dir.y);",
        "let detail = smoothstep(hz.x + 0.07, hz.y + 0.13, dir.y);",
    ),
    (
        "let aerial = mix(col, cloud_col, fex);",
        "let aerial = mix(col, cloud_col, pow(fex, vec3<f32>(hz.w)));",
    ),
    // The cloud look record, read where the clouds are shaped and lit.
    (
        "fn mp_cloud(",
        "fn mp_cl() -> vec4<f32> {
    return select(vec4<f32>(0.3, 0.16, 1.0, 1.0), kit_data[9], arrayLength(&kit_data) > 11u);
}

fn mp_cl2() -> vec4<f32> {
    return select(vec4<f32>(0.8, 0.4, 0.0, 0.0), kit_data[11], arrayLength(&kit_data) > 11u);
}

fn mp_cloud(",
    ),
    (
        "smoothstep(thr, thr + 0.16, v)",
        "smoothstep(thr, thr + mp_cl().y, v)",
    ),
    (
        "let bite = detail * 0.36 *",
        "let bite = detail * 0.36 * mp_cl().z *",
    ),
    ("0.3 * main.z", "mp_cl().x * main.z"),
    (
        "cloud_col = cloud_col * max(day, 0.03);",
        "cloud_col = cloud_col * max(day, 0.03) * mp_cl().w;",
    ),
    // The light record: the lit side's lean and the heaps' ball shading.
    ("out_dir * 0.8 +", "out_dir * mp_cl2().x +"),
    ("0.5 + 0.4 * main.y", "0.5 + mp_cl2().y * main.y"),
    // The cloud volume (record 11.z on): the cumulus march
    // (`shaders/sky_cumulus.wgsl`) in place of the flat clouds.
    (
        "let coverage = r5.z;",
        "let coverage = select(r5.z, 0.0, mp_cu_on()); if (mp_cu_on() && r5.z > 0.0) { col = mp_cumulus(dir, sun, col, lin, fex, sun_e, time, r5); }",
    ),
    // The tone record on the clear sky's scattered light.
    (
        "var col = (lin + l0) * 0.04 + disc_col + night;",
        "let tn = select(vec4<f32>(1.0, 1.0, 1.0, 0.0), kit_data[10], arrayLength(&kit_data) > 11u); let yy = max(dir.y, 0.0); var col = (lin + l0) * 0.04 * tn.xyz * (1.0 - tn.w * yy * yy) + disc_col + night;",
    ),
];

/// Feature `sky-horizon`: the clouds as a real volume (fair-weather cumulus
/// marched through a layer with a flat base, lit through themselves, with
/// a faint cirrus layer), drawn in place of the flat clouds while record
/// 11.z is on ([`crate::sky::CloudVolume`]).
#[cfg(feature = "sky-horizon")]
pub const SKY_CUMULUS: &str = include_str!("../shaders/sky_cumulus.wgsl");

/// Feature `sky-discs`: the sun and moon at their true size
/// (`shaders/sky_discs.wgsl`, [`crate::discs::SkyDiscs`]), drawn in place of
/// the dome's own sun disc while record 14.x is set.
#[cfg(feature = "sky-discs")]
pub const SKY_DISCS: &str = include_str!("../shaders/sky_discs.wgsl");

/// Feature `sky-discs`: the edits (made after [`HORIZON_EDITS`], on lines
/// those leave alone) that turn the dome's sun disc into the disc part's
/// sun and moon, warm the sky's scattered light near the horizon and give
/// the dome its horizon haze and its fine grain while the part is on.
#[cfg(feature = "sky-discs")]
pub const DISC_EDITS: [(&str, &str); 3] = [
    (
        "let disc_col = (760.0 * disc) * min(sun_e * fex, vec3<f32>(80.0));",
        "let disc_col = select((760.0 * disc) * min(sun_e * fex, vec3<f32>(80.0)), mp_discs(dir, sun, sun_e * fex, fex, r6.z, dku), mp_dk_on());",
    ),
    // The haze's unevenness is worked out once (`dku`), for the warmth
    // here and the aureole after it.
    (
        "let l0 = vec3<f32>(0.1) * fex;",
        "let l0 = vec3<f32>(0.1) * fex; let dku = mp_dk_un(dir); lin *= mp_dk_warm(dir, sun, dku);",
    ),
    // The horizon's haze and the sky's fine grain on the dome's finished
    // colour (column 73).
    (
        "return max(col, vec3<f32>(0.0)) * r6.y;",
        "return mp_dk_haze(max(col, vec3<f32>(0.0)), dir, sun) * r6.y * mp_dk_grain(dir);",
    ),
];

/// The sky part as shipped: with feature `sky-horizon`, [`SKY`] with the
/// [`HORIZON_EDITS`] made, then [`SKY_CUMULUS`], without their notes (the
/// GPU gets the code alone, and a sky-only scene's code stays far smaller
/// than the whole painter's). The three.js licence at the top of [`SKY`]
/// stays word for word: its notice ships with every copy (MIT).
#[cfg(feature = "sky-horizon")]
pub fn sky_source() -> String {
    let mut s = SKY.to_string();
    for (from, to) in HORIZON_EDITS {
        s = s.replacen(from, to, 1);
    }
    #[cfg(feature = "sky-discs")]
    for (from, to) in DISC_EDITS {
        s = s.replacen(from, to, 1);
    }
    let keep = s.find(LICENCE_END).map_or(0, |i| i + LICENCE_END.len());
    let mut out = s[..keep].to_string();
    out.push_str(&strip_comments(&s[keep..], false));
    out.push('\n');
    #[cfg(not(feature = "sky-discs"))]
    out.push_str(&strip_comments(SKY_CUMULUS, true));
    // With the disc part the two added parts ship packed ([`pack`]), so a
    // sky-only scene's code stays under a third of the whole painter's.
    #[cfg(feature = "sky-discs")]
    {
        out.push_str(&pack(&strip_comments(SKY_CUMULUS, true)));
        out.push('\n');
        out.push_str(&pack(&strip_comments(SKY_DISCS, true)));
    }
    out
}

/// Feature `sky-discs`: `src` (its comments and indents already gone)
/// without its blank lines, without the spaces round punctuation and
/// without the line ends after `;`, `{`, `}`, `,` and `(`, none of which
/// WGSL needs: a space stays between two names or numbers, and between two
/// operators (`a * -b`, so no two ever join into another one). The code
/// reads the same to the GPU and is about a fifth shorter.
#[cfg(feature = "sky-discs")]
fn pack(src: &str) -> String {
    let (ops, punct) = ("+-&|<>=!/*", "+-&|<>=!/*,;(){}:%");
    let mut out = String::with_capacity(src.len());
    for line in src.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let mut prev = '\n';
        let mut it = line.chars().peekable();
        while let Some(c) = it.next() {
            if c == ' ' {
                let next = it.peek().copied().unwrap_or('\n');
                if prev == ' ' || ((punct.contains(prev) || punct.contains(next)) && !(ops.contains(prev) && ops.contains(next))) {
                    continue;
                }
            }
            out.push(c);
            prev = c;
        }
        if !";{},(".contains(prev) {
            out.push('\n');
        }
    }
    out
}

/// The last words of the three.js licence in [`SKY`].
#[cfg(feature = "sky-horizon")]
const LICENCE_END: &str = "DEALINGS IN THE SOFTWARE.";

/// `src` without its comments: comment-only lines go, trailing comments and
/// spaces are cut, blank lines stay, and with `dedent` the indents go too.
/// WGSL has no string literals, so a `//` always starts a comment.
#[cfg(feature = "sky-horizon")]
fn strip_comments(src: &str, dedent: bool) -> String {
    let mut out = String::with_capacity(src.len());
    for line in src.lines() {
        let code = match line.find("//") {
            Some(i) => {
                let c = line[..i].trim_end();
                if c.trim().is_empty() {
                    continue;
                }
                c
            }
            None => line.trim_end(),
        };
        out.push_str(if dedent { code.trim_start() } else { code });
        out.push('\n');
    }
    out
}

/// A slot number that matches no material (the renderer has 16): decals
/// stay unpainted when the look has no decal slot, and no sky is drawn
/// without a sky slot.
pub const NO_SLOT: u32 = 255;

/// The kit code for these slots. Without feature `sky` the sky slot is
/// ignored (the code carries no sky).
pub fn wgsl(world_slot: u32, decal_slot: u32, sky_slot: u32) -> String {
    #[cfg(all(feature = "sky", not(feature = "sky-horizon")))]
    let (sky_part, sky_call, sky_slot) = (SKY, "mp_sky_rgba(n)", sky_slot);
    #[cfg(feature = "sky-horizon")]
    let sky_src = sky_source();
    #[cfg(feature = "sky-horizon")]
    let (sky_part, sky_call, sky_slot) = (sky_src.as_str(), "mp_sky_rgba(n)", sky_slot);
    #[cfg(not(feature = "sky"))]
    let (sky_part, sky_call, sky_slot) = ("", "vec4<f32>(1.0)", {
        let _ = sky_slot;
        NO_SLOT
    });
    let mut s = String::with_capacity(MATERIAL.len() + sky_part.len() + ENTRY.len() + 2);
    s.push_str(MATERIAL);
    s.push('\n');
    s.push_str(sky_part);
    s.push('\n');
    s.push_str(&ENTRY.replace("$SKY_CALL", sky_call));
    s.replace("$WORLD_SLOT", &world_slot.to_string())
        .replace("$DECAL_SLOT", &decal_slot.to_string())
        .replace("$SKY_SLOT", &sky_slot.to_string())
}

/// The kit code for a game that draws only the sky dome (its scene has no
/// boxes): the sky part and a sky-only entry, so the forward and shadow
/// shaders carry none of the surface painting (smaller, and quicker to
/// compile).
#[cfg(feature = "sky")]
pub fn sky_wgsl(sky_slot: u32) -> String {
    #[cfg(not(feature = "sky-horizon"))]
    let sky = SKY;
    #[cfg(feature = "sky-horizon")]
    let sky_src = sky_source();
    #[cfg(feature = "sky-horizon")]
    let sky = sky_src.as_str();
    let mut s = String::with_capacity(sky.len() + SKY_ENTRY.len() + 1);
    s.push_str(sky);
    s.push('\n');
    s.push_str(&SKY_ENTRY.replace("$SKY_SLOT", &sky_slot.to_string()));
    s
}
