use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MapPoint {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone)]
pub struct MapLine {
    pub p1: MapPoint,
    pub p2: MapPoint,
    pub color: [u8; 3],
    /// Layer index: 0 = base, 1–3 = zone_N.txt numbered layers.
    pub layer: u8,
}

#[derive(Debug, Clone)]
pub struct MapLabel {
    pub pos: MapPoint,
    pub text: String,
    pub color: [u8; 3],
    pub size: u8,
    /// Layer index: 0 = base, 1–3 = zone_N.txt numbered layers.
    pub layer: u8,
}

#[derive(Debug, Default, Clone)]
pub struct MapData {
    pub lines: Vec<MapLine>,
    pub labels: Vec<MapLabel>,
}

impl MapData {
    pub fn merge(&mut self, other: MapData) {
        self.lines.extend(other.lines);
        self.labels.extend(other.labels);
    }

    /// Axis-aligned bounding box over all line endpoints. Returns None for empty maps.
    #[allow(dead_code)]
    pub fn bounding_box(&self) -> Option<(MapPoint, MapPoint) > {
        let mut pts = self.lines.iter().flat_map(|l| [l.p1, l.p2]);
        let first = pts.next()?;
        let (mut min, mut max) = (first, first);
        for p in pts {
            if p.x < min.x { min.x = p.x; }
            if p.y < min.y { min.y = p.y; }
            if p.z < min.z { min.z = p.z; }
            if p.x > max.x { max.x = p.x; }
            if p.y > max.y { max.y = p.y; }
            if p.z > max.z { max.z = p.z; }
        }
        Some((min, max))
    }
}

#[derive(Debug)]
pub enum MapError {
    Io(std::io::Error),
    NoLayersFound,
}

impl fmt::Display for MapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MapError::Io(e) => write!(f, "map I/O error: {e}"),
            MapError::NoLayersFound => write!(f, "no map layer files found for zone"),
        }
    }
}

impl From<std::io::Error> for MapError {
    fn from(e: std::io::Error) -> Self {
        MapError::Io(e)
    }
}

/// Parse a single native EQ map layer file (`zonename_N.txt`).
///
/// Format:
/// ```text
/// L x1,y1,z1,x2,y2,z2,r,g,b
/// P x,y,z,r,g,b,size,label text
/// ```
/// X and Y are negated on load to match the MySEQ display coordinate convention.
/// Unrecognised lines are silently skipped.
pub fn load_layer(path: &Path, layer: u8) -> Result<MapData, MapError> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    parse_lines(reader.lines().map_while(Result::ok), layer)
}

/// Parse native EQ map layer text from an in-memory string (useful for tests).
#[allow(dead_code)]
pub fn parse_str(src: &str) -> MapData {
    parse_lines(src.lines().map(str::to_owned), 0).unwrap_or_default()
}

fn parse_lines(lines: impl Iterator<Item = String>, layer: u8) -> Result<MapData, MapError> {
    let mut data = MapData::default();
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match line.as_bytes().first() {
            Some(b'L') | Some(b'l') => {
                if let Some(mut ml) = parse_map_line(&line[1..]) {
                    ml.layer = layer;
                    data.lines.push(ml);
                }
            }
            Some(b'P') | Some(b'p') => {
                if let Some(mut lbl) = parse_map_label(&line[1..]) {
                    lbl.layer = layer;
                    data.labels.push(lbl);
                }
            }
            _ => {}
        }
    }
    Ok(data)
}

/// Try all three layers for a zone; merge those that exist.
/// Returns `MapError::NoLayersFound` if not a single layer file is present.
pub fn load_zone(dir: &Path, zone: &str) -> Result<MapData, MapError> {
    let mut combined = MapData::default();
    let mut found = false;
    // Base layer (layer 0, no suffix) + up to three numbered layers (1–3).
    let candidates: Vec<(std::path::PathBuf, u8)> =
        std::iter::once((dir.join(format!("{zone}.txt")), 0u8))
            .chain((1..=3u8).map(|n| (dir.join(format!("{zone}_{n}.txt")), n)))
            .collect();
    for (path, layer) in candidates {
        if path.exists() {
            combined.merge(load_layer(&path, layer)?);
            found = true;
        }
    }
    if found { Ok(combined) } else { Err(MapError::NoLayersFound) }
}

// --- internal parsers ---

fn parse_map_line(rest: &str) -> Option<MapLine> {
    let mut t = Tokenizer::new(rest);
    let x1 = t.f32()?;
    let y1 = -t.f32()?;
    let z1 = t.f32()?;
    let x2 = t.f32()?;
    let y2 = -t.f32()?;
    let z2 = t.f32()?;
    let r = t.u8()?;
    let g = t.u8()?;
    let b = t.u8()?;
    Some(MapLine {
        p1: MapPoint { x: x1, y: y1, z: z1 },
        p2: MapPoint { x: x2, y: y2, z: z2 },
        color: [r, g, b],
        layer: 0,
    })
}

fn parse_map_label(rest: &str) -> Option<MapLabel> {
    let mut t = Tokenizer::new(rest);
    let x = t.f32()?;
    let y = -t.f32()?;
    let z = t.f32()?;
    let r = t.u8()?;
    let g = t.u8()?;
    let b = t.u8()?;
    let size = t.u8()?;
    // remainder of the line (after size) is the label text, comma-separated or plain
    let text = t.remainder().trim().to_owned();
    Some(MapLabel {
        pos: MapPoint { x, y, z },
        text,
        color: [r, g, b],
        size,
        layer: 0,
    })
}

/// Simple comma-and-whitespace tokenizer over a string slice.
struct Tokenizer<'a> {
    src: &'a str,
}

