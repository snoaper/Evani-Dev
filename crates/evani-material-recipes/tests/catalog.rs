//! P1 acceptance tests: the catalogue data and the 32-byte block.
#![cfg(feature = "catalog")]
// Test helpers live outside #[test] fns, where clippy.toml does not exempt expect.
#![allow(clippy::expect_used)]

use evani_material_recipes::block::{fnv1a32, seed_byte, Block, BLOCK_BYTES, CONDITION_NAMES};
use evani_material_recipes::catalog::{Catalog, CatalogError};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load() -> Catalog {
    Catalog::load_dir(&repo().join("catalog/materials")).expect("catalogue loads")
}

/// The `| name | n |` rows under one `### Counts per <what>` heading of
/// CATALOGUE_PLAN section 12.
fn plan_table(what: &str) -> BTreeMap<String, usize> {
    let text = std::fs::read_to_string(repo().join("docs/materials/CATALOGUE_PLAN.md")).expect("plan readable");
    let start = text.find("## 12. Count tables").expect("section 12");
    let section = &text[start..];
    let head = format!("### Counts per {what}");
    let at = section.find(&head).unwrap_or_else(|| panic!("missing {head}"));
    let mut out = BTreeMap::new();
    for line in section[at + head.len()..].lines().skip(1) {
        if line.starts_with('#') {
            break;
        }
        let cells: Vec<&str> = line.trim().trim_matches('|').split('|').map(str::trim).collect();
        if let [name, count] = cells.as_slice()
            && let Ok(n) = count.parse::<usize>()
        {
            out.insert((*name).to_string(), n);
        }
    }
    out
}

#[test]
fn counts_equal_plan_section_12() {
    let c = load();
    assert_eq!(c.rows.len(), 1054);
    assert_eq!(c.recipes.len(), 29);

    let cats = plan_table("category");
    assert_eq!(cats.len(), 11);
    assert_eq!(cats.values().sum::<usize>(), 1054);
    assert_eq!(c.count_by(|r| r.category.clone()), cats);

    let batches = plan_table("batch");
    assert_eq!(batches.len(), 11);
    let got: BTreeMap<String, usize> = c.count_by(|r| r.batch).into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    assert_eq!(got, batches);

    let recipes = plan_table("recipe");
    assert_eq!(recipes.len(), 29);
    assert_eq!(c.count_by(|r| r.recipe.clone()), recipes);
}

#[test]
fn recipe_codes_run_one_to_twenty_nine() {
    let c = load();
    for (i, r) in c.recipes.iter().enumerate() {
        assert_eq!(usize::from(r.code), i + 1, "{}", r.id);
    }
}

#[test]
fn ids_are_unique() {
    let c = load();
    c.check_unique_ids().expect("unique");
    let mut dup = c.clone();
    dup.rows.push(c.rows[0].clone());
    assert_eq!(dup.check_unique_ids(), Err(CatalogError::DuplicateId(c.rows[0].id.clone())));
}

#[test]
fn every_rows_conditions_are_allowed_by_its_recipe() {
    let c = load();
    c.check_conditions().expect("conditions allowed");
    // fire allows only dust: a wet fire row is refused with a named error.
    let mut row = c.rows.iter().find(|r| r.recipe == "fire").expect("a fire row").clone();
    row.conditions = vec!["wet".into()];
    assert!(matches!(c.condition_mask(&row), Err(CatalogError::ConditionNotAllowed { .. })));
    row.conditions = vec!["frozen".into()];
    assert!(matches!(c.condition_mask(&row), Err(CatalogError::UnknownCondition { .. })));
}

#[test]
fn every_row_encodes_to_32_bytes_with_defaults() {
    let c = load();
    c.validate().expect("catalogue validates");
    for row in &c.rows {
        let b = c.encode(row).expect("encodes");
        assert_eq!(b.0.len(), BLOCK_BYTES);
        let f = b.unpack();
        let recipe = c.recipe_for(row).expect("recipe");
        assert_eq!(f.recipe, recipe.code, "{}", row.id);
        // A row with numbers (P4 onwards) sets its own variant; the rest keep 0.
        // An alias (B2) draws as the row it names.
        let drawn = c.resolved(row).expect("alias resolves");
        let want_variant = drawn.params.get("variant").and_then(|v| v.as_str()).map_or(0, |v| recipe.variants.iter().position(|x| x == v).expect("known variant"));
        assert_eq!(usize::from(f.variant), want_variant, "{}", row.id);
        assert_eq!(f.seed, seed_byte(&row.id));
        let want_mask = row.conditions.iter().fold(0u8, |m, n| m | 1 << CONDITION_NAMES.iter().position(|c| c == n).expect("known condition"));
        assert_eq!(f.cond_mask, want_mask, "{}", row.id);
        // The amounts are the row's cond_defaults (B2: wet sand, frosted grass, black ice ...).
        let mut want_amounts = [0u8; 8];
        for (name, v) in &drawn.cond_defaults {
            want_amounts[CONDITION_NAMES.iter().position(|c| c == name).expect("known condition")] = evani_material_recipes::block::quantise(v.as_f64().expect("number") as f32, 0.0, 1.0);
        }
        assert_eq!(f.cond_amounts, want_amounts, "{}", row.id);
    }
}

/// P12: the optional 60 (batch 11). 34 are drawn (`ready`, real numbers: 20 from
/// P5 to P10a and 14 from P12), 26 stay `planned`, each for a reason the plan
/// lists; the drawn ones are in range, name only allowed numbers (encode checks
/// both) and show every recipe family they claim.
#[test]
fn the_optional_sixty_are_drawn_or_planned_for_a_reason() {
    let c = load();
    let batch: Vec<_> = c.rows.iter().filter(|r| r.batch == 11).collect();
    assert_eq!(batch.len(), 60);
    assert_eq!(batch.iter().filter(|r| r.category == "vehicles").count(), 30);
    let planned: Vec<&str> = batch.iter().filter(|r| r.status == "planned").map(|r| r.id.as_str()).collect();
    let mut want = vec![
        // vehicles: stripes and lining (a region mask or a decal), bands (heat bands on the exhaust), a mirror (chrome needs an environment to reflect) and zones,
        // lettering and seams (decals), glass and cloth-with-seams (families without numbers yet)
        "car.paint.racing_stripe", "car.paint.carriage", "car.paint.train", "car.exhaust", "car.chrome.bumper", "car.boiler", "car.rail", "car.tyre.sidewall",
        "car.seat.vinyl", "car.sail", "car.headlight", "car.tail_light",
        // fantasy: needs a pulse on a gem, a blended pass, an impact input, or the nature family
        "gem.opal", "gem.magic_crystal", "magic.ethereal", "magic.forcefield", "magic.potion",
        // sci-fi: regions and two materials, a pulse on skin, traces, glass
        "sf.hull.panel", "sf.alien_skin", "sf.liquid_metal", "sf.circuit", "sf.solar", "sf.biomech", "sf.rust_bucket",
    ];
    want.sort_unstable();
    let mut got = planned;
    got.sort_unstable();
    assert_eq!(got, want);
    for r in batch.iter().filter(|r| r.status == "ready") {
        assert!(r.params.contains_key("numbers"), "{}: a drawn row has numbers", r.id);
        c.encode(r).unwrap_or_else(|e| panic!("{}: {e}", r.id));
    }
    assert_eq!(batch.iter().filter(|r| r.status == "ready").count(), 36);
}

