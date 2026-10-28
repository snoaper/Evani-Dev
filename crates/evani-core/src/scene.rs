//! Scene stack (menu over game over loading screen...).
//!
//! Kits register scene factories by name in the [`SceneRegistry`]; the app
//! pushes/pops by name. Only the top scene gets `update`; scenes below are
//! paused. Transitions requested during an update are applied after it.

use std::collections::BTreeMap;

use crate::ecs::World;

/// What a scene asks for after its update.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SceneTransition {
    #[default]
    None,
    Push(String),
    Pop,
    Replace(String),
}

pub trait Scene {
    fn name(&self) -> &str;
    fn on_enter(&mut self, _world: &mut World) {}
    fn on_exit(&mut self, _world: &mut World) {}
    /// Another scene was pushed on top.
    fn on_pause(&mut self, _world: &mut World) {}
    /// The scene on top was popped.
    fn on_resume(&mut self, _world: &mut World) {}
    /// Once per fixed tick, top scene only.
    fn update(&mut self, _world: &mut World) -> SceneTransition {
        SceneTransition::None
    }
    /// Live code swapping (dev builds only): `update` called through
    /// subsecond's jump table. Never override this.
    #[cfg(feature = "hotpatch")]
    #[doc(hidden)]
    fn __evani_hot_update(&mut self, world: &mut World) -> SceneTransition {
        let f: fn(&mut Self, &mut World) -> SceneTransition = <Self as Scene>::update;
        subsecond::HotFn::current(f).call((self, world))
    }
}

pub type SceneFactory = Box<dyn Fn() -> Box<dyn Scene>>;

/// Named scene factories (registered by kits).
#[derive(Default)]
pub struct SceneRegistry {
    factories: BTreeMap<String, SceneFactory>,
}

impl SceneRegistry {
    pub fn register(&mut self, name: &str, factory: impl Fn() -> Box<dyn Scene> + 'static) {
        self.factories.insert(name.to_string(), Box::new(factory));
    }

    pub fn create(&self, name: &str) -> Option<Box<dyn Scene>> {
        self.factories.get(name).map(|f| f())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.factories.keys().map(String::as_str)
    }
}

#[derive(Default)]
pub struct SceneStack {
    stack: Vec<Box<dyn Scene>>,
    pending: Vec<SceneTransition>,
}

impl SceneStack {
    pub fn top_name(&self) -> Option<&str> {
        self.stack.last().map(|s| s.name())
    }

    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Queue a transition (applied by [`SceneStack::apply`]).
    pub fn request(&mut self, t: SceneTransition) {
        if t != SceneTransition::None {
            self.pending.push(t);
        }
    }

    /// Update the top scene, then apply its and any queued transitions.
    /// Returns true if the stack changed.
    pub fn update(&mut self, world: &mut World, registry: &SceneRegistry) -> Result<bool, String> {
        if let Some(top) = self.stack.last_mut() {
            #[cfg(feature = "hotpatch")]
            let t = top.__evani_hot_update(world);
            #[cfg(not(feature = "hotpatch"))]
            let t = top.update(world);
            self.request(t);
        }
        self.apply(world, registry)
    }

    /// Apply queued transitions in order. Unknown scene names are reported and
    /// skipped, leaving the stack as it was for that request.
    pub fn apply(&mut self, world: &mut World, registry: &SceneRegistry) -> Result<bool, String> {
        let mut changed = false;
        let mut error = None;
        let pending = core::mem::take(&mut self.pending);
        for t in pending {
            match t {
                SceneTransition::None => {}
                SceneTransition::Push(name) => match registry.create(&name) {
                    Some(mut s) => {
                        if let Some(top) = self.stack.last_mut() {
                            top.on_pause(world);
                        }
                        s.on_enter(world);
                        self.stack.push(s);
                        changed = true;
                    }
                    None => error = Some(format!("unknown scene {name}")),
                },
                SceneTransition::Pop => {
                    if let Some(mut s) = self.stack.pop() {
                        s.on_exit(world);
                        if let Some(top) = self.stack.last_mut() {
                            top.on_resume(world);
                        }
                        changed = true;
                    }
                }
                SceneTransition::Replace(name) => match registry.create(&name) {
                    Some(mut s) => {
                        if let Some(mut old) = self.stack.pop() {
                            old.on_exit(world);
                        }
                        s.on_enter(world);
                        self.stack.push(s);
                        changed = true;
                    }
                    None => error = Some(format!("unknown scene {name}")),
                },
            }
        }
        match error {
            Some(e) => Err(e),
            None => Ok(changed),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Logged {
        name: &'static str,
        then: SceneTransition,
    }

    impl Scene for Logged {
        fn name(&self) -> &str {
            self.name
        }
        fn on_enter(&mut self, w: &mut World) {
            w.res_mut::<Vec<String>>().push(format!("enter {}", self.name));
        }
        fn on_exit(&mut self, w: &mut World) {
            w.res_mut::<Vec<String>>().push(format!("exit {}", self.name));
        }
        fn on_pause(&mut self, w: &mut World) {
            w.res_mut::<Vec<String>>().push(format!("pause {}", self.name));
        }
        fn on_resume(&mut self, w: &mut World) {
            w.res_mut::<Vec<String>>().push(format!("resume {}", self.name));
        }
        fn update(&mut self, _w: &mut World) -> SceneTransition {
            core::mem::take(&mut self.then)
        }
    }

    #[test]
    fn push_pop_replace_lifecycle() {
        let mut w = World::new();
        w.insert_resource(Vec::<String>::new());
        let mut reg = SceneRegistry::default();
        reg.register("menu", || {
            Box::new(Logged {
                name: "menu",
                then: SceneTransition::Push("pause".into()),
            })
        });
        reg.register("pause", || {
            Box::new(Logged {
                name: "pause",
                then: SceneTransition::Pop,
            })
        });
        reg.register("game", || {
            Box::new(Logged {
                name: "game",
                then: SceneTransition::None,
            })
        });
        let mut s = SceneStack::default();
        s.request(SceneTransition::Push("menu".into()));
        assert!(s.apply(&mut w, &reg).unwrap());
        assert!(s.update(&mut w, &reg).unwrap()); // menu pushes pause
        assert_eq!(s.top_name(), Some("pause"));
        assert!(s.update(&mut w, &reg).unwrap()); // pause pops itself
        assert_eq!(s.top_name(), Some("menu"));
        s.request(SceneTransition::Replace("game".into()));
        s.apply(&mut w, &reg).unwrap();
        assert_eq!(s.top_name(), Some("game"));
        assert_eq!(s.depth(), 1);
        assert_eq!(
            *w.res::<Vec<String>>(),
            vec![
                "enter menu",
                "pause menu",
                "enter pause",
                "exit pause",
                "resume menu",
                "exit menu",
                "enter game"
            ]
        );
        s.request(SceneTransition::Push("nope".into()));
        assert!(s.apply(&mut w, &reg).is_err());
        assert_eq!(s.top_name(), Some("game"));
    }
}
