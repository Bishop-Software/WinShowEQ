use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Client configuration loaded from `winshoweq-client.ini`.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Server IP address.
    pub server_ip: String,
    /// Server TCP port.
    pub server_port: u16,
    /// Tick interval in milliseconds (how often the client polls the server).
    pub update_delay_ms: u32,
    /// Directory for zone filter XML files (hunt/caution/danger/alert).
    pub cfg_dir: String,
    /// Directory for respawn timer files.
    pub timer_dir: String,
    /// Directory for dated log files.
    pub log_dir: String,
    /// Directory for filter XML files (often same as cfg_dir).
    pub filter_dir: String,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_ip: "127.0.0.1".to_owned(),
            server_port: 5555,
            update_delay_ms: 250,
            cfg_dir: "cfg".to_owned(),
            timer_dir: "timers".to_owned(),
            log_dir: "logs".to_owned(),
            filter_dir: "cfg".to_owned(),
        }
    }
}

impl ClientConfig {
    /// Load from `path`. Missing keys fall back to defaults.
    /// Returns defaults if the file is missing or unreadable.
    /// Persist config to `path` in INI format.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write as _;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut f = std::fs::File::create(path)?;
        writeln!(f, "[WinShowEQ]")?;
        writeln!(f, "Server={}", self.server_ip)?;
        writeln!(f, "Port={}", self.server_port)?;
        writeln!(f, "Rate={}", self.update_delay_ms)?;
        writeln!(f)?;
        writeln!(f, "[Directories]")?;
        writeln!(f, "CfgDir={}", self.cfg_dir)?;
        writeln!(f, "TimerDir={}", self.timer_dir)?;
        writeln!(f, "LogDir={}", self.log_dir)?;
        writeln!(f, "FilterDir={}", self.filter_dir)?;
        Ok(())
    }

    pub fn load(path: &Path) -> Self {
        let ini = match fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => return Self::default(),
        };
        let sections = parse_ini(&ini);
        let mut cfg = Self::default();

        if let Some(winshoweq) = sections.get("winshoweq") {
            if let Some(v) = winshoweq.get("server") {
                cfg.server_ip = v.clone();
            }
            if let Some(v) = winshoweq.get("port") {
                if let Ok(n) = v.parse() {
                    cfg.server_port = n;
                }
            }
            if let Some(v) = winshoweq.get("rate") {
                if let Ok(n) = v.parse() {
                    cfg.update_delay_ms = n;
                }
            }
        }

        if let Some(dirs) = sections.get("directories") {
            if let Some(v) = dirs.get("cfgdir") {
                cfg.cfg_dir = v.clone();
            }
            if let Some(v) = dirs.get("timerdir") {
                cfg.timer_dir = v.clone();
            }
            if let Some(v) = dirs.get("logdir") {
                cfg.log_dir = v.clone();
            }
            if let Some(v) = dirs.get("filterdir") {
                cfg.filter_dir = v.clone();
            }
        }

        cfg
    }
}

/// Minimal INI parser. Returns `section_name → (key → value)`, all lowercase.
/// Handles `[Section]`, `key=value`, and `;`/`#` line comments.
fn parse_ini(text: &str) -> HashMap<String, HashMap<String, String>> {
    let mut map: HashMap<String, HashMap<String, String>> = HashMap::new();
    let mut current_section = String::new();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if let Some(end) = line.find(']') {
                current_section = line[1..end].trim().to_lowercase();
            }
        } else if let Some(eq) = line.find('=') {
            let key = line[..eq].trim().to_lowercase();
            let val = line[eq + 1..].trim().to_owned();
            map.entry(current_section.clone())
                .or_default()
                .insert(key, val);
        }
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_defaults_when_file_missing() {
        let cfg = ClientConfig::load(Path::new("nonexistent_winshoweq-client.ini"));
        assert_eq!(cfg.server_ip, "127.0.0.1");
        assert_eq!(cfg.server_port, 5555);
        assert_eq!(cfg.update_delay_ms, 250);
    }

    #[test]
    fn parse_ini_reads_sections_and_keys() {
        let ini = "[WinShowEQ]\nServer=192.168.1.1\nPort=9000\nRate=500\n\
                   [Directories]\nCfgDir=mycfg\nLogDir=mylogs\n";
        let sections = parse_ini(ini);
        assert_eq!(sections["winshoweq"]["server"], "192.168.1.1");
        assert_eq!(sections["winshoweq"]["port"], "9000");
        assert_eq!(sections["directories"]["cfgdir"], "mycfg");
    }

    #[test]
    fn parse_ini_ignores_comments() {
        let ini = "; this is a comment\n[WinShowEQ]\n# also a comment\nServer=10.0.0.1\n";
        let sections = parse_ini(ini);
        assert_eq!(sections["winshoweq"]["server"], "10.0.0.1");
    }
}