/// P11a and P11b: batch 1 (106 rows). 98 are drawn (`ready`: real numbers, in
/// range, every named number allowed: encode checks all three; batch 3 drew the
/// plank deck, denim, lace, damask and newsprint) and 5 stay
/// `planned`, each for a reason DECISIONS P11 lists; every recipe the batch
/// uses has at least one drawn row.
#[test]
fn batch_one_is_drawn_or_planned_for_a_reason() {
    let c = load();
    let batch: Vec<_> = c.rows.iter().filter(|r| r.batch == 1).collect();
    assert_eq!(batch.len(), 106);
    let mut planned: Vec<&str> = batch.iter().filter(|r| r.status == "planned").map(|r| r.id.as_str()).collect();
    planned.sort_unstable();
    let mut want = vec![
        // a body-region mask (white belly and tail)
        "deer.coat.brown",
        // a twist is geometry (wood.plank.deck's nail heads and
        // cloth.denim.indigo's twill left at the batch-3 join: batch 3's
        // pattern tool and cloth recipe stage draw them; cloth.wool.blanket's
        // stripes left with L4's sett, B4-P3a)
        "tool.rope.hemp",
        // facets and rainbow sparkle; open-water whitecaps
        "glass.crystal.cut", "water.ocean.open",
        // roof.thatch left this list at the batch-2 join: batch 2's foliage
        // recipe draws it on its `straw` variant (B2-P1).
    ];
    want.sort_unstable();
    assert_eq!(planned, want);
    let mut ready_recipes = std::collections::BTreeSet::new();
    for r in batch.iter().filter(|r| r.status == "ready") {
        assert!(r.params.contains_key("numbers"), "{}: a drawn row has numbers", r.id);
        c.encode(r).unwrap_or_else(|e| panic!("{}: {e}", r.id));
        ready_recipes.insert(r.recipe.as_str());
    }
    for r in &batch {
        assert!(ready_recipes.contains(r.recipe.as_str()), "{}: its recipe {} has no drawn batch-1 row", r.id, r.recipe);
    }
    // 95 at batch 1's end; roof.thatch joined from batch 2's foliage recipe,
    // wood.plank.deck, cloth.denim.indigo, cloth.lace.white (the cut-out),
    // wall.paper.damask and paper.newsprint from batch 3, cloth.wool.blanket
    // from L4's sett (B4-P3a).
    assert_eq!(batch.iter().filter(|r| r.status == "ready").count(), 102);
}

#[test]
fn encode_then_decode_round_trips() {
    let c = load();
    for row in &c.rows {
        let b = c.encode(row).expect("encodes");
        let f = b.unpack();
        assert_eq!(Block::pack(&f), b, "{}", row.id);
        assert_eq!(Block::from_hex(&b.to_hex()), Ok(b), "{}", row.id);
        // The decoded numbers land within half a step of the row's own value
        // (its `params.numbers`), else of the recipe default.
        let recipe = c.recipe_for(row).expect("recipe");
        let drawn = c.resolved(row).expect("resolves");
        let own = drawn.params.get("numbers").and_then(|v| v.as_object());
        for (i, n) in recipe.numbers.iter().enumerate() {
            let want = own.and_then(|o| o.get(&n.name)).and_then(|v| v.as_f64()).map_or(n.default, |v| v as f32);
            let back = evani_material_recipes::block::dequantise(f.numbers[i], n.min, n.max);
            assert!((back - want).abs() <= (n.max - n.min) / 255.0 * 0.5 + 1e-4, "{} {}", row.id, n.name);
        }
        for i in recipe.numbers.len()..10 {
            assert_eq!(f.numbers[i], 0, "unused number slots stay zero");
        }
    }
}

#[test]
fn the_seed_is_fnv1a() {
    let c = load();
    for row in c.rows.iter().take(50) {
        let b = c.encode(row).expect("encodes");
        assert_eq!(u32::from(b.0[23]), fnv1a32(row.id.as_bytes()) & 0xff, "{}", row.id);
    }
    assert_eq!(fnv1a32(b"foobar"), 0xbf9c_f968);
}

#[test]
fn a_missing_recipe_is_an_error_not_a_panic() {
    let c = load();
    let mut row = c.rows[0].clone();
    row.recipe = "no-such-recipe".into();
    row.id = "test.missing_recipe".into();
    assert!(matches!(c.encode(&row), Err(CatalogError::MissingRecipe { .. })));
    let mut bad = c.clone();
    bad.rows.push(row);
    assert!(matches!(bad.validate(), Err(CatalogError::MissingRecipe { .. })));
}

#[test]
fn params_override_the_defaults_and_bad_ones_are_errors() {
    let c = load();
    let mut row = c.rows.iter().find(|r| r.recipe == "fur" && r.conditions.iter().any(|c| c == "wet")).expect("fur row").clone();
    row.params = json!({"variant": "stripes", "colours": ["ff0000", null, "#00ff00"], "numbers": {"length": 1.0}, "extras": ["shells"]}).as_object().cloned().expect("object");
    row.cond_defaults = json!({"wet": 1.0}).as_object().cloned().expect("object");
    let f = c.encode(&row).expect("encodes").unpack();
    assert_eq!(f.variant, 1);
    assert_eq!(f.colours[0], [255, 0, 0]);
    assert_eq!(f.colours[1], [0xc0, 0x90, 0x60]);
    assert_eq!(f.colours[2], [0, 255, 0]);
    assert_eq!(f.numbers[0], 255);
    assert_eq!(f.extras, 1);
    assert_eq!(f.cond_amounts[0], 255);

    for params in [json!({"variant": "plaid"}), json!({"numbers": {"nope": 1}}), json!({"numbers": {"length": 2.0}}), json!({"extras": ["nope"]}), json!({"colours": ["zz"]}), json!({"mystery": 1})] {
        let mut r = row.clone();
        r.params = params.as_object().cloned().expect("object");
        assert!(matches!(c.encode(&r), Err(CatalogError::BadParams { .. })), "{params}");
    }
    let mut r = row.clone();
    r.cond_defaults = json!({"snow": 0.5}).as_object().cloned().expect("object");
    r.conditions.retain(|c| c != "snow");
    assert!(matches!(c.encode(&r), Err(CatalogError::BadParams { .. })));
}

