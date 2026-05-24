use std::fs;
use std::io::Write as _;
use std::path::Path;

/// A user-placed text annotation on the map, stored in EQ world coordinates.
#[derive(Debug, Clone)]
pub struct MapAnnotation {
    pub id: u32,
    pub text: String,
    /// EQ world coordinates (not negated — rendered via eq_to_map like spawns).
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub color: [u8; 3],
    pub size: u8,
}

/// Annotation overlay for the current zone.
#[derive(Debug, Default)]
pub struct AnnotationStore {
    pub items: Vec<MapAnnotation>,
    next_id: u32,
}

impl AnnotationStore {
    pub fn add(&mut self, text: String, x: f32, y: f32, z: f32, color: [u8; 3], size: u8) {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(MapAnnotation {
            id,
            text,
            x,
            y,
            z,
            color,
            size,
        });
    }

    #[allow(dead_code)]
    pub fn remove(&mut self, id: u32) {
        self.items.retain(|a| a.id != id);
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.items.clear();
        self.next_id = 0;
    }

    /// Load annotations for `zone` from `{dir}/annotations-{zone}.txt`.
    pub fn load(zone: &str, dir: &str) -> Self {
        let path = annotation_path(zone, dir);
        let mut store = Self::default();
        let Ok(text) = fs::read_to_string(&path) else {
            return store;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(ann) = parse_line(line) {
                store.next_id = store.next_id.max(ann.id + 1);
                store.items.push(ann);
            }
        }
        store
    }

    /// Save annotations for `zone` to `{dir}/annotations-{zone}.txt`.
    pub fn save(&self, zone: &str, dir: &str) -> std::io::Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }
        let path = annotation_path(zone, dir);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::File::create(&path)?;
        for a in &self.items {
            writeln!(
                f,
                "{};{};{};{};{};{};{};{};{}",
                a.id, a.text, a.x, a.y, a.z, a.color[0], a.color[1], a.color[2], a.size
            )?;
        }
        Ok(())
    }
}

fn annotation_path(zone: &str, dir: &str) -> std::path::PathBuf {
    Path::new(dir).join(format!("annotations-{zone}.txt"))
}

fn parse_line(line: &str) -> Option<MapAnnotation> {
    let mut parts = line.splitn(9, ';');
    let id: u32 = parts.next()?.parse().ok()?;
    let text = parts.next()?.to_owned();
    let x: f32 = parts.next()?.parse().ok()?;
    let y: f32 = parts.next()?.parse().ok()?;
    let z: f32 = parts.next()?.parse().ok()?;
    let r: u8 = parts.next()?.parse().ok()?;
    let g: u8 = parts.next()?.parse().ok()?;
    let b: u8 = parts.next()?.parse().ok()?;
    let size: u8 = parts.next().and_then(|s| s.parse().ok()).unwrap_or(12);
    Some(MapAnnotation {
        id,
        text,
        x,
        y,
        z,
        color: [r, g, b],
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_remove() {
        let mut store = AnnotationStore::default();
        store.add("Test note".to_owned(), 100.0, 200.0, 0.0, [255, 255, 0], 12);
        assert_eq!(store.items.len(), 1);
        let id = store.items[0].id;
        store.remove(id);
        assert!(store.items.is_empty());
    }

    #[test]
    fn ids_increment() {
        let mut store = AnnotationStore::default();
        store.add("A".to_owned(), 0.0, 0.0, 0.0, [255, 255, 255], 12);
        store.add("B".to_owned(), 0.0, 0.0, 0.0, [255, 255, 255], 12);
        assert_eq!(store.items[0].id, 0);
        assert_eq!(store.items[1].id, 1);
    }

    #[test]
    fn save_and_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();

        let mut store = AnnotationStore::default();
        store.add("Banker".to_owned(), 100.0, 200.0, 5.0, [255, 255, 0], 12);
        store.add("Safe camp".to_owned(), -50.0, 80.0, 0.0, [0, 255, 0], 14);
        store.save("commonlands", &dir_str).unwrap();

        let loaded = AnnotationStore::load("commonlands", &dir_str);
        assert_eq!(loaded.items.len(), 2);
        assert_eq!(loaded.items[0].text, "Banker");
        assert_eq!(loaded.items[1].color, [0, 255, 0]);
        assert_eq!(loaded.items[1].size, 14);
        assert!((loaded.items[0].x - 100.0).abs() < 0.01);
    }

    #[test]
    fn load_missing_returns_empty() {
        let store = AnnotationStore::load("unknownzone", "/nonexistent");
        assert!(store.items.is_empty());
    }

    #[test]
    fn next_id_restored_on_load() {
        let dir = tempfile::tempdir().unwrap();
        let dir_str = dir.path().to_string_lossy().into_owned();

        let mut store = AnnotationStore::default();
        store.add("A".to_owned(), 0.0, 0.0, 0.0, [255, 255, 255], 12);
        store.add("B".to_owned(), 0.0, 0.0, 0.0, [255, 255, 255], 12);
        store.save("testzone", &dir_str).unwrap();

        let mut loaded = AnnotationStore::load("testzone", &dir_str);
        loaded.add("C".to_owned(), 0.0, 0.0, 0.0, [255, 255, 255], 12);
        // New annotation should get id 2, not 0
        assert_eq!(loaded.items[2].id, 2);
    }
}
