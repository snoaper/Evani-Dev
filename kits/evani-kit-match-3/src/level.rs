//! Levels from `data/levels.json`: board shape, colours, moves, seed, goals
//! and the three star scores.

use serde::Deserialize;

use crate::board::{MAX_COLORS, Shape};

/// One level goal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    /// Clear this many pieces of a colour.
    Collect { color: u8, count: u32 },
    /// Clear every frosting layer.
    Frosting,
    /// Reach this score.
    Score(u32),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Level {
    pub name: String,
    pub seed: u64,
    pub colors: u8,
    pub moves: i32,
    pub shape: Shape,
    pub goals: Vec<Goal>,
    /// Scores for one, two and three stars.
    pub stars: [u32; 3],
}

#[derive(Deserialize)]
struct GoalJson {
    collect: Option<u8>,
    count: Option<u32>,
    frosting: Option<bool>,
    score: Option<u32>,
}

#[derive(Deserialize)]
struct LevelJson {
    name: String,
    seed: u64,
    colors: u8,
    moves: i32,
    board: Vec<String>,
    goals: Vec<GoalJson>,
    stars: [u32; 3],
}

#[derive(Deserialize)]
struct LevelsJson {
    levels: Vec<LevelJson>,
}

/// Parse and check every level.
pub fn levels_from_json(json: &str) -> Result<Vec<Level>, String> {
    let file: LevelsJson = serde_json::from_str(json).map_err(|e| format!("levels.json: {e}"))?;
    if file.levels.is_empty() {
        return Err("levels.json: no levels".into());
    }
    file.levels
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let at = |m: String| format!("levels.json level {} ({}): {m}", i + 1, l.name);
            let shape = Shape::parse(&l.board).map_err(at)?;
            if !(3..=MAX_COLORS).contains(&l.colors) {
                return Err(at(format!("colors must be 3 to {MAX_COLORS}")));
            }
            if !(1..=99).contains(&l.moves) {
                return Err(at("moves must be 1 to 99".into()));
            }
            if !(l.stars[0] <= l.stars[1] && l.stars[1] <= l.stars[2]) {
                return Err(at("stars must rise".into()));
            }
            let mut goals = Vec::new();
            for g in &l.goals {
                goals.push(match (g.collect, g.count, g.frosting, g.score) {
                    (Some(c), Some(n), None, None) if c < l.colors && n > 0 => Goal::Collect { color: c, count: n },
                    (None, None, Some(true), None) => {
                        if shape.frost.iter().all(|&f| f == 0) {
                            return Err(at("frosting goal but no frosting on the board".into()));
                        }
                        Goal::Frosting
                    }
                    (None, None, None, Some(s)) => Goal::Score(s),
                    _ => return Err(at("a goal is {collect, count} with a colour in range, {frosting: true} or {score}".into())),
                });
            }
            if goals.is_empty() {
                return Err(at("no goals".into()));
            }
            Ok(Level {
                name: l.name,
                seed: l.seed,
                colors: l.colors,
                moves: l.moves,
                shape,
                goals,
                stars: l.stars,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_levels_parse() {
        let l = levels_from_json(crate::LEVELS_JSON).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].name, "Sprinkle Meadow");
        assert_eq!(l[0].goals.len(), 2);
        assert!(matches!(l[1].goals[0], Goal::Frosting));
        assert_eq!((l[2].shape.w, l[2].shape.h), (9, 9));
    }

    #[test]
    fn bad_levels_are_refused() {
        let base = r####"{"levels":[{"name":"x","seed":1,"colors":5,"moves":10,"board":["###","###","###"],"goals":GOALS,"stars":[1,2,3]}]}"####;
        let with = |g: &str| levels_from_json(&base.replace("GOALS", g));
        assert!(with(r#"[{"collect":1,"count":5}]"#).is_ok());
        assert!(with(r#"[]"#).is_err());
        assert!(with(r#"[{"collect":9,"count":5}]"#).is_err());
        assert!(with(r#"[{"frosting":true}]"#).is_err(), "no frosting on the board");
        assert!(levels_from_json("{}").is_err());
    }
}