/// Material-view round 4: a row may name the viewer's test shape. It is a
/// hint only: the block is the same with it or without, and a shape that is
/// not a name is an error.
#[test]
fn a_rows_shape_is_a_viewer_hint_outside_the_block() {
    let c = load();
    let row = c.rows.iter().find(|r| r.id == "leaf.oak").expect("oak").clone();
    assert_eq!(row.params.get("shape").and_then(|v| v.as_str()), Some("oak_leaf"));
    let mut bare = row.clone();
    bare.params.remove("shape");
    assert_eq!(c.encode(&row).expect("encodes"), c.encode(&bare).expect("encodes"));
    for bad in [json!(3), json!(""), json!(null)] {
        let mut r = row.clone();
        r.params.insert("shape".into(), bad.clone());
        assert!(matches!(c.encode(&r), Err(CatalogError::BadParams { .. })), "{bad}");
    }
}

/// `points_on` (studio material pass, horse.coat.palomino) is a studio hint
/// too: the same block with it or without; it must be a list of names.
#[test]
fn a_rows_points_on_is_a_studio_hint_outside_the_block() {
    let c = load();
    let row = c.rows.iter().find(|r| r.id == "horse.coat.palomino").expect("palomino").clone();
    assert_eq!(row.params.get("points_on"), Some(&json!(["mane", "tail"])));
    let mut bare = row.clone();
    bare.params.remove("points_on");
    assert_eq!(c.encode(&row).expect("encodes"), c.encode(&bare).expect("encodes"));
    for bad in [json!("mane"), json!([]), json!([""]), json!([3]), json!(null)] {
        let mut r = row.clone();
        r.params.insert("points_on".into(), bad.clone());
        assert!(matches!(c.encode(&r), Err(CatalogError::BadParams { .. })), "{bad}");
    }
}

/// `points_to` (studio material pass, horse.coat.red_roan) is a studio hint
/// too: the same block with it or without; it must be a hex colour.
#[test]
fn a_rows_points_to_is_a_studio_hint_outside_the_block() {
    let c = load();
    let row = c.rows.iter().find(|r| r.id == "horse.coat.red_roan").expect("red roan").clone();
    assert!(row.params.get("points_to").and_then(|v| v.as_str()).is_some_and(|s| s.len() == 6));
    let mut bare = row.clone();
    bare.params.remove("points_to");
    assert_eq!(c.encode(&row).expect("encodes"), c.encode(&bare).expect("encodes"));
    for bad in [json!("red"), json!(""), json!(["4a1a10"]), json!(3), json!(null)] {
        let mut r = row.clone();
        r.params.insert("points_to".into(), bad.clone());
        assert!(matches!(c.encode(&r), Err(CatalogError::BadParams { .. })), "{bad}");
    }
}

/// Material-view round 4's rows: slime on organic's see-through slime, the
/// clear quartz see-through, the maple leaves fanned, the damask woven.
#[test]
fn round_4_rows_use_their_new_parts() {
    let c = load();
    let row = |id: &str| c.rows.iter().find(|r| r.id == id).unwrap_or_else(|| panic!("{id}"));
    let slime = c.encode(row("magic.slime")).expect("slime").unpack();
    assert_eq!((slime.recipe, slime.variant, slime.extras), (22, 3, 1 | 2));
    assert_eq!(evani_material_recipes::blend::mode(&c.encode(row("magic.slime")).expect("slime")), evani_material_recipes::blend::OVER);
    let quartz = c.encode(row("gem.quartz_clear")).expect("quartz").unpack();
    assert_eq!(quartz.extras & 4, 4);
    for id in ["leaf.maple.autumn", "leaf.maple.green"] {
        assert_eq!(c.encode(row(id)).expect("maple").unpack().numbers[8], 255, "{id}");
    }
    assert_eq!(c.encode(row("leaf.oak")).expect("oak").unpack().numbers[8], 0);
    let damask = c.encode(row("wall.paper.damask")).expect("damask").unpack();
    assert_eq!(damask.extras, 8, "damask on, the floral motif off");
    // Beans in sauce share the slime variant but stay opaque.
    assert_eq!(evani_material_recipes::blend::mode(&c.encode(row("food.beans")).expect("beans")), evani_material_recipes::blend::OPAQUE);
}

#[test]
fn the_csv_and_the_json_list_the_same_ids_in_the_same_order() {
    let c = load();
    let csv = std::fs::read_to_string(repo().join("docs/materials/catalogue.csv")).expect("csv");
    let ids: Vec<&str> = csv.lines().skip(1).filter(|l| !l.is_empty()).map(|l| l.split(',').next().unwrap_or("")).collect();
    let json_ids: Vec<&str> = c.rows.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(ids, json_ids);
}

#[test]
fn bad_data_is_an_error() {
    assert!(matches!(Catalog::parse("not json", "{}"), Err(CatalogError::Json(_))));
    assert!(matches!(Catalog::parse("{}", "{}"), Err(CatalogError::Spec { .. })));
}

/// The stage table equals `catalog/materials/recipes.json`: every recipe
/// with backend `recipes`, plus paint, with its first quality group (detail
/// when it lists none) and its extras as consecutive bits.
#[test]
fn stage_table_equals_the_catalogue() {
    use evani_material_recipes::catalog::Catalog;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../catalog/materials");
    let cat = Catalog::load_dir(&dir).unwrap();
    let want: Vec<evani_material_recipes::shader::StageRecipe> = cat
        .recipes
        .iter()
        .filter(|r| r.backend == "recipes" || r.id == "paint")
        .map(|r| {
            for (k, e) in r.extras.iter().enumerate() {
                assert_eq!(u32::from(e.mask), 1 << k, "{}: extras are consecutive bits", r.id);
            }
            let group = r.groups.first().map_or(0, |g| evani_material_recipes::shader::GROUPS.iter().position(|x| x == g).unwrap_or_else(|| panic!("{}: group {g}", r.id)));
            (r.code, r.id.clone(), group as u8, r.extras.len() as u8)
        })
        .map(|(code, id, group, extras)| {
            let row = evani_material_recipes::shader::stage_recipe(code).unwrap_or_else(|| panic!("{id}: no stage row"));
            assert_eq!(row.name, id);
            evani_material_recipes::shader::StageRecipe { code, name: row.name, group, extras }
        })
        .collect();
    assert_eq!(want.as_slice(), &evani_material_recipes::shader::STAGE[..]);
}

