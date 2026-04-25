use windows::Win32::System::WindowsProgramming::{
    GetPrivateProfileStringW, WritePrivateProfileStringW,
};
use windows::core::PCWSTR;

/// Primary memory offsets read from [Memory Offsets] in myseqserver.ini.
/// Mirrors PrimaryOffsets in IniReader.h.
#[derive(Debug, Clone, Default)]
pub struct PrimaryOffsets {
    pub spawn_list: u64,
    pub self_addr: u64,
    pub target: u64,
    pub zone_name: u64,
    pub ground: u64,
    pub world: u64,
}

/// Validated runtime configuration loaded from myseqserver.ini.
/// Mirrors ServerConfigModel in IniReader.h.
#[derive(Debug, Clone, Default)]
pub struct ServerConfigModel {
    pub port: u32,
    pub offsets: PrimaryOffsets,
}

fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Parses a string as an unsigned integer.
/// Treats the value as hexadecimal when it starts with "0x", "0X", or "0"
/// (matching C++ IniReader::readIntegerEntry behavior).
fn parse_integer(s: &str) -> u64 {
    if s.is_empty() {
        return 0;
    }
    if s.starts_with("0x") || s.starts_with("0X") {
        u64::from_str_radix(&s[2..], 16).unwrap_or(0)
    } else if s.starts_with('0') {
        u64::from_str_radix(s, 16).unwrap_or(0)
    } else {
        s.parse::<u64>().unwrap_or(0)
    }
}

/// Decodes `\xNN` hex escape sequences in `s` into raw bytes, discarding all other chars.
/// Used to store binary pattern data as human-readable text in config.ini.
pub fn parse_escape_bytes(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() && b[i + 1] == b'x'
            && i + 3 < b.len()
            && b[i + 2].is_ascii_hexdigit()
            && b[i + 3].is_ascii_hexdigit()
        {
            let val = (hex_nibble(b[i + 2]) << 4) | hex_nibble(b[i + 3]);
            result.push(val);
            i += 4;
        } else {
            i += 1;
        }
    }
    result
}

fn hex_nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

/// Parses a string as hex/decimal u64, returning an error for missing or malformed values.
/// Used by read_server_config_model; mirrors the anonymous parseUnsignedQword helper in
/// IniReader.cpp.
fn parse_required_u64(raw: &str, section: &str, entry: &str) -> Result<u64, String> {
    if raw.is_empty() {
        return Err(format!("Missing INI key [{}] {}.", section, entry));
    }
    let (hex_str, base) = if raw.starts_with("0x") || raw.starts_with("0X") {
        (&raw[2..], 16u32)
    } else if raw.starts_with('0') {
        (raw, 16u32)
    } else {
        (raw, 10u32)
    };
    u64::from_str_radix(hex_str, base)
        .map_err(|_| format!("Invalid numeric value for [{}] {}: '{}'.", section, entry, raw))
}

/// INI file reader backed by GetPrivateProfileStringW / WritePrivateProfileStringW.
/// Mirrors IniReader in IniReader.h / IniReader.cpp.
pub struct IniReader {
    filename: String,
    config_filename: String,
    pub patch_date: String,
    pub start_minimized: bool,
}

impl IniReader {
    pub fn new() -> Self {
        Self {
            filename: String::new(),
            config_filename: String::new(),
            patch_date: String::new(),
            start_minimized: false,
        }
    }

    /// Loads myseqserver.ini. Validates that [File Info] PatchDate exists.
    pub fn open_file(&mut self, filename: &str) -> Result<(), String> {
        self.filename = filename.to_string();
        self.patch_date = self.read_string_entry("File Info", "PatchDate", false);
        if self.patch_date.is_empty() {
            return Err(format!("Error: IniReader: Invalid INI file {}", filename));
        }
        println!("IniReader: Reading INI file");
        println!("IniFile: {}", filename);
        println!("Patch Date: {}", self.patch_date);
        Ok(())
    }

    /// Loads config.ini and reads [Server] StartMinimized.
    pub fn open_config_file(&mut self, filename: &str) {
        self.config_filename = filename.to_string();
        let val = self.read_string_entry("Server", "StartMinimized", true);
        self.start_minimized = val.trim() == "1";
        println!("IniReader: Reading Config INI file");
        println!("ConfigIniFile: {}", filename);
    }

