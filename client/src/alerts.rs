use std::collections::HashSet;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::data::spawns::SpawnInfo;

const ZONE_SUPPRESS_SECS: u64 = 10;

/// How an alert fires for a given filter category.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum AlertMode {
    #[default]
    None,
    Beep,
    Speech,
    SoundFile(String),
}

impl AlertMode {
    pub fn from_config(mode_str: &str, sound_path: &str) -> Self {
        match mode_str.to_lowercase().as_str() {
            "beep" => Self::Beep,
            "speech" => Self::Speech,
            "sound" if !sound_path.is_empty() => Self::SoundFile(sound_path.to_owned()),
            "sound" => Self::Beep,
            _ => Self::None,
        }
    }

    #[allow(dead_code)]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Beep => "beep",
            Self::Speech => "speech",
            Self::SoundFile(_) => "sound",
        }
    }
}

enum AudioCmd {
    Speak(String),
    PlayFile(String),
    Beep,
}

/// Fires audio/Discord alerts for filter-flagged spawns.
/// Uses a background audio thread so audio never blocks the network tick.
pub struct AlertEngine {
    tx: mpsc::SyncSender<AudioCmd>,
    alerted: HashSet<u32>,
    suppress_until: Option<Instant>,
    pub danger_mode: AlertMode,
    pub caution_mode: AlertMode,
    pub hunt_mode: AlertMode,
    pub rare_mode: AlertMode,
    pub discord_webhook: String,
    pub discord_on_danger: bool,
    pub discord_on_hunt: bool,
}

impl Default for AlertEngine {
    fn default() -> Self {
        let (tx, rx) = mpsc::sync_channel::<AudioCmd>(32);
        spawn_audio_thread(rx);
        Self {
            tx,
            alerted: HashSet::new(),
            suppress_until: None,
            danger_mode: AlertMode::None,
            caution_mode: AlertMode::None,
            hunt_mode: AlertMode::None,
            rare_mode: AlertMode::None,
            discord_webhook: String::new(),
            discord_on_danger: false,
            discord_on_hunt: false,
        }
    }
}

impl AlertEngine {
    /// Clear alerted set and suppress all alerts for 10 seconds.
    /// Call on every zone change.
    pub fn on_zone_change(&mut self) {
        self.alerted.clear();
        self.suppress_until = Some(Instant::now() + Duration::from_secs(ZONE_SUPPRESS_SECS));
    }

    /// Check a spawn and fire an alert if it is newly flagged and not suppressed.
    /// Returns a log message if an alert fired (for the caller to log).
    pub fn check_spawn(&mut self, spawn: &SpawnInfo) -> Option<String> {
        if self.alerted.contains(&spawn.id) {
            return None;
        }
        if let Some(until) = self.suppress_until
            && Instant::now() < until {
                return None;
            }

        let (mode, category) = if spawn.is_danger {
            (&self.danger_mode, "Danger")
        } else if spawn.is_caution {
            (&self.caution_mode, "Caution")
        } else if spawn.is_hunt {
            (&self.hunt_mode, "Hunt")
        } else if spawn.is_rare {
            (&self.rare_mode, "Rare")
        } else {
            return None;
        };

        // Spoken text matches C# format: "Hunt Mob, Fippy, is up."
        let spoken = format!("{} Mob, {}, is up.", category, tts_name(&spawn.name));

        match mode {
            AlertMode::None => return None,
            AlertMode::Beep => {
                let _ = self.tx.try_send(AudioCmd::Beep);
            }
            AlertMode::Speech => {
                let _ = self.tx.try_send(AudioCmd::Speak(spoken.clone()));
            }
            AlertMode::SoundFile(path) => {
                let _ = self.tx.try_send(AudioCmd::PlayFile(path.clone()));
            }
        }

        self.alerted.insert(spawn.id);

        let send_discord = (spawn.is_danger && self.discord_on_danger)
            || (spawn.is_hunt && self.discord_on_hunt);
        if send_discord && !self.discord_webhook.is_empty() {
            let url = self.discord_webhook.clone();
            let msg = format!(
                "{}: {} at ({:.0}, {:.0})",
                category, spawn.name, spawn.x, spawn.y
            );
            std::thread::spawn(move || {
                post_discord(&url, &msg);
            });
        }

        Some(format!("Alert [{}]: {}", category, spawn.name))
    }
}

fn spawn_audio_thread(rx: mpsc::Receiver<AudioCmd>) {
    std::thread::Builder::new()
        .name("wseq-audio".to_owned())
        .spawn(move || {
            let mut tts_engine = tts::Tts::default().ok();
            for cmd in rx {
                match cmd {
                    AudioCmd::Beep => play_beep(),
                    AudioCmd::Speak(text) => {
                        if let Some(t) = tts_engine.as_mut() {
                            let _ = t.speak(&text, false);
                        }
                    }
                    AudioCmd::PlayFile(path) => play_wav_file(&path),
                }
            }
        })
        .expect("failed to spawn audio thread");
}

fn play_beep() {
    use rodio::source::{SineWave, Source};
    use rodio::stream::DeviceSinkBuilder;
    use rodio::Player;

    let Ok(device_sink) = DeviceSinkBuilder::open_default_sink() else {
        return;
    };
    let player = Player::connect_new(device_sink.mixer());
    let source = SineWave::new(300.0)
        .take_duration(Duration::from_millis(100))
        .amplify(0.5);
    player.append(source);
    player.sleep_until_end();
}

fn play_wav_file(path: &str) {
    use rodio::stream::DeviceSinkBuilder;
    use rodio::{Decoder, Player};
    use std::fs::File;
    use std::io::BufReader;

    let Ok(device_sink) = DeviceSinkBuilder::open_default_sink() else {
        return;
    };
    let player = Player::connect_new(device_sink.mixer());
    let Ok(file) = File::open(path) else {
        return;
    };
    let Ok(source) = Decoder::new(BufReader::new(file)) else {
        return;
    };
    player.append(source);
    player.sleep_until_end();
}

/// Convert a raw EQ spawn name to a TTS-friendly string.
/// Replaces underscores with spaces and strips trailing instance-number suffixes.
fn tts_name(name: &str) -> String {
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = base.trim_end_matches('_');
    base.replace('_', " ")
}

fn post_discord(url: &str, message: &str) {
    // Simple JSON construction — avoids serde dependency.
    let escaped = message.replace('\\', "\\\\").replace('"', "\\\"");
    let body = format!(r#"{{"content":"{}"}}"#, escaped);
    let _ = ureq::post(url)
        .header("Content-Type", "application/json")
        .send(body.as_bytes());
}