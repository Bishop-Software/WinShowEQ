use std::collections::HashMap;

use common::{OPT_GROUND, SpawnRecord};

/// A ground item decoded from an OPT_GROUND SpawnRecord.
#[derive(Debug, Clone)]
pub struct GroundItem {
    pub id: u32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl GroundItem {
    pub fn from_record(rec: &SpawnRecord) -> Option<Self> {
        let (flags, id, x, y, z) = (rec.flags, rec.id, rec.x, rec.y, rec.z);
        if flags != OPT_GROUND {
            return None;
        }
        let end = rec.name.iter().position(|&b| b == 0).unwrap_or(rec.name.len());
        let name = String::from_utf8_lossy(&rec.name[..end]).into_owned();
        Some(Self { id, name, x, y, z })
    }
}

/// Active ground items keyed by item id.
#[derive(Debug, Default)]
pub struct GroundStore {
    items: HashMap<u32, GroundItem>,
}

impl GroundStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or replace the ground item described by `rec`.
    /// Silently ignores records that are not OPT_GROUND.
    pub fn upsert(&mut self, rec: &SpawnRecord) {
        if let Some(item) = GroundItem::from_record(rec) {
            self.items.insert(item.id, item);
        }
    }

    pub fn remove(&mut self, id: u32) {
        self.items.remove(&id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn get(&self, id: u32) -> Option<&GroundItem> {
        self.items.get(&id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &GroundItem> {
        self.items.values()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ground_record(id: u32, name: &str, x: f32, y: f32, z: f32) -> SpawnRecord {
        let mut rec = SpawnRecord::zeroed();
        rec.id = id;
        rec.x = x;
        rec.y = y;
        rec.z = z;
        rec.flags = OPT_GROUND;
        let bytes = name.as_bytes();
        let copy = bytes.len().min(rec.name.len() - 1);
        rec.name[..copy].copy_from_slice(&bytes[..copy]);
        rec
    }

    #[test]
    fn upsert_and_retrieve() {
        let mut store = GroundStore::new();
        let rec = make_ground_record(42, "ITEM0001", 100.0, 200.0, 0.0);
        store.upsert(&rec);
        let item = store.get(42).unwrap();
        assert_eq!(item.name, "ITEM0001");
        assert_eq!(item.x, 100.0);
    }

    #[test]
    fn ignores_non_ground_record() {
        let mut store = GroundStore::new();
        let mut rec = SpawnRecord::zeroed();
        rec.flags = common::OPT_SPAWNS;
        store.upsert(&rec);
        assert!(store.is_empty());
    }

    #[test]
    fn clear_empties_store() {
        let mut store = GroundStore::new();
        store.upsert(&make_ground_record(1, "IT0001", 0.0, 0.0, 0.0));
        store.upsert(&make_ground_record(2, "IT0002", 0.0, 0.0, 0.0));
        assert_eq!(store.len(), 2);
        store.clear();
        assert!(store.is_empty());
    }
}