/// The extras names equal `recipes.json`, in bit order, for every stage row.
#[test]
fn extras_names_equal_the_catalogue() {
    use evani_material_recipes::shader::{stage_recipe, EXTRA_NAMES, STAGE};
    let cat = load();
    for (row, names) in STAGE.iter().zip(EXTRA_NAMES) {
        let r = cat.recipes.iter().find(|r| r.code == row.code).expect("stage row in the catalogue");
        let want: Vec<&str> = r.extras.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, want.as_slice(), "{}", row.name);
        assert!(stage_recipe(row.code).is_some());
    }
}


/// B2: an `alias` row is another ready row under its own id, with its own
/// condition amounts (weather stays a condition layer: a made-up "wet road" is asphalt
/// with the wet layer on; no catalogue row is an alias today, black ice has its own
/// ice row, so the mechanism is tested on a row made here). It resolves to a ready row, has no numbers of its
/// own, cannot chain, and must share its base's recipe and conditions.
#[test]
fn an_alias_resolves_to_a_ready_row_and_cannot_chain() {
    let mut c = load();
    let mut made = c.row("asphalt.road").expect("asphalt").clone();
    made.id = "test.wet_road".into();
    made.name = "Wet road".into();
    made.status = "alias".into();
    made.params = json!({"alias_of": "asphalt.road"}).as_object().cloned().expect("object");
    made.cond_defaults = json!({"wet": 1.0}).as_object().cloned().expect("object");
    c.rows.push(made);
    let black_ice = c.row("test.wet_road").expect("made alias");
    assert_eq!(black_ice.status, "alias");
    let road = c.row("asphalt.road").expect("asphalt");
    assert_eq!(road.status, "ready");
    let drawn = c.resolved(black_ice).expect("resolves");
    assert_eq!((drawn.id.as_str(), drawn.recipe.as_str()), ("test.wet_road", "ground"));
    assert_eq!(drawn.params, road.params, "the base's numbers, none of its own");
    // The block is the base's with the wet layer on, and its own seed.
    let a = c.encode(black_ice).expect("alias encodes").unpack();
    let b = c.encode(road).expect("base encodes").unpack();
    assert_eq!((a.recipe, a.variant, a.colours, a.numbers, a.extras, a.cond_mask), (b.recipe, b.variant, b.colours, b.numbers, b.extras, b.cond_mask));
    assert_eq!((a.cond_amounts[0], b.cond_amounts[0]), (255, 0), "wet 1.0 over asphalt's none");
    // A plain row resolves to itself.
    assert!(matches!(c.resolved(road).expect("plain"), std::borrow::Cow::Borrowed(_)));

    let with = |f: &dyn Fn(&mut Catalog)| {
        let mut x = c.clone();
        f(&mut x);
        x.validate()
    };
    let at = |x: &Catalog, id: &str| x.rows.iter().position(|r| r.id == id).expect("row");
    // No chains: an alias of an alias is refused.
    let chained = with(&|x| {
        let mut second = x.rows[at(x, "test.wet_road")].clone();
        second.id = "test.chain".into();
        second.params = json!({"alias_of": "test.wet_road"}).as_object().cloned().expect("object");
        x.rows.push(second);
    });
    assert!(matches!(chained, Err(CatalogError::BadParams { .. })), "{chained:?}");
    // Not a catalogue id, a planned base, numbers of its own, no base: all refused.
    for (what, edit) in [
        ("unknown base", json!({"alias_of": "no.such.row"})),
        ("planned base", json!({"alias_of": "ground.soil.clay_red"})),
        ("own numbers", json!({"alias_of": "asphalt.road", "numbers": {"rough": 0.5}})),
        ("no base", json!({})),
    ] {
        let got = with(&|x| {
            let i = at(x, "test.wet_road");
            x.rows[i].params = edit.as_object().cloned().expect("object");
        });
        assert!(matches!(got, Err(CatalogError::BadParams { .. })), "{what}: {got:?}");
    }
    let other_recipe = with(&|x| {
        let i = at(x, "test.wet_road");
        x.rows[i].recipe = "ice".into();
        x.rows[i].conditions = vec!["dust".into()];
    });
    assert!(matches!(other_recipe, Err(CatalogError::BadParams { .. })), "{other_recipe:?}");
    // Every alias in the catalogue resolves.
    for r in c.rows.iter().filter(|r| r.status == "alias") {
        c.resolved(r).expect("alias resolves");
    }
}

/// B2: obsidian is an opaque stone, not glass (the gem recipe's raw crystal,
/// clear 0); the rows that are a base look plus a default condition carry the
/// amount in `cond_defaults`.
#[test]
fn b2_catalogue_changes_are_in() {
    let c = load();
    assert_eq!(c.row("rock.obsidian").expect("obsidian").recipe, "gem");
    for (id, layer, amount) in [("ground.sand.wet", "wet", 0.8), ("ground.grass.frosted", "snow", 0.4), ("rock.cave_wall", "wet", 0.6), ("rock.stalactite", "wet", 0.5), ("rock.mossy_boulder", "growth", 0.6)] {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(row.cond_defaults.get(layer).and_then(|v| v.as_f64()), Some(amount), "{id}");
    }
}