    pub fn read_string_entry(&self, section: &str, entry: &str, config: bool) -> String {
        let file = if config { &self.config_filename } else { &self.filename };
        if file.is_empty() {
            return String::new();
        }
        let section_w = to_wide(section);
        let entry_w = to_wide(entry);
        let default_w = to_wide("");
        let file_w = to_wide(file);
        let mut buf = vec![0u16; 256];
        let len = unsafe {
            GetPrivateProfileStringW(
                PCWSTR(section_w.as_ptr()),
                PCWSTR(entry_w.as_ptr()),
                PCWSTR(default_w.as_ptr()),
                Some(&mut buf),
                PCWSTR(file_w.as_ptr()),
            )
        };
        if len == 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize]).to_owned()
    }

    pub fn read_integer_entry(&self, section: &str, entry: &str, config: bool) -> u64 {
        let raw = self.read_string_entry(section, entry, config);
        parse_integer(&raw)
    }

    /// Reads port + all six primary offsets from myseqserver.ini.
    /// Returns an error listing every missing or invalid key (mirrors C++ readServerConfigModel).
    pub fn read_server_config_model(&self) -> Result<ServerConfigModel, String> {
        let mut errors: Vec<String> = Vec::new();

        macro_rules! read_offset {
            ($section:expr, $entry:expr) => {{
                let raw = self.read_string_entry($section, $entry, false);
                match parse_required_u64(&raw, $section, $entry) {
                    Ok(v) => v,
                    Err(e) => { errors.push(e); 0 }
                }
            }};
        }

        let port_raw = self.read_string_entry("Port", "Port", false);
        let port_val = match parse_required_u64(&port_raw, "Port", "Port") {
            Ok(v) => v,
            Err(e) => { errors.push(e); 0 }
        };

        let spawn_list = read_offset!("Memory Offsets", "SpawnHeaderAddr");
        let self_addr   = read_offset!("Memory Offsets", "CharInfo");
        let target       = read_offset!("Memory Offsets", "TargetAddr");
        let zone_name    = read_offset!("Memory Offsets", "ZoneAddr");
        let ground       = read_offset!("Memory Offsets", "ItemsAddr");
        let world        = read_offset!("Memory Offsets", "WorldAddr");

        if port_val > u32::MAX as u64 {
            errors.push("Port value is out of range for u32.".to_string());
        }

        if !errors.is_empty() {
            let mut msg = "Error: IniReader: Invalid INI server config.".to_string();
            for e in &errors {
                msg.push_str("\n - ");
                msg.push_str(e);
            }
            return Err(msg);
        }

        Ok(ServerConfigModel {
            port: port_val as u32,
            offsets: PrimaryOffsets { spawn_list, self_addr, target, zone_name, ground, world },
        })
    }

    pub fn write_string_entry(&self, section: &str, entry: &str, value: &str, config: bool) -> bool {
        let file = if config { &self.config_filename } else { &self.filename };
        if file.is_empty() {
            return false;
        }
        let section_w = to_wide(section);
        let entry_w   = to_wide(entry);
        let value_w   = to_wide(value);
        let file_w    = to_wide(file);

        let result = unsafe {
            WritePrivateProfileStringW(
                PCWSTR(section_w.as_ptr()),
                PCWSTR(entry_w.as_ptr()),
                PCWSTR(value_w.as_ptr()),
                PCWSTR(file_w.as_ptr()),
            )
        };
        result.is_ok()
    }

    /// Reads a value from config.ini and decodes `\xNN` hex escape sequences into raw bytes.
    /// All non-escape characters are discarded; only decoded bytes are returned.
    /// Mirrors IniReader::readEscapeStrings in IniReader.cpp.
    pub fn read_escape_bytes(&self, section: &str, entry: &str) -> Vec<u8> {
        let raw = self.read_string_entry(section, entry, true);
        parse_escape_bytes(&raw)
    }

    pub fn toggle_start_minimized(&mut self) {
        self.start_minimized = !self.start_minimized;
        let val = if self.start_minimized { "1" } else { "0" };
        self.write_string_entry("Server", "StartMinimized", val, true);
    }
}

impl Default for IniReader {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_ini_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        p.push(name);
        p
    }

    #[test]
    fn round_trip_string_entry() {
        let path = temp_ini_path("winshowed_test_rtrip.ini");
        let path_str = path.to_str().unwrap();

        let reader = IniReader::new();
        assert_eq!(reader.write_string_entry("TestSection", "TestKey", "HelloWorld", false), false, "write should fail when filename is empty");

        let mut reader = IniReader::new();
        reader.filename = path_str.to_string();

        assert!(reader.write_string_entry("TestSection", "TestKey", "HelloWorld", false));
        let result = reader.read_string_entry("TestSection", "TestKey", false);
        assert_eq!(result, "HelloWorld");

        // Clean up
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn parse_integer_hex_and_decimal() {
        assert_eq!(parse_integer("0x1A3F"), 0x1A3F);
        assert_eq!(parse_integer("0X1A3F"), 0x1A3F);
        assert_eq!(parse_integer("5555"), 5555);
        assert_eq!(parse_integer(""), 0);
        assert_eq!(parse_integer("0"), 0);
    }
}