/// In-game time representation (mirrors worldBuffer_t in World.h).
/// Transmitted as an OPT_WORLD SpawnRecord with fields repurposed:
/// spawn_type=hour, class=minute, level=day, hidden=month, race=year.
#[derive(Debug, Clone, Copy, Default)]
pub struct WorldTime {
    pub hour:   u8,
    pub minute: u8,
    pub day:    u8,
    pub month:  u8,
    pub year:   u32,
}