/// B5-P2: bristles and plates (hide), hoof, scutes and banded (keratin). The 8
/// batch-5 rows that use them and the 4 batch-6 rows that prove the rest are
/// drawn, use the new variants and numbers, and a block that never sets the new
/// numbers is what it was before they existed (their defaults are their lowest
/// value, which encodes to a zero byte).
/// L4 (B4-P3a/b, B4-N2b): the 27 batch-4 cloth rows and batch 1's striped
/// blanket draw, on the new parts where their look needs them; every cloth
/// row that drew before keeps its 32 bytes (the new numbers and extras are
/// zero there); the sett kinds the shader's table holds are the schema's.
#[test]
fn l4_cloth_rows_use_the_new_parts() {
    let c = load();
    let cloth = evani_material_recipes::recipes::cloth::SCHEMA;
    let want: [(&str, &str, Option<&str>, bool); 28] = [
        ("cloth.cotton.unbleached", "plain", None, true),
        ("cloth.cotton.chambray", "plain", Some("chambray"), true),
        ("cloth.linen.natural", "plain", None, true),
        ("cloth.linen.white", "plain", None, true),
        ("cloth.muslin", "open", None, false),
        ("cloth.calico_print", "plain", None, false),
        ("cloth.gingham", "plain", Some("gingham"), false),
        ("cloth.plaid", "twill", Some("plaid"), false),
        ("cloth.tartan", "twill", Some("tartan"), false),
        ("cloth.pinstripe", "twill", Some("pinstripe"), false),
        ("cloth.ticking", "twill", Some("ticking"), false),
        ("cloth.canvas.waxed", "plain", None, true),
        ("cloth.canvas.army", "plain", None, true),
        ("cloth.denim.faded", "twill", None, true),
        ("cloth.denim.black", "twill", None, true),
        ("cloth.gabardine", "twill", None, false),
        ("cloth.drill.khaki", "twill", None, false),
        ("cloth.serge.navy", "twill", None, false),
        ("cloth.tweed.brown", "twill", None, true),
        ("cloth.tweed.herringbone", "herringbone", None, true),
        ("cloth.houndstooth", "twill", Some("houndstooth"), false),
        ("cloth.wool.homespun", "plain", None, true),
        ("cloth.wool.fine_black", "twill", None, false),
        ("cloth.wool.red", "twill", None, false),
        ("cloth.wool.boiled", "felt", None, false),
        ("cloth.felt.hat", "felt", None, false),
        ("cloth.felt.craft", "felt", None, false),
        ("cloth.wool.blanket", "felt", Some("ticking"), false),
    ];
    let at = |name: &str| cloth.number(name).expect("number");
    for (id, variant, sett, slubs) in want {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!((row.recipe.as_str(), row.status.as_str()), ("cloth", "ready"), "{id}");
        let f = c.encode(row).expect("encodes").unpack();
        assert_eq!(cloth.variants[usize::from(f.variant)], variant, "{id}");
        assert_eq!(f.extras & 16 != 0, sett.is_some(), "{id}: sett extra");
        assert_eq!(f.extras & 32 != 0, slubs, "{id}: slubs extra");
        assert_eq!(f.numbers[at("slub")] > 0, slubs, "{id}: slub number with the extra");
        if let Some(k) = sett {
            let kind = evani_material_recipes::recipes::cloth::SETT_KINDS.iter().position(|n| *n == k).expect("kind");
            // n8 is unorm over 0..7: the stored byte rounds back to the kind.
            assert_eq!((f32::from(f.numbers[at("sett_kind")]) / 255.0 * 7.0).round() as usize, kind, "{id}: sett kind");
        }
    }
    // The cloth rows that drew before L4 keep their blocks: nothing set in
    // the appended slots or bits.
    for id in ["cloth.cotton.white", "cloth.burlap", "cloth.canvas.natural", "cloth.denim.indigo", "cloth.knit.chunky", "cloth.velvet.red", "cloth.silk.plain", "wall.carpet.wool", "wall.carpet.patterned", "wall.rug.woven", "car.canvas.cover"] {
        let f = c.encode(c.row(id).expect("row")).expect("encodes").unpack();
        assert_eq!(f.extras & 0b11_0000, 0, "{id}");
        for name in ["sett_scale", "sett_kind", "slub"] {
            assert_eq!(f.numbers[at(name)], 0, "{id} {name}");
        }
    }
    // The shader's sett table has one entry per kind and sett_kind spans them.
    let src = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders/rc_cloth.wgsl")).expect("shader");
    let n = evani_material_recipes::recipes::cloth::SETT_KINDS.len();
    assert!(src.contains(&format!("array<u32, {n}>(")), "the sett table has {n} kinds");
    assert_eq!(cloth.numbers[at("sett_kind")].max, (n - 1) as f32);
}

#[test]
fn b5_p2_hide_and_horn_rows_use_the_new_parts() {
    let c = load();
    let want: [(&str, &str, &str); 12] = [
        ("pig.skin.pink", "hide", "plain"),
        ("pig.skin.black", "hide", "plain"),
        ("pig.skin.spotted", "hide", "plain"),
        ("horse.muzzle.dark", "hide", "plain"),
        ("horse.muzzle.pink", "hide", "plain"),
        ("rhino.hide", "hide", "plates"),
        ("walrus.hide", "hide", "wrinkled"),
        ("horse.hoof.dark", "keratin", "hoof"),
        ("horse.hoof.pale", "keratin", "hoof"),
        ("horse.hoof.striped", "keratin", "hoof"),
        ("turtle.shell", "keratin", "scutes"),
        ("armadillo.shell", "keratin", "banded"),
    ];
    let hide = evani_material_recipes::recipes::hide::SCHEMA;
    let keratin = evani_material_recipes::recipes::keratin::SCHEMA;
    for (id, recipe, variant) in want {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!((row.recipe.as_str(), row.status.as_str()), (recipe, "ready"), "{id}");
        let f = c.encode(row).expect("encodes").unpack();
        let schema = if recipe == "hide" { hide } else { keratin };
        assert_eq!(schema.variants[usize::from(f.variant)], variant, "{id}");
    }
    let n = |id: &str| c.encode(c.row(id).expect("row")).expect("encodes").unpack().numbers;
    let at = |s: &evani_material_recipes::recipes::Schema, name: &str| s.number(name).expect("number");
    // Bristles: the pigs and the walrus have hairs, the plates have none; the
    // bristles extra (bit 2) is on wherever the number is.
    for id in ["pig.skin.pink", "pig.skin.black", "pig.skin.spotted", "horse.muzzle.dark", "horse.muzzle.pink", "walrus.hide"] {
        assert!(n(id)[at(&hide, "bristles")] > 0, "{id}: bristles");
        assert_eq!(c.encode(c.row(id).expect("row")).expect("encodes").unpack().extras & 4, 4, "{id}: bristles extra");
    }
    assert_eq!(n("rhino.hide")[at(&hide, "bristles")], 0);
    assert!(n("rhino.hide")[at(&hide, "plate_scale")] > 0);
    // Stripes: only the striped hoof has them.
    assert!(n("horse.hoof.striped")[at(&keratin, "bands")] > 0);
    for id in ["horse.hoof.dark", "horse.hoof.pale", "turtle.shell", "armadillo.shell"] {
        assert_eq!(n(id)[at(&keratin, "bands")], 0, "{id}");
    }
    // An older row that never sets the new numbers keeps zero bytes there.
    let toad = n("toad.warty");
    for name in ["bristles", "plate_scale", "bristle_scale"] {
        assert_eq!(toad[at(&hide, name)], 0, "toad.warty {name}");
    }
    assert_eq!(n("horn.cattle")[at(&keratin, "bands")], 0);
}

