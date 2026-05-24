use std::collections::HashMap;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;

/// Filter category. Priority for overlapping matches: Danger > Caution > Hunt > Rare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FilterCategory {
    Hunt,
    Caution,
    Danger,
    Rare,
}

impl FilterCategory {
    fn priority(self) -> u8 {
        match self {
            Self::Danger => 3,
            Self::Caution => 2,
            Self::Hunt => 1,
            Self::Rare => 0,
        }
    }

    fn xml_tag(self) -> &'static str {
        match self {
            Self::Hunt => "hunt",
            Self::Caution => "caution",
            Self::Danger => "danger",
            Self::Rare => "rare",
        }
    }
}

/// A set of named spawn filters organized by category.
/// Load from `seqfilters` XML files (global and/or zone-specific).
#[derive(Debug, Default, Clone)]
pub struct FilterSet {
    /// Lowercase name → highest-priority matching category.
    entries: HashMap<String, FilterCategory>,
}

impl FilterSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load from a `seqfilters` XML file. Missing files return an empty set.
    ///
    /// Supports both the current format and the legacy C# MySEQ format automatically.
    /// If a legacy file is detected it is migrated to the current format in place.
    ///
    /// Current format:
    /// ```xml
    /// <seqfilters>
    ///   <hunt><item name="Fippy Darkpaw" /></hunt>
    ///   <danger><item name="Nagafen" /></danger>
    /// </seqfilters>
    /// ```
    ///
    /// Legacy C# format:
    /// ```xml
    /// <seqfilters>
    ///   <section name="Hunt">
    ///     <oldfilter><regex>Name:Fippy Darkpaw</regex></oldfilter>
    ///   </section>
    ///   <section name="Alert">
    ///     <oldfilter><regex>Name:Nagafen</regex></oldfilter>
    ///   </section>
    /// </seqfilters>
    /// ```
    pub fn load(path: &Path) -> Self {
        let mut set = Self::new();
        if !path.exists() {
            return set;
        }
        match set.load_file(path) {
            Ok(true) => {
                // Legacy format — rewrite in place so future loads are fast
                if let Err(e) = set.save(path) {
                    eprintln!("FilterSet: failed to migrate {:?}: {e}", path);
                }
            }
            Ok(false) => {}
            Err(e) => eprintln!("FilterSet: failed to load {:?}: {e}", path),
        }
        set
    }

    /// Returns `true` if the file used the legacy C# section format (caller should re-save).
    fn load_file(&mut self, path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
        let mut reader = Reader::from_file(path)?;
        reader.config_mut().trim_text(true);
        let mut buf = Vec::new();
        let mut current: Option<FilterCategory> = None;
        let mut is_legacy = false;

        loop {
            match reader.read_event_into(&mut buf)? {
                Event::Start(e) => match e.name().as_ref() {
                    // Current format category tags
                    b"hunt" => current = Some(FilterCategory::Hunt),
                    b"caution" => current = Some(FilterCategory::Caution),
                    b"danger" => current = Some(FilterCategory::Danger),
                    b"rare" => current = Some(FilterCategory::Rare),
                    b"item" => self.insert_from_element(&e, current),
                    // Legacy format: <section name="Hunt|Caution|Danger|Alert|Locate|...">
                    b"section" => {
                        is_legacy = true;
                        current = section_category(&e);
                    }
                    _ => {}
                },
                Event::Empty(e) if e.name().as_ref() == b"item" => {
                    self.insert_from_element(&e, current);
                }
                // Legacy format: text content inside <oldfilter><regex>Name:xxx</regex></oldfilter>
                Event::Text(e) if is_legacy => {
                    if let Some(cat) = current {
                        let raw = std::str::from_utf8(e.as_ref()).unwrap_or("").trim();
                        if let Some(name) = extract_old_filter_name(raw) {
                            self.add(cat, name);
                        }
                    }
                }
                Event::End(e) => match e.name().as_ref() {
                    b"hunt" | b"caution" | b"danger" | b"rare" | b"section" => current = None,
                    _ => {}
                },
                Event::Eof => break,
                _ => {}
            }
            buf.clear();
        }
        Ok(is_legacy)
    }

    fn insert_from_element(
        &mut self,
        e: &quick_xml::events::BytesStart<'_>,
        category: Option<FilterCategory>,
    ) {
        let Some(cat) = category else { return };
        for attr in e.attributes().flatten() {
            if attr.key.as_ref() == b"name" {
                let name = attr
                    .unescape_value()
                    .map(|v| v.trim().to_lowercase())
                    .unwrap_or_default();
                if !name.is_empty() {
                    self.add(cat, name);
                }
                break;
            }
        }
    }

    /// Add a named entry to the set (case-insensitive). Higher-priority categories
    /// overwrite lower-priority ones for the same name.
    pub fn add(&mut self, category: FilterCategory, name: impl Into<String>) {
        let key = name.into().to_lowercase();
        let entry = self.entries.entry(key).or_insert(category);
        if category.priority() > entry.priority() {
            *entry = category;
        }
    }

    /// Merge another FilterSet into this one (higher-priority wins on conflict).
    #[allow(dead_code)]
    pub fn merge(&mut self, other: FilterSet) {
        for (name, cat) in other.entries {
            self.add(cat, name);
        }
    }

    /// Return the filter category for `name`, or None if unfiltered.
    /// Matches if any filter entry is a case-insensitive substring of `name`.
    /// When multiple entries match, the highest-priority category wins.
    pub fn classify(&self, name: &str) -> Option<FilterCategory> {
        let lower = name.to_lowercase();
        self.entries
            .iter()
            .filter(|(entry, _)| lower.contains(entry.as_str()))
            .map(|(_, &cat)| cat)
            .max_by_key(|cat| cat.priority())
    }

    /// Remove an entry by name (case-insensitive).
    #[allow(dead_code)]
    pub fn remove(&mut self, name: &str) {
        self.entries.remove(&name.to_lowercase());
    }

    /// Serialize to a `seqfilters` XML file.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if let Some(dir) = path.parent() {
            write_dtd_if_needed(dir)?;
        }
        let mut f = std::fs::File::create(path)?;
        writeln!(f, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(f, r#"<!DOCTYPE seqfilters SYSTEM "seqfilters.dtd">"#)?;
        writeln!(f, "<seqfilters>")?;
        for cat in [
            FilterCategory::Hunt,
            FilterCategory::Caution,
            FilterCategory::Danger,
            FilterCategory::Rare,
        ] {
            let mut names: Vec<&String> = self
                .entries
                .iter()
                .filter(|&(_, &v)| v == cat)
                .map(|(k, _)| k)
                .collect();
            if names.is_empty() {
                continue;
            }
            names.sort();
            let tag = cat.xml_tag();
            writeln!(f, "  <{tag}>")?;
            for name in names {
                writeln!(f, "    <item name=\"{}\" />", xml_escape(name))?;
            }
            writeln!(f, "  </{tag}>")?;
        }
        writeln!(f, "</seqfilters>")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

const DTD_CONTENT: &str = "\
<!ELEMENT seqfilters (hunt?, caution?, danger?, rare?)>\n\
<!ELEMENT hunt (item*)>\n\
<!ELEMENT caution (item*)>\n\
<!ELEMENT danger (item*)>\n\
<!ELEMENT rare (item*)>\n\
<!ELEMENT item EMPTY>\n\
<!ATTLIST item name CDATA #REQUIRED>\n";

/// Write `seqfilters.dtd` into `dir` if it does not already exist.
fn write_dtd_if_needed(dir: &std::path::Path) -> std::io::Result<()> {
    let dtd_path = dir.join("seqfilters.dtd");
    if !dtd_path.exists() {
        std::fs::write(&dtd_path, DTD_CONTENT)?;
    }
    Ok(())
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Map a `<section name="...">` element to a FilterCategory (legacy C# format).
/// Hunt→Hunt, Caution→Caution, Danger→Danger, Alert/Locate→Rare; others return None.
fn section_category(e: &quick_xml::events::BytesStart<'_>) -> Option<FilterCategory> {
    for attr in e.attributes().flatten() {
        if attr.key.as_ref() == b"name" {
            let val = attr.unescape_value().ok()?;
            return match val.to_lowercase().as_str() {
                "hunt" => Some(FilterCategory::Hunt),
                "caution" => Some(FilterCategory::Caution),
                "danger" => Some(FilterCategory::Danger),
                "alert" | "locate" => Some(FilterCategory::Rare),
                _ => None,
            };
        }
    }
    None
}

/// Extract a spawn name from legacy `<regex>Name:xxx</regex>` text content.
/// Strips the `Name:` prefix (case-insensitive). Skips entries containing regex
/// special chars (`[`, `:`, `^`, `*`) that can't be represented as plain names,
/// matching C# filter behavior. `#`-prefixed names are kept as-is — EQ named mobs
/// have `#` at the start of their spawn name in the game data.
fn extract_old_filter_name(text: &str) -> Option<String> {
    let name = if text.len() >= 5 && text[..5].eq_ignore_ascii_case("name:") {
        text[5..].trim()
    } else {
        text.trim()
    };
    if name.is_empty() || name.contains(['[', ':', '^', '*']) {
        return None;
    }
    Some(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn write_temp_xml(content: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f
    }

    const SAMPLE_XML: &str = r#"<seqfilters>
  <hunt><item name="Fippy Darkpaw" /></hunt>
  <caution><item name="a gnoll scout" /></caution>
  <danger><item name="Lord Nagafen" /></danger>
  <rare><item name="Lockjaw" /></rare>
</seqfilters>"#;

    #[test]
    fn classify_loaded_entries() {
        let f = write_temp_xml(SAMPLE_XML);
        let set = FilterSet::load(f.path());
        assert_eq!(set.classify("Fippy Darkpaw"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("a gnoll scout"), Some(FilterCategory::Caution));
        assert_eq!(set.classify("Lord Nagafen"), Some(FilterCategory::Danger));
        assert_eq!(set.classify("Lockjaw"), Some(FilterCategory::Rare));
    }

    #[test]
    fn classify_is_case_insensitive() {
        let f = write_temp_xml(SAMPLE_XML);
        let set = FilterSet::load(f.path());
        assert_eq!(set.classify("fippy darkpaw"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("LORD NAGAFEN"), Some(FilterCategory::Danger));
    }

    #[test]
    fn classify_returns_none_for_unknown() {
        let f = write_temp_xml(SAMPLE_XML);
        let set = FilterSet::load(f.path());
        assert_eq!(set.classify("Xygoz"), None);
    }

    #[test]
    fn classify_matches_substring() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Hunt, "gnoll");
        assert_eq!(set.classify("a gnoll scout"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("a gnoll warrior"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("GNOLL SHAMAN"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("orc pawn"), None);
    }

    #[test]
    fn classify_highest_priority_wins_on_multiple_substring_matches() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Hunt, "gnoll");
        set.add(FilterCategory::Danger, "gnoll lord");
        assert_eq!(set.classify("a gnoll lord"), Some(FilterCategory::Danger));
    }

    #[test]
    fn higher_priority_wins_on_merge() {
        let mut base = FilterSet::new();
        base.add(FilterCategory::Hunt, "Pox");
        let mut overlay = FilterSet::new();
        overlay.add(FilterCategory::Danger, "Pox");
        base.merge(overlay);
        assert_eq!(base.classify("Pox"), Some(FilterCategory::Danger));
    }

    #[test]
    fn lower_priority_does_not_overwrite() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Danger, "Boss");
        set.add(FilterCategory::Hunt, "Boss");
        assert_eq!(set.classify("Boss"), Some(FilterCategory::Danger));
    }

    #[test]
    fn missing_file_returns_empty_set() {
        let set = FilterSet::load(Path::new("does_not_exist.xml"));
        assert!(set.is_empty());
    }

    #[test]
    fn remove_drops_entry() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Hunt, "Fippy");
        set.remove("Fippy");
        assert!(set.classify("Fippy").is_none());
        assert!(set.is_empty());
    }

    #[test]
    fn remove_is_case_insensitive() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Danger, "Nagafen");
        set.remove("NAGAFEN");
        assert!(set.is_empty());
    }

    #[test]
    fn save_and_reload_round_trips() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Danger, "Lord Nagafen");
        set.add(FilterCategory::Hunt, "Fippy Darkpaw");
        set.add(FilterCategory::Caution, "a gnoll");
        set.add(FilterCategory::Rare, "Lockjaw");
        let tmp = tempfile::NamedTempFile::new().unwrap();
        set.save(tmp.path()).unwrap();
        let loaded = FilterSet::load(tmp.path());
        assert_eq!(
            loaded.classify("Lord Nagafen"),
            Some(FilterCategory::Danger)
        );
        assert_eq!(loaded.classify("Fippy Darkpaw"), Some(FilterCategory::Hunt));
        assert_eq!(loaded.classify("a gnoll"), Some(FilterCategory::Caution));
        assert_eq!(loaded.classify("Lockjaw"), Some(FilterCategory::Rare));
    }

    const LEGACY_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE seqfilters SYSTEM "seqfilters.dtd">