impl<'a> Tokenizer<'a> {
    fn new(src: &'a str) -> Self {
        Self { src: src.trim_start_matches(|c: char| c == ',' || c.is_whitespace()) }
    }

    fn next_token(&mut self) -> Option<&str> {
        let s = self.src.trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        if s.is_empty() {
            self.src = s;
            return None;
        }
        let end = s.find(|c: char| c == ',' || c.is_whitespace()).unwrap_or(s.len());
        let token = &s[..end];
        self.src = s[end..].trim_start_matches(|c: char| c == ',' || c.is_whitespace());
        Some(token)
    }

    fn f32(&mut self) -> Option<f32> {
        self.next_token()?.parse().ok()
    }

    fn u8(&mut self) -> Option<u8> {
        let v: f32 = self.next_token()?.parse().ok()?;
        Some(v.clamp(0.0, 255.0) as u8)
    }

    fn remainder(&self) -> &str {
        self.src
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
L -100,-200,0,100,-200,0,255,0,0
L -100,-200,0,-100,200,0,0,255,0
P -75,-150,0,125,0,125,2,Entrance
P 0,0,50,0,0,255,1,Center
# this is a comment and should be ignored
unknown line here
";

    #[test]
    fn parses_line_count() {
        let d = parse_str(SAMPLE);
        assert_eq!(d.lines.len(), 2);
    }

    #[test]
    fn parses_label_count() {
        let d = parse_str(SAMPLE);
        assert_eq!(d.labels.len(), 2);
    }

    #[test]
    fn line_coords_y_negated() {
        let d = parse_str(SAMPLE);
        // Input: L -100,-200,0,100,-200,0 → Y negated only: (-100, 200) and (100, 200)
        let l = &d.lines[0];
        assert!((l.p1.x - (-100.0)).abs() < 0.01);
        assert!((l.p1.y - 200.0).abs() < 0.01);
        assert!((l.p1.z - 0.0).abs() < 0.01);
        assert!((l.p2.x - 100.0).abs() < 0.01);
        assert!((l.p2.y - 200.0).abs() < 0.01);
    }

    #[test]
    fn line_color() {
        let d = parse_str(SAMPLE);
        assert_eq!(d.lines[0].color, [255, 0, 0]);
        assert_eq!(d.lines[1].color, [0, 255, 0]);
    }

    #[test]
    fn label_text_and_size() {
        let d = parse_str(SAMPLE);
        assert_eq!(d.labels[0].text, "Entrance");
        assert_eq!(d.labels[0].size, 2);
        assert_eq!(d.labels[1].text, "Center");
        assert_eq!(d.labels[1].size, 1);
    }

    #[test]
    fn label_coords_y_negated() {
        let d = parse_str(SAMPLE);
        // Input: P -75,-150,0 → Y negated only: (-75, 150, 0)
        let lbl = &d.labels[0];
        assert!((lbl.pos.x - (-75.0)).abs() < 0.01);
        assert!((lbl.pos.y - 150.0).abs() < 0.01);
    }

    #[test]
    fn label_color() {
        let d = parse_str(SAMPLE);
        assert_eq!(d.labels[0].color, [125, 0, 125]);
    }

    #[test]
    fn bounding_box_correct() {
        let d = parse_str(SAMPLE);
        let (min, max) = d.bounding_box().unwrap();
        // lines: (-100,200,0)↔(100,200,0) and (-100,200,0)↔(-100,-200,0)
        assert!((min.y - (-200.0)).abs() < 0.01);
        assert!((max.y - 200.0).abs() < 0.01);
        assert!((min.x - (-100.0)).abs() < 0.01);
        assert!((max.x - 100.0).abs() < 0.01);
    }

    #[test]
    fn empty_map_has_no_bounding_box() {
        assert!(MapData::default().bounding_box().is_none());
    }

    #[test]
    fn merge_combines_layers() {
        let a = parse_str("L 0,0,0,1,1,1,0,0,0\n");
        let b = parse_str("L 2,2,2,3,3,3,0,0,0\nP 0,0,0,0,0,0,1,Label\n");
        let mut combined = a;
        combined.merge(b);
        assert_eq!(combined.lines.len(), 2);
        assert_eq!(combined.labels.len(), 1);
    }

    #[test]
    fn unknown_tokens_skipped() {
        let d = parse_str("X garbage\nL 0,0,0,1,1,1,255,255,255\n");
        assert_eq!(d.lines.len(), 1);
    }

    #[test]
    fn load_zone_missing_returns_error() {
        let tmp = std::env::temp_dir();
        assert!(matches!(
            load_zone(&tmp, "nonexistentzone_xyz"),
            Err(MapError::NoLayersFound)
        ));
    }

    #[test]
    fn load_zone_reads_layers() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        // base layer + _1 layer = 2 lines total
        let mut f = File::create(dir.path().join("testzone.txt")).unwrap();
        writeln!(f, "L 0,0,0,10,10,0,255,0,0").unwrap();
        let mut f = File::create(dir.path().join("testzone_1.txt")).unwrap();
        writeln!(f, "L 1,1,0,2,2,0,0,255,0").unwrap();
        let data = load_zone(dir.path(), "testzone").unwrap();
        assert_eq!(data.lines.len(), 2);
    }

    #[test]
    fn load_zone_base_only() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let mut f = File::create(dir.path().join("testzone2.txt")).unwrap();
        writeln!(f, "L 0,0,0,1,1,0,255,0,0").unwrap();
        let data = load_zone(dir.path(), "testzone2").unwrap();
        assert_eq!(data.lines.len(), 1);
    }
}