use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Client configuration loaded from `client.ini`.
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
    /// Directory for per-zone annotation files.
    pub annotations_dir: String,
    /// Directory for dated log files.
    pub log_dir: String,
    /// Directory for filter XML files (dev default: "cfg"; installed default: "filters").
    pub filter_dir: String,
    /// Directory containing EQ native map files ({zone}_1.txt etc.).
    pub map_dir: String,

    // ── Alert modes — "none" | "beep" | "speech" | "sound" ──────────────────
    pub alert_danger_mode: String,
    pub alert_danger_sound: String,
    pub alert_caution_mode: String,
    pub alert_caution_sound: String,
    pub alert_hunt_mode: String,
    pub alert_hunt_sound: String,
    pub alert_rare_mode: String,
    pub alert_rare_sound: String,

    // ── Discord ───────────────────────────────────────────────────────────────
    pub discord_webhook: String,
    pub discord_on_danger: bool,
    pub discord_on_hunt: bool,

    // ── EverQuest installation ────────────────────────────────────────────────
    /// Path to the EverQuest installation folder (for loading dbstr_us.txt).
    pub eq_path: String,

    // ── Logging ───────────────────────────────────────────────────────────────
    pub log_enabled: bool,
    /// Minimum log level: "debug" | "info" | "warn" | "error"
    pub log_level: String,

    // ── Startup ───────────────────────────────────────────────────────────────
    /// Connect to the server automatically on startup without showing the Connect dialog.
    pub auto_connect: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            server_ip: "127.0.0.1".to_owned(),
            server_port: 5555,
            update_delay_ms: 250,
            cfg_dir: "cfg".to_owned(),
            timer_dir: "timers".to_owned(),
            annotations_dir: "annotations".to_owned(),
            log_dir: "logs".to_owned(),
            filter_dir: "cfg".to_owned(),
            map_dir: "maps".to_owned(),
            alert_danger_mode: "speech".to_owned(),
            alert_danger_sound: String::new(),
            alert_caution_mode: "beep".to_owned(),
            alert_caution_sound: String::new(),
            alert_hunt_mode: "beep".to_owned(),
            alert_hunt_sound: String::new(),
            alert_rare_mode: "none".to_owned(),
            alert_rare_sound: String::new(),
            discord_webhook: String::new(),
            discord_on_danger: false,
            discord_on_hunt: false,
            eq_path: String::new(),
            log_enabled: true,
            log_level: "info".to_owned(),
            auto_connect: false,
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
            fs::create_dir_all(parent)?;
        }
        let mut f = fs::File::create(path)?;
        writeln!(f, "[WinShowEQ]")?;
        writeln!(f, "Server={}", self.server_ip)?;
        writeln!(f, "Port={}", self.server_port)?;
        writeln!(f, "Rate={}", self.update_delay_ms)?;
        writeln!(f, "AutoConnect={}", if self.auto_connect { 1 } else { 0 })?;
        writeln!(f)?;
        writeln!(f, "[Directories]")?;
        writeln!(f, "CfgDir={}", self.cfg_dir)?;
        writeln!(f, "TimerDir={}", self.timer_dir)?;
        writeln!(f, "AnnotationsDir={}", self.annotations_dir)?;
        writeln!(f, "LogDir={}", self.log_dir)?;
        writeln!(f, "FilterDir={}", self.filter_dir)?;
        writeln!(f, "MapDir={}", self.map_dir)?;
        writeln!(f)?;
        writeln!(f, "[Alerts]")?;
        writeln!(f, "DangerMode={}", self.alert_danger_mode)?;
        writeln!(f, "DangerSound={}", self.alert_danger_sound)?;
        writeln!(f, "CautionMode={}", self.alert_caution_mode)?;
        writeln!(f, "CautionSound={}", self.alert_caution_sound)?;
        writeln!(f, "HuntMode={}", self.alert_hunt_mode)?;
        writeln!(f, "HuntSound={}", self.alert_hunt_sound)?;
        writeln!(f, "AlertMode={}", self.alert_rare_mode)?;
        writeln!(f, "AlertSound={}", self.alert_rare_sound)?;
        writeln!(f)?;
        writeln!(f, "[Discord]")?;
        writeln!(f, "Webhook={}", self.discord_webhook)?;
        writeln!(f, "OnDanger={}", if self.discord_on_danger { 1 } else { 0 })?;
        writeln!(f, "OnHunt={}", if self.discord_on_hunt { 1 } else { 0 })?;
        writeln!(f)?;
        writeln!(f, "[EQ]")?;
        writeln!(f, "Path={}", self.eq_path)?;
        writeln!(f)?;
        writeln!(f, "[Logging]")?;
        writeln!(f, "Enabled={}", if self.log_enabled { 1 } else { 0 })?;
        writeln!(f, "Level={}", self.log_level)?;
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
            if let Some(v) = winshoweq.get("port")
                && let Ok(n) = v.parse() {
                    cfg.server_port = n;
                }
            if let Some(v) = winshoweq.get("rate")
                && let Ok(n) = v.parse() {
                    cfg.update_delay_ms = n;
                }
            if let Some(v) = winshoweq.get("autoconnect") {
                cfg.auto_connect = v == "1";
            }
        }

        if let Some(dirs) = sections.get("directories") {
            if let Some(v) = dirs.get("cfgdir") {
                cfg.cfg_dir = v.clone();
            }
            if let Some(v) = dirs.get("timerdir") {
                cfg.timer_dir = v.clone();
            }
            if let Some(v) = dirs.get("annotationsdir") {
                cfg.annotations_dir = v.clone();
            }
            if let Some(v) = dirs.get("logdir") {
                cfg.log_dir = v.clone();
            }
            if let Some(v) = dirs.get("filterdir") {
                cfg.filter_dir = v.clone();
            }
            if let Some(v) = dirs.get("mapdir") {
                cfg.map_dir = v.clone();
            }
        }

        if let Some(alerts) = sections.get("alerts") {
            if let Some(v) = alerts.get("dangermode") { cfg.alert_danger_mode = v.clone(); }
            if let Some(v) = alerts.get("dangersound") { cfg.alert_danger_sound = v.clone(); }
            if let Some(v) = alerts.get("cautionmode") { cfg.alert_caution_mode = v.clone(); }
            if let Some(v) = alerts.get("cautionsound") { cfg.alert_caution_sound = v.clone(); }
            if let Some(v) = alerts.get("huntmode") { cfg.alert_hunt_mode = v.clone(); }
            if let Some(v) = alerts.get("huntsound") { cfg.alert_hunt_sound = v.clone(); }
            if let Some(v) = alerts.get("alertmode") { cfg.alert_rare_mode = v.clone(); }
            if let Some(v) = alerts.get("alertsound") { cfg.alert_rare_sound = v.clone(); }
        }

        if let Some(discord) = sections.get("discord") {
            if let Some(v) = discord.get("webhook") { cfg.discord_webhook = v.clone(); }
            if let Some(v) = discord.get("ondanger") { cfg.discord_on_danger = v == "1"; }
            if let Some(v) = discord.get("onhunt") { cfg.discord_on_hunt = v == "1"; }
        }

        if let Some(eq) = sections.get("eq")
            && let Some(v) = eq.get("path") { cfg.eq_path = v.clone(); }

        if let Some(logging) = sections.get("logging") {
            if let Some(v) = logging.get("enabled") { cfg.log_enabled = v != "0"; }
            if let Some(v) = logging.get("level") { cfg.log_level = v.clone(); }
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
        let cfg = ClientConfig::load(Path::new("nonexistent_client.ini"));
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