<seqfilters>
  <section name="Hunt">
    <oldfilter><regex>Name:Fippy Darkpaw</regex></oldfilter>
    <oldfilter><regex>Name:a gnoll scout</regex></oldfilter>
  </section>
  <section name="Danger">
    <oldfilter><regex>Name:Lord Nagafen</regex></oldfilter>
  </section>
  <section name="Alert">
    <oldfilter><regex>Name:Lockjaw</regex></oldfilter>
  </section>
  <section name="Filtered">
    <oldfilter><regex>Name:ShouldBeSkipped</regex></oldfilter>
  </section>
</seqfilters>"#;

    #[test]
    fn legacy_format_loads_correctly() {
        let f = write_temp_xml(LEGACY_XML);
        let set = FilterSet::load(f.path());
        assert_eq!(set.classify("Fippy Darkpaw"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("a gnoll scout"), Some(FilterCategory::Hunt));
        assert_eq!(set.classify("Lord Nagafen"), Some(FilterCategory::Danger));
        assert_eq!(set.classify("Lockjaw"), Some(FilterCategory::Rare));
        assert_eq!(set.classify("ShouldBeSkipped"), None);
    }

    #[test]
    fn legacy_format_migrates_on_load() {
        let f = write_temp_xml(LEGACY_XML);
        FilterSet::load(f.path());
        // After load, file should be rewritten in new format
        let contents = std::fs::read_to_string(f.path()).unwrap();
        assert!(contents.contains("<hunt>"));
        assert!(!contents.contains("<section"));
        // Reload the migrated file and verify it still classifies correctly
        let set2 = FilterSet::load(f.path());
        assert_eq!(set2.classify("Fippy Darkpaw"), Some(FilterCategory::Hunt));
        assert_eq!(set2.classify("Lord Nagafen"), Some(FilterCategory::Danger));
        assert_eq!(set2.classify("Lockjaw"), Some(FilterCategory::Rare));
    }

    #[test]
    fn extract_old_filter_name_strips_prefix() {
        assert_eq!(
            extract_old_filter_name("Name:Fippy Darkpaw"),
            Some("Fippy Darkpaw".to_owned())
        );
        assert_eq!(
            extract_old_filter_name("name:fippy darkpaw"),
            Some("fippy darkpaw".to_owned())
        );
        // '#' prefix is part of EQ named mob names — keep as-is
        assert_eq!(
            extract_old_filter_name("Name:#Fippy"),
            Some("#Fippy".to_owned())
        );
        assert_eq!(extract_old_filter_name("Name:"), None);
        assert_eq!(extract_old_filter_name("Name:some[regex]"), None);
        assert_eq!(extract_old_filter_name("Name:^anchored"), None);
        assert_eq!(extract_old_filter_name("Name:wild*card"), None);
    }

    #[test]
    fn save_xml_escapes_special_chars() {
        let mut set = FilterSet::new();
        set.add(FilterCategory::Hunt, "a & b");
        let tmp = tempfile::NamedTempFile::new().unwrap();
        set.save(tmp.path()).unwrap();
        let contents = std::fs::read_to_string(tmp.path()).unwrap();
        assert!(contents.contains("&amp;"));
        // round-trip still works
        let loaded = FilterSet::load(tmp.path());
        assert_eq!(loaded.classify("a & b"), Some(FilterCategory::Hunt));
    }
}
