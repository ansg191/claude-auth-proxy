use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use bimap::BiHashMap;
use sha2::{Digest, Sha256};

const TOOL_NAME_PREFIX: &str = "t_";

#[derive(Debug)]
pub struct ToolNameMapper {
    min_hash_len: usize,
    max_hash_len: usize,
    mappings: RwLock<BiHashMap<String, String>>,
}

impl ToolNameMapper {
    pub fn new(min_hash_len: usize, max_hash_len: usize) -> Self {
        Self {
            min_hash_len,
            max_hash_len,
            mappings: RwLock::new(BiHashMap::new()),
        }
    }

    pub const fn min_hash_len(&self) -> usize {
        self.min_hash_len
    }

    pub const fn max_hash_len(&self) -> usize {
        self.max_hash_len
    }

    pub fn obfuscate(&self, original: &str) -> String {
        if let Some(existing) = self.mappings_read().get_by_right(original) {
            return existing.clone();
        }

        let mut mappings = self.mappings_write();

        if let Some(existing) = mappings.get_by_right(original) {
            return existing.clone();
        }

        for salt in 0_u32.. {
            let hex = hashed_name(original, salt);
            let max_len = self.max_hash_len.min(hex.len());
            for len in self.min_hash_len..=max_len {
                let candidate = format!("{TOOL_NAME_PREFIX}{}", &hex[..len]);
                if !matches!(mappings.get_by_left(&candidate), Some(existing) if existing != original)
                {
                    mappings.insert(candidate.clone(), original.to_owned());
                    return candidate;
                }
            }
        }

        unreachable!("collision search should always find a free tool name")
    }

    pub fn deobfuscate(&self, obfuscated: &str) -> String {
        self.mappings_read()
            .get_by_left(obfuscated)
            .cloned()
            .unwrap_or_else(|| obfuscated.to_owned())
    }

    fn mappings_read(&self) -> RwLockReadGuard<'_, BiHashMap<String, String>> {
        self.mappings
            .read()
            .expect("tool-name mapper read lock poisoned")
    }

    fn mappings_write(&self) -> RwLockWriteGuard<'_, BiHashMap<String, String>> {
        self.mappings
            .write()
            .expect("tool-name mapper write lock poisoned")
    }
}

fn hashed_name(original: &str, salt: u32) -> String {
    let mut hasher = Sha256::new();
    hasher.update(original.as_bytes());
    hasher.update([0]);
    hasher.update(salt.to_be_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn obfuscate_round_trips_to_original_name() {
        let mapper = ToolNameMapper::new(8, 64);

        let obfuscated = mapper.obfuscate("background_output");

        assert!(obfuscated.starts_with("t_"));
        assert_eq!(mapper.deobfuscate(&obfuscated), "background_output");
    }

    #[test]
    fn obfuscate_reuses_existing_mapping_for_same_name() {
        let mapper = ToolNameMapper::new(8, 64);

        let first = mapper.obfuscate("todowrite");
        let second = mapper.obfuscate("todowrite");

        assert_eq!(first, second);
    }

    #[test]
    fn obfuscate_expands_hash_length_to_avoid_collisions() {
        let mapper = ToolNameMapper::new(1, 64);
        let original = "background_output";
        let hex = hashed_name(original, 0);
        let colliding_candidate = format!("{TOOL_NAME_PREFIX}{}", &hex[..1]);

        mapper
            .mappings_write()
            .insert(colliding_candidate.clone(), "other_tool".to_owned());

        let obfuscated = mapper.obfuscate(original);

        assert_ne!(obfuscated, colliding_candidate);
        assert!(obfuscated.starts_with("t_"));
        assert!(obfuscated.len() > colliding_candidate.len());
        assert_eq!(mapper.deobfuscate(&obfuscated), original);
    }

    #[test]
    fn obfuscate_respects_max_hash_len() {
        let mapper = ToolNameMapper::new(4, 4);

        let obfuscated = mapper.obfuscate("background_output");

        assert_eq!(obfuscated.len(), TOOL_NAME_PREFIX.len() + 4);
    }
}