/// P7-2: feathers, scales, shells and wings. The 17 rows (the dragonfly wing
/// back to `planned` with its numbers kept) draw on the new
/// variants, pattern kinds, spots, dots and film; every older row of the four
/// recipes keeps its 32 bytes (the new slots and bits are zero there), and a
/// ringed snake that uses C as its third ring colour keeps the shimmer off.
#[test]
fn p7_2_feather_scale_shell_and_wing_rows_use_the_new_parts() {
    use evani_material_recipes::recipes::{Schema, feather, keratin, scales, thin};
    let c = load();
    let want: [(&str, Schema, &str); 17] = [
        ("egret.feather", feather::SCHEMA, "plume"),
        ("ostrich.feather", feather::SCHEMA, "plume"),
        ("misc.feather_duster", feather::SCHEMA, "plume"),
        ("pheasant.feather", feather::SCHEMA, "scalloped"),
        ("goose.feather", feather::SCHEMA, "scalloped"),
        ("quill.feather", feather::SCHEMA, "quill"),
        ("snake.banded", scales::SCHEMA, "overlapping"),
        ("snake.garter", scales::SCHEMA, "overlapping"),
        ("pike.skin", scales::SCHEMA, "overlapping"),
        ("mackerel.skin", scales::SCHEMA, "overlapping"),
        ("ladybird.shell", keratin::SCHEMA, "beetle"),
        ("snail.shell", keratin::SCHEMA, "spiral"),
        ("butterfly.orange", thin::SCHEMA, "wing"),
        ("butterfly.blue", thin::SCHEMA, "wing"),
        ("dragonfly.wing", thin::SCHEMA, "wing"),
        ("squid.skin", thin::SCHEMA, "membrane"),
        ("shrimp.shell", thin::SCHEMA, "segments"),
    ];
    let f = |id: &str| c.encode(c.row(id).unwrap_or_else(|| panic!("{id}"))).expect("encodes").unpack();
    for (id, schema, variant) in want {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        // The dragonfly wing waits (parallel veins, a cell mesh, see-through) but keeps its numbers.
        let status = if id == "dragonfly.wing" { "planned" } else { "ready" };
        assert_eq!((row.recipe.as_str(), row.status.as_str()), (schema.name, status), "{id}");
        assert_eq!(schema.variants[usize::from(f(id).variant)], variant, "{id}");
    }
    let num = |s: &Schema, name: &str| s.number(name).expect("number");
    // An extra's bit read from the schema by name, so a merge that renumbers
    // a recipe's extras (round 4's damask took thin's bit 3, film moved to
    // bit 4) moves the check with it.
    let bit = |s: &Schema, name: &str| 1u8 << s.extras.iter().position(|x| *x == name).unwrap_or_else(|| panic!("{} has no extra {name}", s.name));
    // Scales: kinds 1 to 4 in order (n7 over 0..4), and the rings keep the shimmer off.
    for (id, kind) in [("snake.banded", 1.0), ("snake.garter", 2.0), ("pike.skin", 3.0), ("mackerel.skin", 4.0)] {
        assert_eq!((f32::from(f(id).numbers[num(&scales::SCHEMA, "pattern_kind")]) / 255.0 * 4.0).round(), kind, "{id}");
    }
    assert_eq!(f("snake.banded").extras & 2, 0, "snake.banded: shimmer off (C is a ring colour)");
    // Keratin: spots on the ladybird only; thin: film on the shimmering wings.
    assert_eq!(f("ladybird.shell").extras & bit(&keratin::SCHEMA, "spots"), bit(&keratin::SCHEMA, "spots"));
    assert!(f("ladybird.shell").numbers[num(&keratin::SCHEMA, "spot_scale")] > 0);
    for (id, film) in [("butterfly.orange", false), ("butterfly.blue", true), ("dragonfly.wing", true), ("squid.skin", false), ("shrimp.shell", false)] {
        assert_eq!(f(id).extras & bit(&thin::SCHEMA, "film") != 0, film, "{id}: film extra");
        assert_eq!(f(id).numbers[num(&thin::SCHEMA, "film")] > 0, film, "{id}: film number with the extra");
    }
    assert!(f("squid.skin").numbers[num(&thin::SCHEMA, "dots")] > 0);
    // Every older row of the four recipes: nothing in the appended slots or bits.
    let new_ids: Vec<&str> = want.iter().map(|w| w.0).collect();
    let mut old = 0;
    // Rows that later packages drew on purpose with these parts (W3's paper
    // and fruit use `dots`) are not older rows; they are named, never guessed.
    let later = later_package_rows();
    for row in c.rows.iter().filter(|r| r.status == "ready" && !new_ids.contains(&r.id.as_str()) && !later.contains(&r.id.as_str())) {
        let b = f(&row.id);
        let (slots, bits): (&[&str], u8) = match row.recipe.as_str() {
            "feather" => (&[], 0),
            "scales" => (&["pattern_kind"], 0),
            "keratin" => (&["spot_scale"], bit(&keratin::SCHEMA, "spots")),
            "thin" => (&["dots", "film", "cross"], bit(&thin::SCHEMA, "film")),
            _ => continue,
        };
        let schema = evani_material_recipes::recipes::schema(c.recipe_for(row).expect("recipe").code).expect("schema");
        for name in slots {
            assert_eq!(b.numbers[num(schema, name)], 0, "{} {name}", row.id);
        }
        assert_eq!(b.extras & bits, 0, "{}", row.id);
        if row.recipe == "feather" {
            assert!(b.variant < 4, "{}: an older feather row on a new variant", row.id);
        }
        old += 1;
    }
    assert!(old >= 50, "older rows checked: {old}");
}

