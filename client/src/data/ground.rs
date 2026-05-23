use common::{OPT_GROUND, SpawnRecord};

/// A ground item decoded from an OPT_GROUND SpawnRecord.
#[derive(Debug, Clone)]
pub struct GroundItem {
    #[allow(dead_code)]
    pub id: u32,
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl GroundItem {
    pub fn from_record(rec: &SpawnRecord) -> Option<Self> {
        if rec.flags != OPT_GROUND {
            return None;
        }
        let end = rec
            .name
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(rec.name.len());
        let name = String::from_utf8_lossy(&rec.name[..end]).into_owned();
        Some(Self {
            id: rec.id,
            name,
            x: rec.y,
            y: rec.x,
            z: rec.z,
        })
    }
}

/// Active ground items. Replaced wholesale each tick via clear() + push().
#[derive(Debug, Default)]
pub struct GroundStore {
    items: Vec<GroundItem>,
}

impl GroundStore {
    #[allow(dead_code)]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append the ground item described by `rec`. Silently ignores non-OPT_GROUND records.
    /// Caller must call clear() before the first push() of each tick.
    pub fn push(&mut self, rec: &SpawnRecord) {
        if let Some(item) = GroundItem::from_record(rec) {
            self.items.push(item);
        }
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn iter(&self) -> impl Iterator<Item = &GroundItem> {
        self.items.iter()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[allow(dead_code)]
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
    fn push_and_iterate() {
        let mut store = GroundStore::new();
        store.push(&make_ground_record(0, "IT0001", 100.0, 200.0, 0.0));
        store.push(&make_ground_record(0, "IT0002", 300.0, 400.0, 0.0));
        assert_eq!(store.len(), 2);
        let names: Vec<_> = store.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"IT0001"));
        assert!(names.contains(&"IT0002"));
    }

    #[test]
    fn ignores_non_ground_record() {
        let mut store = GroundStore::new();
        let mut rec = SpawnRecord::zeroed();
        rec.flags = common::OPT_SPAWNS;
        store.push(&rec);
        assert!(store.is_empty());
    }

    #[test]
    fn clear_empties_store() {
        let mut store = GroundStore::new();
        store.push(&make_ground_record(0, "IT0001", 0.0, 0.0, 0.0));
        store.push(&make_ground_record(0, "IT0002", 0.0, 0.0, 0.0));
        assert_eq!(store.len(), 2);
        store.clear();
        assert!(store.is_empty());
    }
}
