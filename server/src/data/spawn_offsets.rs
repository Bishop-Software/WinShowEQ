use crate::config::IniReader;

/// Byte-level offsets within EQ's spawn struct in memory.
/// Read from [SpawnInfo Offsets] in myseqserver.ini.
/// Mirrors Spawn::init() in Spawn.cpp.
#[derive(Debug, Clone, Default)]
pub struct SpawnOffsets {
    pub name:      usize,
    pub last_name: usize,
    pub x:         usize,
    pub y:         usize,
    pub z:         usize,
    pub speed:     usize,
    pub heading:   usize,
    pub prev:      usize,
    pub next:      usize,
    pub type_:     usize,
    pub level:     usize,
    pub hidden:    usize,
    pub class:     usize,
    pub id:        usize,
    pub owner:     usize,
    pub race:      usize,
    pub primary:   usize,
    pub offhand:   usize,
    /// When true, race is 8-bit and id/owner/primary/offhand are 16-bit.
    pub race8:     bool,
    /// Bytes to read per spawn node = largest field offset + 30.
    pub buf_size:  usize,
}

impl SpawnOffsets {
    pub fn from_ini(ir: &IniReader) -> Self {
        let o = |name: &str| ir.read_integer_entry("SpawnInfo Offsets", name, false) as usize;
        let race8 = ir.read_integer_entry("SpawnInfo Offsets", "EightBitRace", false) != 0;

        let name      = o("NameOffset");
        let last_name = o("LastNameOffset");
        let x         = o("XOffset");
        let y         = o("YOffset");
        let z         = o("ZOffset");
        let speed     = o("SpeedOffset");
        let heading   = o("HeadingOffset");
        let prev      = o("PrevOffset");
        let next      = o("NextOffset");
        let type_     = o("TypeOffset");
        let level     = o("LevelOffset");
        let hidden    = o("HideOffset");
        let class     = o("ClassOffset");
        let id        = o("SpawnIDOffset");
        let owner     = o("OwnerIDOffset");
        let race      = o("RaceOffset");
        let primary   = o("PrimaryOffset");
        let offhand   = o("OffhandOffset");

        let largest = [name, last_name, x, y, z, speed, heading, prev, next,
                       type_, level, hidden, class, id, owner, race, primary, offhand]
            .iter().copied().max().unwrap_or(0);

        Self {
            name, last_name, x, y, z, speed, heading, prev, next,
            type_, level, hidden, class, id, owner, race, primary, offhand,
            race8,
            buf_size: largest + 30,
        }
    }
}

/// Byte-level offsets within EQ's ground item struct in memory.
/// Read from [GroundItem Offsets] in myseqserver.ini.
/// Mirrors Item::init() in Item.cpp.
#[derive(Debug, Clone, Default)]
pub struct ItemOffsets {
    pub prev:     usize,
    pub next:     usize,
    pub id:       usize,
    pub drop_id:  usize,
    pub x:        usize,
    pub y:        usize,
    pub z:        usize,
    pub name:     usize,
    pub buf_size: usize,
}

impl ItemOffsets {
    pub fn from_ini(ir: &IniReader) -> Self {
        let o = |name: &str| ir.read_integer_entry("GroundItem Offsets", name, false) as usize;

        let prev    = o("PrevOffset");
        let next    = o("NextOffset");
        let id      = o("IdOffset");
        let drop_id = o("DropIdOffset");
        let x       = o("XOffset");
        let y       = o("YOffset");
        let z       = o("ZOffset");
        let name    = o("NameOffset");

        let largest = [prev, next, id, drop_id, x, y, z, name]
            .iter().copied().max().unwrap_or(0);

        Self { prev, next, id, drop_id, x, y, z, name, buf_size: largest + 30 }
    }
}

/// Byte-level offsets within EQ's world info struct in memory.
/// Read from [WorldInfo Offsets] in myseqserver.ini.
/// Mirrors World::init() in World.cpp.
#[derive(Debug, Clone, Default)]
pub struct WorldOffsets {
    pub hour:     usize,
    pub minute:   usize,
    pub day:      usize,
    pub month:    usize,
    pub year:     usize,
    /// When true, year is 16-bit (same EightBitRace flag as spawn).
    pub race8:    bool,
    pub buf_size: usize,
}

impl WorldOffsets {
    pub fn from_ini(ir: &IniReader) -> Self {
        let o = |name: &str| ir.read_integer_entry("WorldInfo Offsets", name, false) as usize;
        let race8 = ir.read_integer_entry("SpawnInfo Offsets", "EightBitRace", false) != 0;

        let hour   = o("WorldHourOffset");
        let minute = o("WorldMinuteOffset");
        let day    = o("WorldDayOffset");
        let month  = o("WorldMonthOffset");
        let year   = o("WorldYearOffset");

        let largest = [hour, minute, day, month, year].iter().copied().max().unwrap_or(0);

        Self { hour, minute, day, month, year, race8, buf_size: largest + 30 }
    }
}