/// P8-4: finishes and odds. The 12 rows draw on the new variants, the
/// temper and lustre extras and the lustre and swirl numbers; every older row
/// of the six recipes keeps its 32 bytes (nothing in the new slots or bits,
/// no older row on a new variant).
#[test]
fn p8_4_finish_rows_use_the_new_parts() {
    use evani_material_recipes::recipes::{Schema, ceramic, glass, leather, metal, organic, wood};
    let c = load();
    let want: [(&str, Schema, &str); 12] = [
        ("timber.charred", wood::SCHEMA, "charred"),
        ("misc.charcoal", wood::SCHEMA, "charred"),
        ("finish.marquetry", wood::SCHEMA, "marquetry"),
        ("pot.lustre", ceramic::SCHEMA, "stoneware"),
        ("food.corn_cob", organic::SCHEMA, "kernels"),
        ("plastic.marble_toy", glass::SCHEMA, "window"),
        ("uph.leather_button", leather::SCHEMA, "tufted"),
        ("metal.steel.damascus", metal::SCHEMA, "damascus"),
        ("metal.steel.case_hardened", metal::SCHEMA, "case colours"),
        ("metal.zinc.galvanised", metal::SCHEMA, "spangle"),
        ("metal.heat_tinted", metal::SCHEMA, "plain"),
        ("cloth.sequins", metal::SCHEMA, "sequins"),
    ];
    let f = |id: &str| c.encode(c.row(id).unwrap_or_else(|| panic!("{id}"))).expect("encodes").unpack();
    for (id, schema, variant) in want {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!((row.recipe.as_str(), row.status.as_str()), (schema.name, "ready"), "{id}");
        assert_eq!(schema.variants[usize::from(f(id).variant)], variant, "{id}");
    }
    let num = |s: &Schema, name: &str| s.number(name).expect("number");
    // Charred wood's cracks are its plank gaps; the temper and lustre extras
    // come with their rows, and lustre and swirl numbers with them.
    for id in ["timber.charred", "misc.charcoal"] {
        assert_eq!(f(id).extras & 2, 2, "{id}: plank gaps (the cracks)");
    }
    assert_eq!(f("metal.heat_tinted").extras & 64, 64, "temper");
    assert_eq!(f("pot.lustre").extras & 8, 8, "lustre");
    assert!(f("pot.lustre").numbers[num(&ceramic::SCHEMA, "lustre")] > 0);
    assert!(f("plastic.marble_toy").numbers[num(&glass::SCHEMA, "swirl")] > 0);
    // Every older row of the six recipes: nothing in the appended slots or
    // bits, and no older row on a variant P8-4 added.
    let new_ids: Vec<&str> = want.iter().map(|w| w.0).collect();
    let mut old = 0;
    // Material-view round 6 moved the pie onto organic's pastry variant.
    for row in c.rows.iter().filter(|r| r.status == "ready" && !new_ids.contains(&r.id.as_str()) && r.id != "food.pie") {
        let (slots, bits, first_new): (&[&str], u8, u8) = match row.recipe.as_str() {
            "metal" => (&[], 64, 9),
            "wood" => (&[], 0, 13),
            "ceramic" => (&["lustre"], 8, 10),
            "organic" => (&[], 0, 5),
            "glass" => (&["swirl"], 0, 7),
            "leather" => (&[], 0, 5),
            _ => continue,
        };
        let b = f(&row.id);
        let schema = evani_material_recipes::recipes::schema(c.recipe_for(row).expect("recipe").code).expect("schema");
        for name in slots {
            assert_eq!(b.numbers[num(schema, name)], 0, "{} {name}", row.id);
        }
        assert_eq!(b.extras & bits, 0, "{}", row.id);
        assert!(b.variant < first_new, "{}: an older row on a P8-4 variant", row.id);
        old += 1;
    }
    assert!(old >= 100, "older rows checked: {old}");
}

/// A number added to a recipe after its first rows must default to its lowest
/// value, which encodes to a zero byte, so every earlier row keeps its exact
/// 32 bytes (and the models and proofs that carry them stay valid). Found at
/// the batch 3-5 join: fur's `mix` defaulted to 0.5 and changed 13 older
/// coats' blocks without changing how they draw. Rule: any number a ready row
/// leaves out has `default == min`, except the ones listed here, which were
/// designed with their recipe's first rows.
#[test]
fn a_number_rows_leave_out_defaults_to_its_lowest() {
    const DESIGNED: [(&str, &str); 2] = [("glass", "backlight"), ("glass", "pane_scale")];
    let read = |f: &str| -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(repo().join("catalog/materials").join(f)).expect("readable")).expect("json")
    };
    let recipes = read("recipes.json");
    let materials = read("materials.json");
    let rows = materials.as_array().or_else(|| materials.as_object().and_then(|o| o.values().find_map(|v| v.as_array()))).expect("rows");
    let mut bad = Vec::new();
    for r in recipes["recipes"].as_array().expect("recipes").iter().filter(|r| r["backend"] == "recipes") {
        let id = r["id"].as_str().expect("id");
        for n in r["numbers"].as_array().expect("numbers") {
            let name = n["name"].as_str().expect("name");
            if n["default"].as_f64() == n["min"].as_f64() || DESIGNED.contains(&(id, name)) {
                continue;
            }
            let left_out = rows.iter().filter(|m| m["recipe"] == id && m["status"] == "ready" && m["params"]["numbers"].get(name).is_none()).count();
            if left_out > 0 {
                bad.push(format!("{id}.{name}: default {} (lowest {}), left out by {left_out} ready rows", n["default"], n["min"]));
            }
        }
    }
    assert!(bad.is_empty(), "set the default to the lowest value, or set the number on those rows:\n{}", bad.join("\n"));
}

/// P8-1 (batch 8, numbers only): 40 timber, crop, kitchen, furniture and misc
/// rows draw on recipes that already exist. Each is ready, sits on its row's
/// recipe, packs to its own 32 bytes (no two share a block, so no near-copies),
/// and `crop.vegetable_patch` stays planned (its rows need a layout the foliage
/// recipe does not have).
#[test]
fn p8_1_wood_crop_and_kitchen_rows_are_ready_and_distinct() {
    let c = load();
    let ids = [
        "timber.cedar", "timber.birch", "timber.cherry", "timber.mahogany", "timber.teak", "timber.walnut", "timber.redwood", "timber.ash",
        "timber.elm", "timber.olive", "timber.driftwood", "timber.end_grain", "timber.plywood", "crop.wheat.green", "crop.corn", "crop.cotton",
        "crop.tobacco", "crop.straw", "crop.sugarcane", "crop.rice", "kitchen.cast_iron_pan", "kitchen.copper_pot", "kitchen.tin_cup",
        "kitchen.stainless", "kitchen.silverware", "kitchen.cast_stove", "kitchen.enamel_stove", "kitchen.coffee_grinder", "kitchen.lantern",
        "kitchen.oil_can", "finish.varnish.dark", "finish.varnish.golden", "finish.painted.green", "finish.gilt", "finish.wax_polish",
        "misc.wicker", "misc.rattan", "misc.cork", "misc.coal_lump", "misc.cigar_ash",
    ];
    let mut seen = BTreeMap::new();
    for id in ids {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(row.status, "ready", "{id}");
        let b = c.encode(row).expect("encodes");
        assert_eq!(b.0.len(), BLOCK_BYTES, "{id}");
        if let Some(other) = seen.insert(b.0, id) {
            panic!("{id} packs to the same block as {other}");
        }
    }
    assert_eq!(c.row("crop.vegetable_patch").expect("row").status, "planned");
}

