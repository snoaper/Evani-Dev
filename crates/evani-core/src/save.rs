//! Save/load as JSON in `localStorage` (memory store natively and in tests).
//! Keys are namespaced per game: `evani:<game>:<key>`.

use std::collections::BTreeMap;

use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveError {
    /// Storage missing (private mode, disabled) or quota exceeded.
    Unavailable(String),
    Json(String),
}

impl core::fmt::Display for SaveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SaveError::Unavailable(e) => write!(f, "storage unavailable: {e}"),
            SaveError::Json(e) => write!(f, "save data: {e}"),
        }
    }
}

/// A string key-value store.
pub trait KeyValueStore {
    fn get(&self, key: &str) -> Result<Option<String>, SaveError>;
    fn set(&mut self, key: &str, value: &str) -> Result<(), SaveError>;
    fn remove(&mut self, key: &str) -> Result<(), SaveError>;
}

/// In-memory store (native, tests, and fallback when localStorage is blocked).
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    map: BTreeMap<String, String>,
}

impl KeyValueStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>, SaveError> {
        Ok(self.map.get(key).cloned())
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), SaveError> {
        self.map.insert(key.to_string(), value.to_string());
        Ok(())
    }
    fn remove(&mut self, key: &str) -> Result<(), SaveError> {
        self.map.remove(key);
        Ok(())
    }
}

/// World resource: namespaced save slots on top of a store.
pub struct SaveData {
    prefix: String,
    store: Box<dyn KeyValueStore>,
}

impl SaveData {
    pub fn new(game: &str, store: Box<dyn KeyValueStore>) -> Self {
        Self {
            prefix: format!("evani:{game}:"),
            store,
        }
    }

    /// `localStorage` on wasm (falling back to memory if blocked), memory
    /// natively.
    pub fn for_platform(game: &str) -> Self {
        #[cfg(all(target_arch = "wasm32", not(evani_headless)))]
        {
            match crate::web::LocalStore::open() {
                Some(s) => Self::new(game, Box::new(s)),
                None => Self::new(game, Box::new(MemoryStore::default())),
            }
        }
        #[cfg(not(all(target_arch = "wasm32", not(evani_headless))))]
        {
            Self::new(game, Box::new(MemoryStore::default()))
        }
    }

    fn key(&self, key: &str) -> String {
        format!("{}{key}", self.prefix)
    }

    pub fn save<T: Serialize>(&mut self, key: &str, value: &T) -> Result<(), SaveError> {
        let json = serde_json::to_string(value).map_err(|e| SaveError::Json(e.to_string()))?;
        let k = self.key(key);
        self.store.set(&k, &json)
    }

    /// `Ok(None)` when nothing is saved under `key`.
    pub fn load<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, SaveError> {
        match self.store.get(&self.key(key))? {
            None => Ok(None),
            Some(s) => serde_json::from_str(&s)
                .map(Some)
                .map_err(|e| SaveError::Json(e.to_string())),
        }
    }

    pub fn save_raw(&mut self, key: &str, value: &str) -> Result<(), SaveError> {
        let k = self.key(key);
        self.store.set(&k, value)
    }

    pub fn load_raw(&self, key: &str) -> Result<Option<String>, SaveError> {
        self.store.get(&self.key(key))
    }

    pub fn delete(&mut self, key: &str) -> Result<(), SaveError> {
        let k = self.key(key);
        self.store.remove(&k)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Progress {
        level: u32,
        name: String,
    }

    #[test]
    fn round_trip_and_namespacing() {
        let mut s = SaveData::new("demo", Box::new(MemoryStore::default()));
        assert_eq!(s.load::<Progress>("slot1").unwrap(), None);
        let p = Progress {
            level: 3,
            name: "kart".into(),
        };
        s.save("slot1", &p).unwrap();
        assert_eq!(s.load::<Progress>("slot1").unwrap(), Some(p));
        s.save_raw("bad", "{not json").unwrap();
        assert!(matches!(s.load::<Progress>("bad"), Err(SaveError::Json(_))));
        s.delete("slot1").unwrap();
        assert_eq!(s.load::<Progress>("slot1").unwrap(), None);
        assert_eq!(s.key("x"), "evani:demo:x");
    }
}