/// Rows drawn by the W3 numbers packages (P8-2, P8-3, P9-2), each package's
/// ready rows. Older-row guards of earlier packages skip exactly these.
const P8_2_ROWS: &[&str] = &[
    "pot.stoneware.brown", "pot.stoneware.grey", "pot.porcelain.blue_white", "pot.enamel.speckled", "pot.earthenware", "pot.crackle",
    "pot.celadon", "pot.bisque", "glass.bottle.brown", "glass.bottle.clear", "glass.bottle.cobalt", "glass.milk", "glass.lamp_chimney",
    "glass.jar.mason", "glass.frosted_bulb", "glass.vaseline", "plastic.clear", "paper.parchment", "paper.kraft", "paper.cardboard",
    "paper.book_cloth", "paper.book_leather", "paper.banknote", "paper.wanted_poster",
    "wax.candle.beeswax", "wax.candle.red", "wax.soap", "wax.lampshade", "jewel.jet", "jewel.coral_bead",
];

fn later_package_rows() -> Vec<&'static str> {
    [P8_2_ROWS, P8_3_ROWS, P9_2_ROWS].concat()
}

/// Batch 8 and 9 numbers packages (W3: P8-2, P8-3, P9-2): every listed row is
/// ready, encodes to its own 32 bytes (no two rows of a package share a
/// block, so no near-copies), and every row the package held back stays
/// planned with its numbers kept.
fn numbers_rows_are_ready_and_distinct(ids: &[&str], held: &[&str]) {
    let c = load();
    let mut seen = BTreeMap::new();
    for id in ids {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(row.status, "ready", "{id}");
        let b = c.encode(row).unwrap_or_else(|e| panic!("{id}: {e}"));
        assert_eq!(b.0.len(), BLOCK_BYTES, "{id}");
        if let Some(other) = seen.insert(b.0, *id) {
            panic!("{id} packs to the same block as {other}");
        }
    }
    for id in held {
        let row = c.row(id).unwrap_or_else(|| panic!("{id}"));
        assert_eq!(row.status, "planned", "{id}: held back with its reason");
        assert!(row.params.contains_key("numbers"), "{id}: a held row keeps the numbers it was tried with");
    }
}

/// P8-2 (batch 8, numbers only): pottery, glassware, paper and books, wax,
/// toy glass and jewellery. Held: cut crystal and paste (no facets on the
/// glass recipe), open ocean (no open-sea whitecaps), the map (a printed
/// picture, not a pattern), the playing card (the same printed picture), the
/// paper lantern (a glow outside the nature scene).
#[test]
fn p8_2_pottery_glass_and_paper_rows_are_ready_and_distinct() {
    numbers_rows_are_ready_and_distinct(P8_2_ROWS, &["glass.crystal.cut", "jewel.paste", "water.ocean.open", "paper.map", "paper.playing_card", "paper.paper_lantern"]);
    // Black ice is held (the ice recipe lightens a near-black tint to slate; batch 10) with the numbers it was tried with.
    let ice = load();
    let ice = ice.row("ground.ice.sheet_road").expect("row");
    assert_eq!((ice.status.as_str(), ice.recipe.as_str()), ("planned", "ice"));
    assert!(ice.params.contains_key("numbers"));
}

/// P8-3's ready rows.
const P8_3_ROWS: &[&str] = &[
    "food.meat.jerky", "food.fish.cooked", "food.cheese.hard", "food.egg", "food.apple", "food.orange", "food.grapes", "food.banana",
    "food.pie", "food.chocolate", "food.honey", "food.coffee", "food.whiskey",
    "food.wine", "misc.tobacco", "misc.sponge", "misc.ivory_comb", "misc.leather_strap", "uph.horsehair", "uph.chintz",
    "uph.cushion_velvet",
];

/// P8-3 (batch 8, numbers only): food and soft goods. Held: bacon (its fat
/// streaks need a stripe the flesh recipe lacks; the marbling reads as raw meat)
/// and beer (a see-through gold body reads as honey without a foam top);
/// round 5 held cake (icing over sponge needs a cut-slice preview) and milk
/// (an opaque white liquid needs a liquid preview), both plain white balls,
/// and stew (chunks in a glossy liquid).
#[test]
fn p8_3_food_and_soft_goods_rows_are_ready_and_distinct() {
    numbers_rows_are_ready_and_distinct(P8_3_ROWS, &["food.meat.bacon", "food.beer", "food.cake", "food.milk", "food.stew"]);
}

/// P9-2's ready rows.
const P9_2_ROWS: &[&str] = &[
    "cloth.knit.jersey", "cloth.knit.ribbed", "cloth.knit.socks", "cloth.velvet.black", "cloth.velvet.green", "cloth.satin.white",
    "cloth.satin.black", "cloth.silk.brocade", "cloth.fleece", "cloth.terry", "cloth.embroidery", "cloth.lace.black", "cloth.tulle",
    "cloth.nylon", "cloth.hi_vis", "cloth.vinyl", "cloth.rubberised", "fit.button.horn", "fit.button.shell", "fit.button.bone",
    "fit.bead.glass", "fit.bead.wood", "fit.quillwork", "fit.ribbon", "fit.straw_hat", "fit.cork_sole", "fit.celluloid",
    "fit.clog_wood", "fit.badge_enamel", "wood.maple.curly", "wood.ash.handle", "wood.hickory.handle", "wood.ebony", "wood.rosewood",
    "grip.ivory", "grip.pearl", "grip.bakelite", "grip.leather_wrap", "grip.cord_wrap", "armour.leather_hard", "armour.kevlar",
];

/// P9-2 (batch 9, numbers only): the rest of the clothing, buttons, beads,
/// handles, grips, hard leather and kevlar. Held: chiffon (sheer needs the
/// see-through pass; opaque it reads as pink silk). The cloth rows sit on L4's
/// cloth stage (sheen moved into the recipe), so they were looked at after it.
#[test]
fn p9_2_clothing_grips_and_beads_rows_are_ready_and_distinct() {
    numbers_rows_are_ready_and_distinct(P9_2_ROWS, &["cloth.chiffon"]);
}
